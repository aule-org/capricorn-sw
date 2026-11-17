//! Tutoring sessions: who may start one, the room token, what the agent
//! reports while it runs, and what happens to it afterwards.
//!
//! A session exists from the moment its token is issued. Everything the agent
//! later says about it is checked against the token of that one session.

use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use chrono::Utc;
use rand::Rng;
use serde::Deserialize;
use serde_json::{Value, json};
use sqlx::PgPool;
use uuid::Uuid;

use crate::auth::{Agent, Caller, Input, Operator, Session, token_hash};
use crate::error::{ApiError, ApiResult};
use crate::families::{Child, child_for, current_consent, find_child};
use crate::livekit::{RoomToken, room_token};
use crate::state::AppState;
use crate::supervisor::{self, Brief, PlanInputs, Turn};
use crate::{events, jobs, learning, memory};

pub const AUDIENCES: [&str; 2] = ["child", "operator"];
/// How long the agent waits for the supervisor's plan before starting with
/// the learner model's own.
const BRIEF_WAIT: std::time::Duration = std::time::Duration::from_secs(6);
const MAX_TURN_CHARS: usize = 4000;
const MAX_TURNS_PER_REQUEST: usize = 20;

#[derive(Deserialize)]
pub struct NewSession {
    #[serde(default)]
    child_id: Option<Uuid>,
    /// "child", or "operator" for an adult on the team trying the tutor.
    audience: String,
    #[serde(default)]
    language: Option<String>,
}

pub async fn create_session(
    caller: Caller,
    State(state): State<AppState>,
    Input(body): Input<NewSession>,
) -> ApiResult<(StatusCode, Json<Value>)> {
    if !AUDIENCES.contains(&body.audience.as_str()) {
        return Err(ApiError::bad_request("audience must be child or operator"));
    }
    if body.language.as_deref().is_some_and(|language| !["es", "en"].contains(&language)) {
        return Err(ApiError::bad_request("language must be es or en"));
    }
    // Only the people running the server can open an operator's session.
    if caller != Caller::Operator && body.audience != "child" {
        return Err(ApiError::Forbidden("a guardian can only start a child's session"));
    }
    let child = match body.child_id {
        Some(child_id) => Some(child_for(&state.pool, &caller, child_id).await?),
        None if body.audience == "child" => {
            return Err(ApiError::bad_request("a child's session needs child_id"));
        }
        None => None,
    };
    let session = open_session(&state, child, &body.audience, body.language, None).await?;
    Ok((StatusCode::CREATED, Json(session)))
}

