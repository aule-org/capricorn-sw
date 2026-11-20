//! What a family is told, in the app, by email and with a push to their phone: a summary after each
//! session, and a short note when a safety gate saw something a guardian
//! should know about. Notices are made from events, so they survive a crash
//! and are never sent twice for the same session.
//!
//! An attention note uses fixed text. It says that something came up, never
//! what the child said.

use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use serde_json::{Value, json};
use uuid::Uuid;

use crate::auth::Guardian;
use crate::error::{ApiError, ApiResult};
use crate::events::{SAFETY_FLAGGED, SESSION_REVIEWED, SESSION_STOPPED};
use crate::state::AppState;

/// The gates that listen to the child, and the outcomes a guardian is told about.
const CHILD_GATES: [&str; 2] = ["moderation_input", "physical_safety"];

/// Title and body of an attention note, by what the gate did.
fn attention_text(action: &str, language: &str, nickname: &str) -> Option<(String, String)> {
    let (title, body) = match (action, language) {
        ("support", "en") => (
            format!("{nickname} may need you"),
            format!(
                "During a session, {nickname} said something that made the tutor suggest talking with a trusted adult. The tutor did not continue that topic. It may be nothing, but a calm conversation with {nickname} today is a good idea."
            ),
        ),
        ("support", _) => (
            format!("{nickname} puede necesitarte"),
            format!(
                "Durante una sesión, {nickname} dijo algo que hizo que el tutor le sugiriera conversar con un adulto de confianza. El tutor no siguió con ese tema. Puede no ser nada, pero conviene conversar con {nickname} hoy, con calma."
            ),
        ),
        ("adult_help", "en") => (
            format!("{nickname} asked about something that needs an adult"),
            format!(
                "During a session, {nickname} talked about doing something that could be physically risky. The tutor did not explain how and asked {nickname} to find an adult nearby."
            ),
        ),
        ("adult_help", _) => (
            format!("{nickname} preguntó por algo que necesita a un adulto"),
            format!(
                "Durante una sesión, {nickname} habló de hacer algo que podía ser físicamente riesgoso. El tutor no explicó cómo hacerlo y le pidió buscar a un adulto cercano."
            ),
        ),
        _ => return None,
    };
    Some((title, body))
}

/// What the family reads when a session was closed early because the student
/// was not working. It says what happened, never what the student said.
fn stopped_text(language: &str, nickname: &str) -> (String, String) {
    match language {
        "en" => (
            format!("{nickname}'s session ended early"),
            format!(
                "Aule closed today's session before its time: {nickname} was not working on the activity, and kept going after a reminder. Nothing is wrong. Another session can start whenever {nickname} is ready to work."
            ),
        ),
        _ => (
            format!("La sesión de {nickname} terminó antes de tiempo"),
            format!(
                "Aule cerró la sesión de hoy antes de tiempo: {nickname} no estaba trabajando en la actividad y siguió así después de un aviso. No pasó nada malo. Pueden empezar otra sesión cuando {nickname} tenga ganas de trabajar."
            ),
        ),
    }
}

fn summary_title(language: &str, nickname: &str) -> String {
    match language {
        "en" => format!("{nickname}'s session"),
        _ => format!("La sesión de {nickname}"),
    }
}

