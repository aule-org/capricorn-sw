//! Events: what the platform tells the outside, and itself, about what
//! happened. An event is written to the outbox in the same transaction as the
//! change it describes, and the worker delivers it afterwards, at least once:
//! first to the platform's own consumers, then to the configured webhook.
//!
//! The shape of every event is fixed by `contracts/` at the repository root,
//! and a test checks what this module produces against those schemas. Events
//! carry ids, times, counts and fixed categories, never a child's words.

use chrono::{DateTime, SecondsFormat, Utc};
use hmac::{Hmac, Mac};
use serde_json::{Value, json};
use sha2::Sha256;
use sqlx::PgExecutor;
use uuid::Uuid;

use crate::notices;
use crate::state::AppState;

pub const SOURCE: &str = "capricorn/platform";
pub const SESSION_ENDED: &str = "session.ended.v1";
pub const SESSION_REVIEWED: &str = "session.reviewed.v1";
pub const SAFETY_FLAGGED: &str = "safety.flagged.v1";
pub const DEVICE_CLAIMED: &str = "device.claimed.v1";
const MAX_ATTEMPTS: i32 = 10;

pub async fn publish<'a>(
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
            .header("X-Capricorn-Signature", signature(&webhook.secret, body.as_bytes()))
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

type Pending = (Uuid, String, Option<String>, Value, DateTime<Utc>, i32);

/// Delivers one pending event, oldest first. Returns whether there was one.
/// A consumer may see an event twice, so every consumer must be safe to repeat.
pub async fn deliver_once(state: &AppState) -> Result<bool, sqlx::Error> {
    let mut tx = state.pool.begin().await?;
    let pending: Option<Pending> = sqlx::query_as(
        "select id, type, subject, data, time, attempts from outbox
         where delivered_at is null and attempts < $1
           and time + make_interval(mins => attempts * attempts) <= now()
         order by time for update skip locked limit 1",
    )
    .bind(MAX_ATTEMPTS)
    .fetch_optional(&mut *tx)
    .await?;
    let Some((id, event_type, subject, data, time, attempts)) = pending else { return Ok(false) };

    let event = envelope(id, &event_type, subject.as_deref(), time, &data);
    match deliver(state, &event).await {
        Ok(()) => {
            sqlx::query("update outbox set delivered_at = now(), last_error = null where id = $1")
                .bind(id)
                .execute(&mut *tx)
                .await?;
        }
        Err(error) => {
            tracing::warn!(event = %id, %event_type, attempts, %error, "event not delivered");
            sqlx::query("update outbox set attempts = attempts + 1, last_error = $2 where id = $1")
                .bind(id)
                .bind(error.chars().take(500).collect::<String>())
                .execute(&mut *tx)
                .await?;
        }
    }
    tx.commit().await?;
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
