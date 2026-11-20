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
use crate::{events, flow, jobs, learning, llm, memory};

pub const AUDIENCES: [&str; 2] = ["child", "operator"];
/// How long the agent waits for the supervisor's plan before starting with
/// the learner model's own.
// A plan written without a model is ready at once; one a model writes takes
// seconds, and the greeting does not need it. The tutor does not wait for
// that one: it arrives with the answer to its first report of turns.
const BRIEF_WAIT: std::time::Duration = std::time::Duration::from_millis(250);
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
    /// What the family says this session is for.
    #[serde(default)]
    objective: Option<ObjectiveInput>,
    /// "engine" asks for the session engine to run the session. It does when
    /// it can; see `flow::goal`.
    #[serde(default)]
    flow: Option<String>,
}

const TOKEN_GRACE_SECONDS: i64 = 5 * 60;

pub const OBJECTIVE_KINDS: [&str; 4] = ["homework", "exam_prep", "exam_review", "explore"];

#[derive(Deserialize)]
pub struct ObjectiveInput {
    kind: String,
    #[serde(default)]
    subject: Option<String>,
    #[serde(default)]
    topic: Option<String>,
    #[serde(default)]
    due: Option<chrono::NaiveDate>,
}

impl ObjectiveInput {
    /// The objective as it is stored and handed to the planner.
    fn checked(self) -> ApiResult<Value> {
        if !OBJECTIVE_KINDS.contains(&self.kind.as_str()) {
            return Err(ApiError::bad_request(format!(
                "objective.kind must be one of: {}",
                OBJECTIVE_KINDS.join(", ")
            )));
        }
        let subject = self.subject.filter(|subject| !subject.trim().is_empty());
        if subject
            .as_deref()
            .is_some_and(|subject| subject != "other" && !crate::families::SUBJECTS.contains(&subject))
        {
            return Err(ApiError::bad_request("objective.subject must be a known subject or \"other\""));
        }
        let topic = self.topic.map(|topic| topic.trim().to_string()).filter(|topic| !topic.is_empty());
        if topic.as_deref().is_some_and(|topic| topic.chars().count() > 200) {
            return Err(ApiError::bad_request("objective.topic must have at most 200 characters"));
        }
        Ok(json!({ "kind": self.kind, "subject": subject, "topic": topic, "due": self.due }))
    }
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
    let engine = match body.flow.as_deref() {
        None => false,
        Some(flow::ENGINE) => true,
        Some(_) => return Err(ApiError::bad_request("flow must be \"engine\" when it is sent")),
    };
    let objective = body.objective.map(ObjectiveInput::checked).transpose()?;
    let session = open_session(&state, child, &body.audience, body.language, None, objective, engine).await?;
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
    objective: Option<Value>,
    // Whether the session engine was asked for. It runs the session only for
    // a child and an objective its bank covers; otherwise nothing changes.
    engine: bool,
) -> ApiResult<Value> {
    let livekit = state.config.livekit.as_ref().ok_or(ApiError::Unavailable("LiveKit is not configured"))?;

    // A child's profile is only used with the guardian's consent, whoever is
    // at the microphone. Without consent for voice there is no session.
    let mut tx = state.pool.begin().await?;
    let (keep_transcript, use_memory) = match &child {
        Some(child) => {
            sqlx::query_scalar::<_, Uuid>("select id from children where id = $1 for update")
                .bind(child.id)
                .fetch_optional(&mut *tx)
                .await?
                .ok_or(ApiError::NotFound("child"))?;
            let consent = current_consent(&mut *tx, child.id).await?;
            if !consent["voice"] {
                return Err(ApiError::Forbidden("the guardian has not consented to voice sessions"));
            }
            (consent["transcripts"], consent["memory"])
        }
        None => (false, false),
    };
    let language =
        language.or(child.as_ref().map(|child| child.language.clone())).unwrap_or_else(|| "es".to_string());
    let flow = (engine && child.is_some() && flow::goal(objective.as_ref(), &language).is_some())
        .then_some(flow::ENGINE);
    let flow_start = match (&child, flow) {
        (Some(child), Some(_)) => {
            Some(flow::start_for(&mut tx, child.id, state.config.session_minutes).await?)
        }
        _ => None,
    };

    let session_id = Uuid::now_v7();
    let mut secret = [0u8; 32];
    rand::rng().fill_bytes(&mut secret);
    let agent_token = hex::encode(secret);
    let room = format!("session-{}", session_id.simple());
    sqlx::query(
        "insert into sessions
           (id, child_id, audience, room, language, agent_token_hash, keep_transcript, use_memory, device_id,
            objective, flow, flow_start)
         values ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12)",
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
    .bind(&objective)
    .bind(flow)
    .bind(flow_start)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;

    let tutor = crate::families::tutor(child.as_ref());
    // The plan is written while the person connects, so nobody waits for it.
    if let Some(child) = child {
        let state = state.clone();
        tokio::spawn(async move {
            if let Err(error) = prepare_brief(&state, session_id, &child, use_memory).await {
                tracing::warn!(%session_id, %error, "no brief was prepared");
            }
        });
    }

    // The session lasts `session_minutes`; the room's token outlives it by a
    // few minutes so the goodbye is never cut by an expired token.
    let session_seconds = i64::from(state.config.session_minutes) * 60;
    let ttl_seconds = session_seconds + TOKEN_GRACE_SECONDS;
    let dispatch_metadata = json!({
        "audience": audience,
        "language": language,
        "session_id": session_id,
        "platform_token": agent_token,
        "tutor_name": tutor["name"],
        "tutor_persona": tutor["persona"],
        "tutor_persona_note": tutor["note"],
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
        // What a clock on the page counts down: the session, not the token.
        "expiresInSeconds": session_seconds,
        "flow": flow,
    }))
}