/// Creates a session and its room token. Every way of starting one comes
/// through here, so the consent rule is in one place.
pub async fn open_session(
    state: &AppState,
    child: Option<Child>,
    audience: &str,
    language: Option<String>,
    device_id: Option<Uuid>,
) -> ApiResult<Value> {
    let livekit = state.config.livekit.as_ref().ok_or(ApiError::Unavailable("LiveKit is not configured"))?;

    // A child's profile is only used with the guardian's consent, whoever is
    // at the microphone. Without consent for voice there is no session.
    let (keep_transcript, use_memory) = match &child {
        Some(child) => {
            let consent = current_consent(&state.pool, child.id).await?;
            if !consent["voice"] {
                return Err(ApiError::Forbidden("the guardian has not consented to voice sessions"));
            }
            (consent["transcripts"], consent["memory"])
        }
        None => (false, false),
    };
    let language =
        language.or(child.as_ref().map(|child| child.language.clone())).unwrap_or_else(|| "es".to_string());

    let session_id = Uuid::now_v7();
    let mut secret = [0u8; 32];
    rand::rng().fill_bytes(&mut secret);
    let agent_token = hex::encode(secret);
    let room = format!("session-{}", session_id.simple());
    sqlx::query(
        "insert into sessions
           (id, child_id, audience, room, language, agent_token_hash, keep_transcript, use_memory, device_id)
         values ($1, $2, $3, $4, $5, $6, $7, $8, $9)",
    )
    .bind(session_id)
    .bind(child.as_ref().map(|child| child.id))
    .bind(audience)
    .bind(&room)
    .bind(&language)
    .bind(token_hash(&agent_token))
    .bind(keep_transcript)
    .bind(use_memory)
    .bind(device_id)
    .execute(&state.pool)
    .await?;

    // The plan is written while the person connects, so nobody waits for it.
    if let Some(child) = child {
        let state = state.clone();
        tokio::spawn(async move {
            if let Err(error) = prepare_brief(&state, session_id, &child, use_memory).await {
                tracing::warn!(%session_id, %error, "no brief was prepared");
            }
        });
    }

    let ttl_seconds = i64::from(state.config.session_minutes) * 60;
    let dispatch_metadata = json!({
        "audience": audience,
        "language": language,
        "session_id": session_id,
        "platform_token": agent_token,
    });
    let token = room_token(
        livekit,
        &RoomToken {
            room: &room,
            identity: &format!("{audience}-{}", session_id.simple()),
            name: if audience == "child" { "Student" } else { "Operator" },
            ttl_seconds,
            agent_name: &state.config.agent_name,
            dispatch_metadata: &dispatch_metadata,
        },
        Utc::now().timestamp(),
    );
    Ok(json!({
        "session_id": session_id,
        "serverUrl": livekit.public_url,
        "token": token,
        "room": room,
        "agentName": state.config.agent_name,
        "expiresInSeconds": ttl_seconds,
    }))
}

async fn prepare_brief(state: &AppState, session_id: Uuid, child: &Child, use_memory: bool) -> ApiResult<()> {
    let now = Utc::now();
    let trace = learning::trace_child(&state.pool, child.id, now).await?;
    let catalog = learning::catalog(&state.pool).await?;
    let candidates = learning::candidates(&trace.skills, &catalog, child.grade.as_deref(), now);
    let memories = if use_memory { memory::latest(&state.pool, child.id, 8).await? } else { Vec::new() };
    let last_next_step: Option<String> = sqlx::query_scalar(
        "select next_step from session_reviews where child_id = $1 order by created_at desc limit 1",
    )
    .bind(child.id)
    .fetch_optional(&state.pool)
    .await?
    .flatten();

    let brief = match &state.llm {
        Some(llm) => {
            let inputs = PlanInputs {
                child: &child.context(now),
                candidates: &candidates,
                memories: &memories,
                last_next_step: last_next_step.as_deref(),
                minutes: state.config.session_minutes,
            };
            supervisor::plan(llm.as_ref(), &inputs).await.unwrap_or_else(|error| {
                tracing::warn!(%session_id, %error, "planning fell back to the learner model");
                supervisor::fallback_brief(&candidates)
            })
        }
        None => supervisor::fallback_brief(&candidates),
    };
    sqlx::query("insert into session_briefs (session_id, brief) values ($1, $2) on conflict do nothing")
        .bind(session_id)
        .bind(sqlx::types::Json(&brief))
        .execute(&state.pool)
        .await?;
    Ok(())
}

async fn stored_brief(pool: &PgPool, session_id: Uuid) -> Result<Option<Brief>, sqlx::Error> {
    let brief: Option<sqlx::types::Json<Brief>> =
        sqlx::query_scalar("select brief from session_briefs where session_id = $1")
            .bind(session_id)
            .fetch_optional(pool)
            .await?;
    Ok(brief.map(|brief| brief.0))
}

/// What the tutor is told about the student, in words.
fn student_line(child: &Child) -> String {
    let mut line = format!("The student's name is {}.", child.nickname);
    if let Some(age) = child.age(Utc::now()) {
        line.push_str(&format!(" They are about {age} years old."));
    }
    if let Some(grade) = &child.grade {
        line.push_str(&format!(" School grade: {grade}."));
    }
    if !child.interests.is_empty() {
        line.push_str(&format!(" They like: {}.", child.interests.join(", ")));
    }
    line
}

