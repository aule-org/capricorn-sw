//! Work that happens outside a request, in the worker process: the review of
//! a finished session and the housekeeping that keeps data from outliving its
//! purpose. Jobs live in Postgres, so they commit together with the change
//! that caused them and no broker is needed.

use std::time::Duration;

use serde_json::Value;
use sqlx::{PgExecutor, PgPool};
use uuid::Uuid;

use crate::sessions;
use crate::state::AppState;

pub const REVIEW_SESSION: &str = "review_session";
const MAX_ATTEMPTS: i32 = 5;
/// Hours a transcript survives a session whose review keeps failing, when the
/// guardian did not agree to keep transcripts.
const UNREVIEWED_TRANSCRIPT_HOURS: i32 = 2;
/// Hours after which a session that never reported its end is closed.
const STALE_SESSION_HOURS: i32 = 3;

pub async fn enqueue<'a>(
    executor: impl PgExecutor<'a>,
    kind: &str,
    payload: &Value,
) -> Result<(), sqlx::Error> {
    sqlx::query("insert into jobs (kind, payload) values ($1, $2)")
        .bind(kind)
        .bind(payload)
        .execute(executor)
        .await?;
    Ok(())
}

/// Takes the next due job, hiding it from other workers for five minutes.
async fn claim(pool: &PgPool) -> Result<Option<(i64, String, Value, i32)>, sqlx::Error> {
    sqlx::query_as(
        "update jobs set locked_until = now() + interval '5 minutes', attempts = attempts + 1
         where id = (
             select id from jobs
             where done_at is null and run_at <= now() and attempts < $1
               and (locked_until is null or locked_until < now())
             order by run_at for update skip locked limit 1
         )
         returning id, kind, payload, attempts",
    )
    .bind(MAX_ATTEMPTS)
    .fetch_optional(pool)
    .await
}

async fn run(state: &AppState, kind: &str, payload: &Value) -> Result<(), String> {
    match kind {
        REVIEW_SESSION => {
            let session_id: Uuid = payload["session_id"]
                .as_str()
                .and_then(|id| id.parse().ok())
                .ok_or_else(|| "review_session needs session_id".to_string())?;
            sessions::review_session(state, session_id).await.map_err(|error| match error {
                crate::error::ApiError::Internal(detail) => detail,
                other => other.to_string(),
            })
        }
        other => Err(format!("unknown job kind {other}")),
    }
}

/// Runs one job if one is due. Returns whether there was one.
pub async fn work_once(state: &AppState) -> Result<bool, sqlx::Error> {
    let Some((id, kind, payload, attempts)) = claim(&state.pool).await? else { return Ok(false) };
    match run(state, &kind, &payload).await {
        Ok(()) => {
            sqlx::query(
                "update jobs set done_at = now(), locked_until = null, last_error = null where id = $1",
            )
            .bind(id)
            .execute(&state.pool)
            .await?;
        }
        Err(error) => {
            tracing::warn!(job = id, %kind, attempts, %error, "job failed");
            // Wait longer after each failure: 1, 4, 9, 16 minutes.
            sqlx::query(
                "update jobs set locked_until = null, last_error = $2,
                   run_at = now() + make_interval(mins => $3) where id = $1",
            )
            .bind(id)
            .bind(error.chars().take(500).collect::<String>())
            .bind(attempts * attempts)
            .execute(&state.pool)
            .await?;
        }
    }
    Ok(true)
}

/// Deletes what has outlived its purpose and closes what was left open.
pub async fn housekeeping(state: &AppState) -> Result<(), sqlx::Error> {
    let pool = &state.pool;
    let stale: Vec<Uuid> = sqlx::query_scalar(
        "select id from sessions where status <> 'ended' and created_at < now() - make_interval(hours => $1)",
    )
    .bind(STALE_SESSION_HOURS)
    .fetch_all(pool)
    .await?;
    for session_id in stale {
        sessions::end_session(pool, session_id, None).await?;
    }

    let transcripts = sqlx::query(
        "delete from session_turns using sessions
         where session_turns.session_id = sessions.id and sessions.status = 'ended'
           and sessions.ended_at < now() - make_interval(hours =>
               case when sessions.keep_transcript then $1 else $2 end)",
    )
    .bind(state.config.transcript_hours as i32)
    .bind(UNREVIEWED_TRANSCRIPT_HOURS)
    .execute(pool)
    .await?
    .rows_affected();
    if transcripts > 0 {
        tracing::info!(turns = transcripts, "transcripts past their time were deleted");
    }

    sqlx::query("delete from session_guidance using sessions
                 where session_guidance.session_id = sessions.id and sessions.reviewed_at < now() - interval '1 day'")
        .execute(pool)
        .await?;
    sqlx::query("delete from jobs where done_at < now() - interval '7 days'").execute(pool).await?;
    Ok(())
}

pub async fn run_worker(state: AppState) {
    let mut last_housekeeping = tokio::time::Instant::now() - Duration::from_secs(3600);
    loop {
        if last_housekeeping.elapsed() >= Duration::from_secs(60) {
            if let Err(error) = housekeeping(&state).await {
                tracing::error!(%error, "housekeeping failed");
            }
            last_housekeeping = tokio::time::Instant::now();
        }
        match work_once(&state).await {
            Ok(true) => {}
            Ok(false) => tokio::time::sleep(Duration::from_secs(2)).await,
            Err(error) => {
                tracing::error!(%error, "the worker could not reach the database");
                tokio::time::sleep(Duration::from_secs(10)).await;
            }
        }
    }
}