/// The platform's own consumer of events. Safe to run twice for one event.
pub async fn on_event(state: &AppState, event: &Value) -> Result<(), sqlx::Error> {
    let data = &event["data"];
    let ids = |field: &str| data[field].as_str().and_then(|id| id.parse::<Uuid>().ok());
    let (Some(session_id), Some(child_id)) = (ids("session_id"), ids("child_id")) else { return Ok(()) };

    let child: Option<(Uuid, String)> =
        sqlx::query_as("select family_id, nickname from children where id = $1")
            .bind(child_id)
            .fetch_optional(&state.pool)
            .await?;
    // The child was erased between the event and now: nothing to tell anyone.
    let Some((family_id, nickname)) = child else { return Ok(()) };
    let language: String =
        sqlx::query_scalar("select language from guardians where family_id = $1 order by created_at limit 1")
            .bind(family_id)
            .fetch_optional(&state.pool)
            .await?
            .unwrap_or_else(|| "es".to_string());

    let (kind, title, body) = match event["type"].as_str() {
        Some(SESSION_REVIEWED) => {
            let summary: Option<String> =
                sqlx::query_scalar("select guardian_summary from session_reviews where session_id = $1")
                    .bind(session_id)
                    .fetch_optional(&state.pool)
                    .await?;
            let Some(summary) = summary else { return Ok(()) };
            ("session_summary", summary_title(&language, &nickname), summary)
        }
        Some(SAFETY_FLAGGED) => {
            let gate = data["gate"].as_str().unwrap_or_default();
            let text = attention_text(data["action"].as_str().unwrap_or_default(), &language, &nickname);
            match (CHILD_GATES.contains(&gate), text) {
                (true, Some((title, body))) => ("attention", title, body),
                _ => return Ok(()),
            }
        }
        Some(SESSION_STOPPED) => {
            let (title, body) = stopped_text(&language, &nickname);
            ("session_stopped", title, body)
        }
        _ => return Ok(()),
    };

    let created: Option<Uuid> = sqlx::query_scalar(
        "insert into notices (id, family_id, child_id, session_id, kind, title, body)
         values ($1, $2, $3, $4, $5, $6, $7)
         on conflict (session_id, kind) do nothing returning id",
    )
    .bind(Uuid::now_v7())
    .bind(family_id)
    .bind(child_id)
    .bind(session_id)
    .bind(kind)
    .bind(&title)
    .bind(&body)
    .fetch_optional(&state.pool)
    .await?;

    if let (Some(notice_id), Some(pusher)) = (created, &state.pusher)
        && crate::push::announce(&state.pool, pusher.as_ref(), family_id, notice_id, kind).await?
    {
        sqlx::query("update notices set pushed_at = now() where id = $1")
            .bind(notice_id)
            .execute(&state.pool)
            .await?;
    }
    if let (Some(notice_id), Some(mailer)) = (created, &state.mailer) {
        let recipients: Vec<String> =
            sqlx::query_scalar("select email::text from guardians where family_id = $1 and notices_by_email")
                .bind(family_id)
                .fetch_all(&state.pool)
                .await?;
        let mut sent = false;
        for email in recipients {
            match mailer.send(&email, &title, &body).await {
                Ok(()) => sent = true,
                // The notice is still in the app; the email is not retried.
                Err(error) => tracing::warn!(%notice_id, %error, "a notice was not emailed"),
            }
        }
        if sent {
            sqlx::query("update notices set emailed_at = now() where id = $1")
                .bind(notice_id)
                .execute(&state.pool)
                .await?;
        }
    }
    Ok(())
}

#[derive(serde::Serialize, sqlx::FromRow)]
struct Notice {
    id: Uuid,
    child_id: Option<Uuid>,
    session_id: Option<Uuid>,
    kind: String,
    title: String,
    body: String,
    created_at: chrono::DateTime<chrono::Utc>,
    read_at: Option<chrono::DateTime<chrono::Utc>>,
}

pub async fn list(guardian: Guardian, State(state): State<AppState>) -> ApiResult<Json<Value>> {
    let notices: Vec<Notice> = sqlx::query_as(
        "select id, child_id, session_id, kind, title, body, created_at, read_at from notices
         where family_id = $1 order by created_at desc limit 50",
    )
    .bind(guardian.family_id)
    .fetch_all(&state.pool)
    .await?;
    Ok(Json(json!({ "notices": notices })))
}

pub async fn mark_read(
    guardian: Guardian,
    State(state): State<AppState>,
    Path(notice_id): Path<Uuid>,
) -> ApiResult<StatusCode> {
    let updated =
        sqlx::query("update notices set read_at = coalesce(read_at, now()) where id = $1 and family_id = $2")
            .bind(notice_id)
            .bind(guardian.family_id)
            .execute(&state.pool)
            .await?;
    if updated.rows_affected() == 0 {
        return Err(ApiError::NotFound("notice"));
    }
    Ok(StatusCode::NO_CONTENT)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_outcomes_a_guardian_should_hear_about_have_text() {
        for action in ["support", "adult_help"] {
            for language in ["es", "en"] {
                let (title, body) = attention_text(action, language, "Mateo").unwrap();
                assert!(title.contains("Mateo") && body.contains("Mateo"));
            }
        }
        for action in ["redirect", "clarify", "unavailable", "cooldown", ""] {
            assert!(attention_text(action, "es", "Mateo").is_none(), "{action}");
        }
    }

    #[test]
    fn a_stopped_session_is_told_in_both_languages() {
        for language in ["es", "en"] {
            let (title, body) = stopped_text(language, "Mateo");
            assert!(title.contains("Mateo") && body.contains("Mateo"));
        }
    }
}