async fn prepare_brief(state: &AppState, session_id: Uuid, child: &Child, use_memory: bool) -> ApiResult<()> {
    let now = Utc::now();
    let trace = learning::trace_child(&state.pool, child.id, now).await?;
    let catalog = learning::catalog(&state.pool).await?;
    let objective: Option<Value> = sqlx::query_scalar("select objective from sessions where id = $1")
        .bind(session_id)
        .fetch_optional(&state.pool)
        .await?
        .flatten();
    // What the family asked for comes first; the learner model's own choices follow.
    let topic = objective.as_ref().and_then(|objective| objective["topic"].as_str()).unwrap_or_default();
    let mut candidates = learning::asked(&trace.skills, &catalog, child.grade.as_deref(), topic);
    for candidate in learning::candidates(&trace.skills, &catalog, child.grade.as_deref(), now) {
        if candidates.iter().all(|asked| asked.skill_id != candidate.skill_id) {
            candidates.push(candidate);
        }
    }
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
                objective: objective.as_ref(),
                minutes: state.config.session_minutes,
            };
            llm::SESSION.scope(session_id, supervisor::plan(llm.as_ref(), &inputs)).await.unwrap_or_else(
                |error| {
                    tracing::warn!(%session_id, %error, "planning fell back to the learner model");
                    supervisor::fallback_brief(&candidates)
                },
            )
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

/// The plan, if it is ready when the tutor starts. It is marked as handed
/// over, so it is not sent a second time with the turns.
async fn brief_at_start(pool: &PgPool, session_id: Uuid) -> Result<Option<Brief>, sqlx::Error> {
    let brief: Option<sqlx::types::Json<Brief>> = sqlx::query_scalar(
        "update session_briefs set sent_at_start = true where session_id = $1 returning brief",
    )
    .bind(session_id)
    .fetch_optional(pool)
    .await?;
    Ok(brief.map(|brief| brief.0))
}

/// The plan, when it was finished after the tutor had started.
async fn late_brief(pool: &PgPool, session_id: Uuid) -> Result<Option<Brief>, sqlx::Error> {
    let brief: Option<sqlx::types::Json<Brief>> =
        sqlx::query_scalar("select brief from session_briefs where session_id = $1 and not sent_at_start")
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
    if !child.subjects.is_empty() {
        line.push_str(&format!(" Their family wants to reinforce: {}.", child.subjects.join(", ")));
    }
    line
}