/// The agent's first call: marks the session as running and returns what the
/// tutor should know before it says hello.
pub async fn agent_start(Agent(session): Agent, State(state): State<AppState>) -> ApiResult<Json<Value>> {
    if session.status == "ended" {
        return Err(ApiError::Conflict("the session has ended"));
    }
    sqlx::query(
        "update sessions set status = 'active', started_at = now() where id = $1 and status = 'created'",
    )
    .bind(session.id)
    .execute(&state.pool)
    .await?;

    let Some(child_id) = session.child_id else {
        return Ok(Json(json!({ "session_id": session.id, "instructions": "" })));
    };
    let child = find_child(&state.pool, child_id).await?;
    let deadline = tokio::time::Instant::now() + BRIEF_WAIT;
    let brief = loop {
        if let Some(brief) = stored_brief(&state.pool, session.id).await? {
            break Some(brief);
        }
        if tokio::time::Instant::now() >= deadline {
            break None;
        }
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    };
    let instructions = match &brief {
        Some(brief) => format!("{}\n\n{}", student_line(&child), brief.as_instructions()),
        None => student_line(&child),
    };
    Ok(Json(json!({ "session_id": session.id, "instructions": instructions, "brief": brief })))
}

#[derive(Deserialize)]
pub struct TurnInput {
    seq: i32,
    role: String,
    content: String,
}

#[derive(Deserialize)]
pub struct TurnsInput {
    turns: Vec<TurnInput>,
    /// The id of the last guidance the agent already has.
    #[serde(default)]
    after_guidance: i64,
}

/// The agent reports what was just said and collects any guidance written
/// since it last asked. It answers at once: the supervisor reads the new
/// turns after this call returns.
pub async fn agent_turns(
    Agent(session): Agent,
    State(state): State<AppState>,
    Input(body): Input<TurnsInput>,
) -> ApiResult<Json<Value>> {
    if session.status == "ended" {
        return Err(ApiError::Conflict("the session has ended"));
    }
    if body.turns.len() > MAX_TURNS_PER_REQUEST {
        return Err(ApiError::bad_request("too many turns in one request"));
    }
    let mut child_spoke = false;
    for turn in &body.turns {
        if !["child", "tutor"].contains(&turn.role.as_str()) || turn.seq < 0 {
            return Err(ApiError::bad_request("a turn needs seq >= 0 and role child or tutor"));
        }
        let content: String = turn.content.trim().chars().take(MAX_TURN_CHARS).collect();
        if content.is_empty() {
            continue;
        }
        // The same turn sent twice is stored once, and read by the supervisor once.
        let stored = sqlx::query(
            "insert into session_turns (session_id, seq, role, content) values ($1, $2, $3, $4)
             on conflict (session_id, seq) do nothing",
        )
        .bind(session.id)
        .bind(turn.seq)
        .bind(&turn.role)
        .bind(content)
        .execute(&state.pool)
        .await?
        .rows_affected()
            > 0;
        child_spoke |= stored && turn.role == "child";
    }

    if child_spoke && state.llm.is_some() && state.watching.lock().expect("lock").insert(session.id) {
        let (state, session) = (state.clone(), session.clone());
        tokio::spawn(async move {
            if let Err(error) = watch(&state, &session).await {
                tracing::warn!(session_id = %session.id, %error, "the supervisor could not read the session");
            }
            state.watching.lock().expect("lock").remove(&session.id);
        });
    }

    let guidance: Vec<(i64, String)> = sqlx::query_as(
        "select id, guidance from session_guidance where session_id = $1 and id > $2 order by id",
    )
    .bind(session.id)
    .bind(body.after_guidance)
    .fetch_all(&state.pool)
    .await?;
    let guidance: Vec<Value> = guidance
        .into_iter()
        .map(|(id, text)| {
            let instructions = supervisor::Guidance { text, signals: Default::default() }.as_instructions();
            json!({ "id": id, "instructions": instructions })
        })
        .collect();
    Ok(Json(json!({ "guidance": guidance })))
}

