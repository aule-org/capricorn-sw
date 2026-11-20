//! Events: what the platform tells the outside, and itself, about what
//! happened. An event is written to the outbox in the same transaction as the
//! change it describes, and the worker delivers it afterwards, at least once:
//! first to the platform's own consumers, then to the configured webhook.
//!
//! The shape of every event is fixed by `contracts/` at the repository root,
//! and a test checks what this module produces against those schemas. Events
//! carry ids, times, counts and fixed categories, never a child's words.

use chrono::{DateTime, SecondsFormat, Utc};
use hmac::{Hmac, KeyInit, Mac};
use sea_orm::ActiveValue::Set;
use sea_orm::{ConnectionTrait, DbBackend, DbErr, EntityTrait, Statement, TransactionTrait};
use serde_json::{Value, json};
use sha2::Sha256;
use sqlx::PgExecutor;
use uuid::Uuid;

use crate::entity::outbox;
use crate::notices;
use crate::state::AppState;

pub const SOURCE: &str = "aule/platform";
pub const SESSION_ENDED: &str = "session.ended.v1";
pub const SESSION_REVIEWED: &str = "session.reviewed.v1";
pub const SESSION_STOPPED: &str = "session.stopped.v1";
pub const SAFETY_FLAGGED: &str = "safety.flagged.v1";
pub const DEVICE_CLAIMED: &str = "device.claimed.v1";
const MAX_ATTEMPTS: i32 = 10;

/// Writes the event to the outbox, in the caller's transaction.
pub async fn publish(
    db: &impl ConnectionTrait,
    event_type: &str,
    subject: Uuid,
    data: Value,
) -> Result<(), sea_orm::DbErr> {
    outbox::Entity::insert(outbox::ActiveModel {
        id: Set(Uuid::now_v7()),
        event_type: Set(event_type.to_string()),
        subject: Set(Some(subject.to_string())),
        data: Set(data),
        ..Default::default()
    })
    .exec_without_returning(db)
    .await?;
    Ok(())
}

/// `publish` for a transaction still on SQLx: the device claim, until the
/// devices module moves to SeaORM.
pub async fn publish_sqlx<'a>(
    executor: impl PgExecutor<'a>,
    event_type: &str,
    subject: Uuid,
    data: Value,
) -> Result<(), sqlx::Error> {
    sqlx::query("insert into outbox (id, type, subject, data) values ($1, $2, $3, $4)")
        .bind(Uuid::now_v7())
        .bind(event_type)
        .bind(subject.to_string())
        .bind(data)
        .execute(executor)
        .await?;
    Ok(())
}

/// The event as it travels: a CloudEvents 1.0 envelope in structured JSON.
pub fn envelope(
    id: Uuid,
    event_type: &str,
    subject: Option<&str>,
    time: DateTime<Utc>,
    data: &Value,
) -> Value {
    let mut event = json!({
        "specversion": "1.0",
        "id": id,
        "source": SOURCE,
        "type": event_type,
        "time": time.to_rfc3339_opts(SecondsFormat::Secs, true),
        "datacontenttype": "application/json",
        "data": data,
    });
    if let Some(subject) = subject {
        event["subject"] = json!(subject);
    }
    event
}

/// `sha256=<hex>`: an HMAC of the exact body, so the receiver can check who sent it.
pub fn signature(secret: &str, body: &[u8]) -> String {
    let mut mac = Hmac::<Sha256>::new_from_slice(secret.as_bytes()).expect("HMAC accepts any key length");
    mac.update(body);
    format!("sha256={}", hex::encode(mac.finalize().into_bytes()))
}

async fn deliver(state: &AppState, event: &Value) -> Result<(), String> {
    notices::on_event(state, event).await.map_err(|error| error.to_string())?;
    if let Some(webhook) = &state.config.events_webhook {
        let body = event.to_string();
        let response = reqwest::Client::new()
            .post(&webhook.url)
            .timeout(std::time::Duration::from_secs(10))
            .header("Content-Type", "application/cloudevents+json")
            .header("X-Aule-Signature", signature(&webhook.secret, body.as_bytes()))
            .body(body)
            .send()
            .await
            .map_err(|error| error.without_url().to_string())?;
        if !response.status().is_success() {
            return Err(format!("the webhook answered {}", response.status().as_u16()));
        }
    }
    Ok(())
}

/// Delivers one pending event, oldest first. Returns whether there was one.
/// A consumer may see an event twice, so every consumer must be safe to repeat.
pub async fn deliver_once(state: &AppState) -> Result<bool, DbErr> {
    let txn = state.db.begin().await?;
    let pending = txn
        .query_one_raw(Statement::from_sql_and_values(
            DbBackend::Postgres,
            "select id, type, subject, data, time, attempts from outbox
             where delivered_at is null and attempts < $1
               and time + make_interval(mins => attempts * attempts) <= now()
             order by time for update skip locked limit 1",
            [MAX_ATTEMPTS.into()],
        ))
        .await?;
    let Some(pending) = pending else { return Ok(false) };
    let id: Uuid = pending.try_get("", "id")?;
    let event_type: String = pending.try_get("", "type")?;
    let subject: Option<String> = pending.try_get("", "subject")?;
    let data: Value = pending.try_get("", "data")?;
    let time: DateTime<Utc> = pending.try_get("", "time")?;
    let attempts: i32 = pending.try_get("", "attempts")?;

    let event = envelope(id, &event_type, subject.as_deref(), time, &data);
    match deliver(state, &event).await {
        Ok(()) => {
            txn.execute_raw(Statement::from_sql_and_values(
                DbBackend::Postgres,
                "update outbox set delivered_at = now(), last_error = null where id = $1",
                [id.into()],
            ))
            .await?;
        }
        Err(error) => {
            tracing::warn!(event = %id, %event_type, attempts, %error, "event not delivered");
            txn.execute_raw(Statement::from_sql_and_values(
                DbBackend::Postgres,
                "update outbox set attempts = attempts + 1, last_error = $2 where id = $1",
                [id.into(), error.chars().take(500).collect::<String>().into()],
            ))
            .await?;
        }
    }
    txn.commit().await?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_signature_is_an_hmac_of_the_body() {
        // RFC 4231, test case 2.
        assert_eq!(
            signature("Jefe", b"what do ya want for nothing?"),
            "sha256=5bdcc146bf60754e6a042426089575c75a003f089d2739839dec58b964ec3843"
        );
    }

    #[test]
    fn the_envelope_is_a_cloudevent() {
        let time = "2026-11-17T15:36:48Z".parse().unwrap();
        let event = envelope(Uuid::nil(), SESSION_ENDED, Some("s"), time, &json!({ "a": 1 }));

        assert_eq!(event["specversion"], "1.0");
        assert_eq!(event["time"], "2026-11-17T15:36:48Z");
        assert_eq!(event["subject"], "s");
        assert!(envelope(Uuid::nil(), SESSION_ENDED, None, time, &json!({})).get("subject").is_none());
    }
}