/// The agent's first call: marks the session as running and returns what the
/// tutor should know before it says hello. The plan is included when it is
/// already written; otherwise it follows in the answer to the turns.
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
        return Ok(Json(
            json!({ "session_id": session.id, "instructions": "", "tutor": crate::families::tutor(None) }),
        ));
    };
    let child = find_child(&state.pool, child_id).await?;
    let deadline = tokio::time::Instant::now() + BRIEF_WAIT;
    let brief = loop {
        if let Some(brief) = brief_at_start(&state.pool, session.id).await? {
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
    // How many sessions this student has started before this one: lets a
    // written greeting vary without the agent keeping anything.
    let session_number: i64 = sqlx::query_scalar(
        "select count(*) from sessions where child_id = $1 and id <> $2 and started_at is not null",
    )
    .bind(child_id)
    .bind(session.id)
    .fetch_one(&state.pool)
    .await?;
    let mut start = json!({
        "session_id": session.id,
        "instructions": instructions,
        "brief": brief,
        "tutor": crate::families::tutor(Some(&child)),
        // For lines that are written, not generated. Never logged.
        "student_name": child.nickname,
        "session_number": session_number,
    });
    // The engine opens its sessions itself: these are the tutor's first lines.
    if flow::runs(&session) {
        start["script"] = flow::open(&state, &session).await?;
    }
    Ok(Json(start))
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
    /// How many breaks the session has taken so far, as the agent counts them.
    #[serde(default)]
    breaks_taken: Option<i32>,
}

/// A break that is offered and not taken within this long was declined.
const OFFER_STANDS_SECONDS: i64 = 120;
/// No break is offered this close to the end of the session.
const NO_BREAK_IN_THE_LAST_MINUTES: i64 = 10;

/// Whether to offer a break now. A rule, not a model: a block of work is over,
/// or the supervisor read tiredness since the last break; never in the middle
/// of an exercise and never near the end.
async fn pause(
    state: &AppState,
    session: &Session,
    breaks_taken: Option<i32>,
) -> Result<Option<&'static str>, sqlx::Error> {
    // The agent took a break: the next block starts now.
    if let Some(taken) = breaks_taken.filter(|taken| (0..=20).contains(taken)) {
        sqlx::query(
            "update sessions set breaks = $2, last_break_at = now(), pause_offered_at = null, tired_at = null
             where id = $1 and breaks < $2",
        )
        .bind(session.id)
        .bind(taken)
        .execute(&state.pool)
        .await?;
    }
    // An offer nobody took was a no: count the block again from here.
    sqlx::query(
        "update sessions set last_break_at = now(), pause_offered_at = null, tired_at = null
         where id = $1 and pause_offered_at < now() - make_interval(secs => $2)",
    )
    .bind(session.id)
    .bind(OFFER_STANDS_SECONDS as f64)
    .execute(&state.pool)
    .await?;

    let due: Option<(bool,)> = sqlx::query_as(
        "select true from sessions s
         where s.id = $1 and s.status = 'active' and s.stop_step is distinct from 'end'
           and now() < coalesce(s.started_at, s.created_at) + make_interval(mins => $3 - $4)
           and (now() >= coalesce(s.last_break_at, s.started_at, s.created_at) + make_interval(mins => $2)
                or s.tired_at is not null
                or s.pause_offered_at is not null)
           and not exists (select 1 from session_exercises e where e.session_id = s.id and e.status = 'open')",
    )
    .bind(session.id)
    .bind(state.config.break_every_minutes as i32)
    .bind(state.config.session_minutes as i32)
    .bind(NO_BREAK_IN_THE_LAST_MINUTES as i32)
    .fetch_optional(&state.pool)
    .await?;
    if due.is_none() {
        return Ok(None);
    }
    sqlx::query("update sessions set pause_offered_at = coalesce(pause_offered_at, now()) where id = $1")
        .bind(session.id)
        .execute(&state.pool)
        .await?;
    Ok(Some("offer"))
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
    let mut tutor_said = Vec::new();
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
        .bind(&content)
        .execute(&state.pool)
        .await?
        .rows_affected()
            > 0;
        child_spoke |= stored && turn.role == "child";
        if stored && turn.role == "tutor" {
            tutor_said.push(content);
        }
    }
    crate::practice::note_answer_said(&state.pool, session.id, &tutor_said).await?;

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
    // Repeated on every answer: the agent keeps the first one it sees.
    let plan = late_brief(&state.pool, session.id).await?.map(|brief| brief.as_instructions());
    // "warn" or "end", repeated while it holds: the agent says each once.
    let stop: Option<String> = sqlx::query_scalar("select stop_step from sessions where id = $1")
        .bind(session.id)
        .fetch_one(&state.pool)
        .await?;
    // "offer" while a break is due; the agent says it once and reports it taken.
    // The engine gives its own breaks, as lines of its script: a second offer
    // from here would be a second system of breaks.
    let pause = if flow::runs(&session) { None } else { pause(&state, &session, body.breaks_taken).await? };
    let mut answer = json!({ "guidance": guidance, "plan": plan, "stop": stop, "pause": pause });
    // What the engine answered to something typed on the page, for the tutor to say.
    if flow::runs(&session)
        && let Some(script) = flow::queued(&state.pool, session.id).await?
    {
        answer["script"] = script;
    }
    Ok(Json(answer))
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
    let open_exercise = crate::practice::open_exercise(&state.pool, session.id).await?;
    let reading = supervisor::watch(llm.as_ref(), &brief, &turns, &memories, open_exercise.as_ref());
    let guidance = llm::SESSION
        .scope(session.id, reading)
        .await
        .map_err(|error| ApiError::Internal(error.to_string()))?;
    count_not_trying(state, session, guidance.signals.not_trying).await?;
    if guidance.signals.tired {
        sqlx::query("update sessions set tired_at = coalesce(tired_at, now()) where id = $1")
            .bind(session.id)
            .execute(&state.pool)
            .await?;
    }
    // The engine decides what the tutor does next; advice would pull against
    // it. What the supervisor read (not working, tired) still counts above.
    if !guidance.is_empty() && !flow::runs(session) {
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

pub const END_REASONS: [&str; 7] =
    ["child_ended", "idle", "budget_reached", "parent_ended", "device_offline", "not_working", "error"];

/// Turns in a row in which the student is plainly not working before the
/// tutor says so, and before the session closes and the family is told.
const WARN_AFTER: i32 = 2;
const STOP_AFTER: i32 = 4;

/// Counts the turn the supervisor just read and decides whether to warn or
/// close. The decision is the platform's, by count: the model only says
/// whether this one turn was fooling around.
async fn count_not_trying(state: &AppState, session: &Session, not_trying: bool) -> Result<(), sqlx::Error> {
    let mut tx = state.pool.begin().await?;
    let step: Option<(i32, Option<String>)> = sqlx::query_as(
        "update sessions set not_trying_streak = case when $2 then not_trying_streak + 1 else 0 end
         where id = $1 and status <> 'ended' returning not_trying_streak, stop_step",
    )
    .bind(session.id)
    .bind(not_trying)
    .fetch_optional(&mut *tx)
    .await?;
    let Some((streak, before)) = step else { return Ok(()) };
    // Once the close is asked for it stands, whatever is said afterwards.
    let now = match (before.as_deref(), streak) {
        (Some("end"), _) => Some("end"),
        (_, streak) if streak >= STOP_AFTER => Some("end"),
        (_, streak) if streak >= WARN_AFTER => Some("warn"),
        _ => None,
    };
    if now != before.as_deref() {
        sqlx::query("update sessions set stop_step = $2 where id = $1")
            .bind(session.id)
            .bind(now)
            .execute(&mut *tx)
            .await?;
        if let (Some("end"), Some(child_id)) = (now, session.child_id) {
            let data = json!({ "session_id": session.id, "child_id": child_id, "reason": "not_working" });
            events::publish(&mut *tx, events::SESSION_STOPPED, session.id, data).await?;
        }
    }
    tx.commit().await
}

#[derive(Deserialize)]
pub struct EndInput {
    #[serde(default)]
    speaking_seconds: Option<i32>,
    #[serde(default)]
    end_reason: Option<String>,
    /// What the agent measured about the session: names and numbers only.
    #[serde(default)]
    metrics: Option<Value>,
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
    // An exercise still open when the session closes was left, not failed.
    sqlx::query(
        "update session_exercises set status = 'dropped', resolved_at = now() where session_id = $1 and status = 'open'",
    )
    .bind(session.id)
    .execute(&state.pool)
    .await?;
    if let Some(metrics) = body.metrics.as_ref().and_then(crate::metrics::numbers_only) {
        sqlx::query("update sessions set agent_metrics = $2 where id = $1 and status <> 'ended'")
            .bind(session.id)
            .bind(metrics)
            .execute(&state.pool)
            .await?;
    }
    end_session(&state.pool, session.id, speaking_seconds, reason).await?;
    flow::forget(&state, session.id);
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
        // How much each side spoke, counted now: the turns themselves are
        // deleted after the review unless the family keeps transcripts.
        sqlx::query(
            "update sessions set
               child_turns = (select count(*) from session_turns where session_id = $1 and role = 'child'),
               tutor_turns = (select count(*) from session_turns where session_id = $1 and role = 'tutor'),
               child_chars = (select coalesce(sum(char_length(content)), 0) from session_turns
                              where session_id = $1 and role = 'child'),
               tutor_chars = (select coalesce(sum(char_length(content)), 0) from session_turns
                              where session_id = $1 and role = 'tutor')
             where id = $1",
        )
        .bind(session_id)
        .execute(&mut *tx)
        .await?;
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
        "select id, child_id, audience, room, language, status, keep_transcript, use_memory, created_at, ended_at, flow
         from sessions where id = $1 and reviewed_at is null"
    )
    .bind(session_id)
    .fetch_optional(&state.pool)
    .await?;
    let Some(session) = session else { return Ok(()) };

    // A session the engine ran is told to the family in the engine's words,
    // and leaves the child's level and its graded exercises behind. Done
    // first: it needs the session's inputs, which do not outlive a review
    // that keeps failing.
    let engine_report = flow::conclude(state, &session).await?;
    if let (Some(report), Some(child_id)) = (&engine_report, session.child_id) {
        sqlx::query(
            "insert into session_reviews (session_id, child_id, guardian_summary) values ($1, $2, $3)
             on conflict (session_id) do update set guardian_summary = excluded.guardian_summary",
        )
        .bind(session.id)
        .bind(child_id)
        .bind(report)
        .execute(&state.pool)
        .await?;
    }
    let mut family_told = false;

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

        let context = child.context(now);
        let reviewing = supervisor::review(llm.as_ref(), &context, &brief, &turns, &known);
        let review = llm::SESSION
            .scope(session_id, reviewing)
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
        // Skills the tutor's exercises were checked on already have their
        // evidence, from the check; the review's reading of them is left out.
        let checked: Vec<String> = sqlx::query_scalar(
            "select distinct skill_id from attempts where session_id = $1 and source in ('tutor', 'engine')",
        )
        .bind(session.id)
        .fetch_all(&state.pool)
        .await?;
        let attempts: Vec<learning::NewAttempt> = review
            .evidence
            .iter()
            .filter(|item| !checked.contains(&item.skill_id))
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
             on conflict (session_id) do update set
               guardian_summary = case when $6 then session_reviews.guardian_summary
                                       else excluded.guardian_summary end,
               next_step = excluded.next_step, flags = excluded.flags",
        )
        .bind(session.id)
        .bind(child_id)
        .bind(&review.guardian_summary)
        .bind(&review.next_step)
        .bind(&review.flags)
        // The engine's report, once written, is what the family reads: the
        // supervisor adds its next step and flags, not its own summary.
        .bind(flow::runs(&session))
        .execute(&state.pool)
        .await?;
        let data = json!({ "session_id": session.id, "child_id": child_id, "flags": review.flags });
        events::publish(&state.pool, events::SESSION_REVIEWED, session.id, data).await?;
        family_told = true;
    }
    // Without a supervisor the engine's report still reaches the family.
    if let (false, Some(_), Some(child_id)) = (family_told, &engine_report, session.child_id) {
        let data = json!({ "session_id": session.id, "child_id": child_id, "flags": [] });
        events::publish(&state.pool, events::SESSION_REVIEWED, session.id, data).await?;
    }

    // Consent may have changed while the external review was running.
    sqlx::query(
        "delete from session_turns using sessions
         where session_turns.session_id = sessions.id and sessions.id = $1 and not sessions.keep_transcript",
    )
    .bind(session.id)
    .execute(&state.pool)
    .await?;
    flow::delete_inputs_after_review(&state.pool, session.id).await?;
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
    // What the family is told once the session has been reviewed.
    let review: Option<(String, Option<String>, Vec<String>)> = sqlx::query_as(
        "select guardian_summary, next_step, flags from session_reviews where session_id = $1",
    )
    .bind(session_id)
    .fetch_optional(&state.pool)
    .await?;
    let review = review.map(|(guardian_summary, next_step, flags)| {
        json!({ "guardian_summary": guardian_summary, "next_step": next_step, "flags": flags })
    });
    let exercises = crate::practice::exercises(&state.pool, session_id).await?;
    Ok(Json(json!({
        "status": status, "brief": brief, "guidance": guidance, "review": review, "exercises": exercises,
    })))
}