pub async fn load_turns(pool: &PgPool, session_id: Uuid) -> Result<Vec<Turn>, sqlx::Error> {
    let rows: Vec<(i32, String, String)> =
        sqlx::query_as("select seq, role, content from session_turns where session_id = $1 order by seq")
            .bind(session_id)
            .fetch_all(pool)
            .await?;
    Ok(rows.into_iter().map(|(seq, role, content)| Turn { seq, role, content }).collect())
}

async fn watch(state: &AppState, session: &Session) -> ApiResult<()> {
    let Some(llm) = &state.llm else { return Ok(()) };
    let turns = load_turns(&state.pool, session.id).await?;
    let Some(last) = turns.last() else { return Ok(()) };
    let brief = stored_brief(&state.pool, session.id).await?.unwrap_or_default();
    let memories = match (session.child_id, session.use_memory) {
        (Some(child_id), true) => {
            let said =
                turns.iter().rev().find(|turn| turn.role == "child").map_or("", |turn| turn.content.as_str());
            memory::recall(&state.pool, state.embedder.as_deref(), child_id, said, 3).await?
        }
        _ => Vec::new(),
    };
    let guidance = supervisor::watch(llm.as_ref(), &brief, &turns, &memories)
        .await
        .map_err(|error| ApiError::Internal(error.to_string()))?;
    if !guidance.is_empty() {
        sqlx::query(
            "insert into session_guidance (session_id, after_seq, guidance, signals) values ($1, $2, $3, $4)",
        )
        .bind(session.id)
        .bind(last.seq)
        .bind(&guidance.text)
        .bind(sqlx::types::Json(&guidance.signals))
        .execute(&state.pool)
        .await?;
    }
    Ok(())
}

pub const END_REASONS: [&str; 6] =
    ["child_ended", "idle", "budget_reached", "parent_ended", "device_offline", "error"];

#[derive(Deserialize)]
pub struct EndInput {
    #[serde(default)]
    speaking_seconds: Option<i32>,
    #[serde(default)]
    end_reason: Option<String>,
}

/// Closes the session and queues its review. Ending twice changes nothing.
pub async fn agent_end(
    Agent(session): Agent,
    State(state): State<AppState>,
    Input(body): Input<EndInput>,
) -> ApiResult<StatusCode> {
    let reason = body.end_reason.as_deref().unwrap_or("child_ended");
    if !END_REASONS.contains(&reason) {
        return Err(ApiError::bad_request(format!("end_reason must be one of: {}", END_REASONS.join(", "))));
    }
    let speaking_seconds = body.speaking_seconds.filter(|seconds| (0..=86_400).contains(seconds));
    end_session(&state.pool, session.id, speaking_seconds, reason).await?;
    Ok(StatusCode::NO_CONTENT)
}

type Ended = (Option<Uuid>, Option<Uuid>, chrono::DateTime<Utc>, chrono::DateTime<Utc>);

pub async fn end_session(
    pool: &PgPool,
    session_id: Uuid,
    speaking_seconds: Option<i32>,
    end_reason: &str,
) -> Result<bool, sqlx::Error> {
    let mut tx = pool.begin().await?;
    let ended: Option<Ended> = sqlx::query_as(
        "update sessions set status = 'ended', ended_at = now(), speaking_seconds = $2
         where id = $1 and status <> 'ended'
         returning child_id, device_id, coalesce(started_at, created_at), ended_at",
    )
    .bind(session_id)
    .bind(speaking_seconds)
    .fetch_optional(&mut *tx)
    .await?;
    if let Some((child_id, device_id, started_at, ended_at)) = ended {
        jobs::enqueue(&mut *tx, jobs::REVIEW_SESSION, &json!({ "session_id": session_id })).await?;
        // The event is about a child's session on a device; a session from the
        // playground has neither.
        if let (Some(child_id), Some(device_id)) = (child_id, device_id) {
            let turns: i64 = sqlx::query_scalar("select count(*) from session_turns where session_id = $1")
                .bind(session_id)
                .fetch_one(&mut *tx)
                .await?;
            let data = json!({
                "session_id": session_id,
                "device_id": device_id,
                "child_id": child_id,
                "started_at": started_at.to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
                "ended_at": ended_at.to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
                "duration_seconds": (ended_at - started_at).num_seconds().max(0),
                "end_reason": end_reason,
                "turns": turns,
            });
            events::publish(&mut *tx, events::SESSION_ENDED, session_id, data).await?;
        }
    }
    tx.commit().await?;
    Ok(ended.is_some())
}

pub const FLAG_GATES: [&str; 3] = ["moderation_input", "moderation_output", "physical_safety"];
pub const FLAG_ACTIONS: [&str; 5] = ["redirect", "adult_help", "clarify", "support", "unavailable"];

#[derive(Deserialize)]
pub struct FlagInput {
    gate: String,
    action: String,
    categories: Vec<String>,
}

/// The agent reports that a safety gate stopped a turn: which gate, what it
/// did, and fixed categories. Never what was said.
pub async fn agent_flag(
    Agent(session): Agent,
    State(state): State<AppState>,
    Input(body): Input<FlagInput>,
) -> ApiResult<StatusCode> {
    let is_category = |category: &String| {
        !category.is_empty()
            && category.len() <= 40
            && category.chars().all(|c| c.is_ascii_lowercase() || c == '_' || c == '/')
    };
    let mut categories = body.categories;
    categories.sort();
    categories.dedup();
    if !FLAG_GATES.contains(&body.gate.as_str())
        || !FLAG_ACTIONS.contains(&body.action.as_str())
        || categories.is_empty()
        || categories.len() > 10
        || !categories.iter().all(is_category)
    {
        return Err(ApiError::bad_request("a flag needs a known gate and action and 1 to 10 categories"));
    }
    let mut tx = state.pool.begin().await?;
    sqlx::query("insert into session_flags (session_id, gate, action, categories) values ($1, $2, $3, $4)")
        .bind(session.id)
        .bind(&body.gate)
        .bind(&body.action)
        .bind(&categories)
        .execute(&mut *tx)
        .await?;
    if let Some(child_id) = session.child_id {
        let data = json!({
            "session_id": session.id,
            "child_id": child_id,
            "gate": body.gate,
            "action": body.action,
            "categories": categories,
        });
        events::publish(&mut *tx, events::SAFETY_FLAGGED, session.id, data).await?;
    }
    tx.commit().await?;
    Ok(StatusCode::NO_CONTENT)
}

/// The work after a session: evidence for the learner model, memories, and a
/// note for the guardian. Then the transcript goes, unless the guardian agreed
/// to keep it for a while. Safe to run again after a failure.
pub async fn review_session(state: &AppState, session_id: Uuid) -> ApiResult<()> {
    let session: Option<Session> = sqlx::query_as(
        "select id, child_id, audience, room, language, status, keep_transcript, use_memory, created_at, ended_at
         from sessions where id = $1 and reviewed_at is null"
    )
    .bind(session_id)
    .fetch_optional(&state.pool)
    .await?;
    let Some(session) = session else { return Ok(()) };

    let turns = load_turns(&state.pool, session.id).await?;
    let child_spoke = turns.iter().any(|turn| turn.role == "child");
    if let (Some(child_id), Some(llm), true) = (session.child_id, &state.llm, child_spoke) {
        let child = find_child(&state.pool, child_id).await?;
        let now = Utc::now();
        let brief = stored_brief(&state.pool, session.id).await?.unwrap_or_default();
        let mut known: Vec<String> =
            learning::catalog(&state.pool).await?.into_iter().map(|skill| skill.id).collect();
        known.extend(
            learning::trace_child(&state.pool, child_id, now).await?.skills.into_iter().map(|s| s.skill_id),
        );
        known.sort();
        known.dedup();
        known.truncate(300);

        let review = supervisor::review(llm.as_ref(), &child.context(now), &brief, &turns, &known)
            .await
            .map_err(|error| ApiError::Internal(error.to_string()))?;

        // A second run replaces what the first one wrote.
        sqlx::query("delete from attempts where session_id = $1 and source = 'review'")
            .bind(session.id)
            .execute(&state.pool)
            .await?;
        sqlx::query("delete from memories where session_id = $1")
            .bind(session.id)
            .execute(&state.pool)
            .await?;

        let at = session.ended_at.unwrap_or(now);
        let attempts: Vec<learning::NewAttempt> = review
            .evidence
            .iter()
            .map(|item| learning::NewAttempt {
                skill_id: item.skill_id.clone(),
                question_id: None,
                correct: item.correct,
                credit: item.credit,
                choice: false,
                at: Some(at),
            })
            .collect();
        learning::record(&state.pool, child_id, Some(session.id), "review", &attempts, now).await?;

        if session.use_memory {
            let embedder = state.embedder.as_deref();
            if let Err(error) =
                memory::remember(&state.pool, embedder, child_id, Some(session.id), &review.memories).await
            {
                // A memory without an embedding is still found by date.
                tracing::warn!(%session_id, %error, "memories were stored without embeddings");
                memory::remember(&state.pool, None, child_id, Some(session.id), &review.memories)
                    .await
                    .map_err(|error| ApiError::Internal(error.to_string()))?;
            }
        }

        sqlx::query(
            "insert into session_reviews (session_id, child_id, guardian_summary, next_step, flags)
             values ($1, $2, $3, $4, $5)
             on conflict (session_id) do update set guardian_summary = excluded.guardian_summary,
               next_step = excluded.next_step, flags = excluded.flags",
        )
        .bind(session.id)
        .bind(child_id)
        .bind(&review.guardian_summary)
        .bind(&review.next_step)
        .bind(&review.flags)
        .execute(&state.pool)
        .await?;
        let data = json!({ "session_id": session.id, "child_id": child_id, "flags": review.flags });
        events::publish(&state.pool, events::SESSION_REVIEWED, session.id, data).await?;
    }

    if !session.keep_transcript {
        sqlx::query("delete from session_turns where session_id = $1")
            .bind(session.id)
            .execute(&state.pool)
            .await?;
    }
    sqlx::query("update sessions set reviewed_at = now() where id = $1")
        .bind(session.id)
        .execute(&state.pool)
        .await?;
    Ok(())
}

/// What the supervisor planned and advised in one session, for the people
/// running the server to see it work.
pub async fn supervisor_view(
    _: Operator,
    State(state): State<AppState>,
    Path(session_id): Path<Uuid>,
) -> ApiResult<Json<Value>> {
    let status: Option<String> = sqlx::query_scalar("select status from sessions where id = $1")
        .bind(session_id)
        .fetch_optional(&state.pool)
        .await?;
    let status = status.ok_or(ApiError::NotFound("session"))?;
    let brief = stored_brief(&state.pool, session_id).await?;
    let guidance: Vec<(i64, i32, String, sqlx::types::Json<supervisor::Signals>)> = sqlx::query_as(
        "select id, after_seq, guidance, signals from session_guidance where session_id = $1 order by id",
    )
    .bind(session_id)
    .fetch_all(&state.pool)
    .await?;
    let guidance: Vec<Value> = guidance
        .into_iter()
        .map(|(id, after_seq, text, signals)| json!({ "id": id, "after_seq": after_seq, "text": text, "signals": signals.0 }))
        .collect();
    Ok(Json(json!({ "status": status, "brief": brief, "guidance": guidance })))
}
