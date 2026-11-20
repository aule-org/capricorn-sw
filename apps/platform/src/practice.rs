//! How a session moves through an exercise: find out first, teach if needed,
//! practise until it holds.
//!
//! The rules come from the tutoring demo that preceded this platform, where a
//! rule engine and not a language model decided the course of it. They are adapted to
//! a tutor that speaks and makes up its own exercises:
//!
//! - Before teaching a skill, ask a couple of questions with no hints and no
//!   "right" or "wrong", to see what the student can already do.
//! - If those go well the skill is known: move on. If not, teach, then practise.
//! - In practice a wrong answer gets a hint aimed at the mistake, then another,
//!   then a worked example with other numbers. The exercise stays the same.
//! - When hints run out, the exercise is worked out together and the idea is
//!   taught again, and practice starts over.
//! - Practice is done after a few exercises solved, each from a different angle.
//!
//! The tutor is told what to do next in a sentence; it is never told the
//! answer of an exercise that is still open.

use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;
use chrono::Utc;
use sea_orm::ActiveValue::Set;
use sea_orm::sea_query::{Expr, ExprTrait};
use sea_orm::{ColumnTrait, DatabaseConnection, DbErr, EntityTrait, QueryFilter, QueryOrder, QuerySelect};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use uuid::Uuid;

use crate::algebra::{Problem, Shape};
use crate::auth::{Agent, Input, Session};
use crate::entity::{session, session_exercise as exercise_row, skill};
use crate::error::{ApiError, ApiResult};
use crate::exercise::{Exercise, Form, Misconception, Reading, Verdict, check, is_about_something_else};
use crate::learning::{self, NewAttempt};
use crate::state::AppState;

/// Questions asked to find the level, per skill and session.
pub const PROBES: usize = 2;
/// Exercises to solve before a skill is done for the session.
pub const PRACTICE_TARGET: usize = 3;
/// Two questions that point the way, then a worked example with other numbers.
pub const HINTS: i32 = 3;
/// What an answer is worth to the learner model when it needed help.
const CREDIT_WITH_HINTS: f64 = 0.5;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    Open,
    Correct,
    Failed,
    Dropped,
}

impl Status {
    /// As it is stored.
    fn as_str(self) -> &'static str {
        match self {
            Self::Open => "open",
            Self::Correct => "correct",
            Self::Failed => "failed",
            Self::Dropped => "dropped",
        }
    }

    /// A stored status; anything else is read as dropped, which counts for nothing.
    fn stored(text: &str) -> Self {
        match text {
            "open" => Self::Open,
            "correct" => Self::Correct,
            "failed" => Self::Failed,
            _ => Self::Dropped,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Stage {
    /// Finding out what the student can do.
    Diagnose,
    Practice,
    /// Nothing more to do on this skill today.
    Done,
}

/// An exercise of this session on one skill, as far as the rules care.
#[derive(Clone, Copy, Debug)]
pub struct Past {
    pub probe: bool,
    pub status: Status,
}

/// Where a skill stands in the session, from the exercises done on it.
pub fn stage(past: &[Past]) -> Stage {
    let resolved = |item: &&Past| matches!(item.status, Status::Correct | Status::Failed);
    let probes: Vec<&Past> = past.iter().filter(|item| item.probe).filter(resolved).collect();
    if probes.len() < PROBES {
        return Stage::Diagnose;
    }
    if probes.iter().all(|probe| probe.status == Status::Correct) {
        return Stage::Done;
    }
    // Solved since the last one that had to be worked out together.
    let solved = past
        .iter()
        .filter(|item| !item.probe)
        .filter(resolved)
        .rev()
        .take_while(|item| item.status == Status::Correct)
        .count();
    if solved >= PRACTICE_TARGET { Stage::Done } else { Stage::Practice }
}

/// What the tutor does next.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Next {
    /// No number was understood: ask for the answer again.
    AskAgain,
    /// Same value, other form: ask for the form.
    FixForm,
    /// Ask another question to find the level.
    Probe,
    Hint,
    WorkedExample,
    /// Teach the idea, then practise.
    Teach,
    NextExercise,
    /// This skill is done for today.
    MoveOn,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Directive {
    pub stage: Stage,
    pub next: Next,
    /// What to do, said to the tutor.
    pub say: String,
    /// The answer, only once the exercise is closed and it helps to have it.
    pub reveal: Option<String>,
}

/// What changes on the exercise, and what the tutor is told.
#[derive(Clone, Debug, PartialEq)]
pub struct Outcome {
    pub status: Status,
    pub attempts: i32,
    pub hints: i32,
    pub first_try: Option<bool>,
    pub directive: Directive,
}

/// The exercise an answer is for.
#[derive(Clone, Copy, Debug)]
pub struct Current<'a> {
    pub probe: bool,
    pub attempts: i32,
    pub hints: i32,
    pub answer: &'a str,
    pub form: Form,
}

const ANGLES: [&str; 4] = [
    "a direct calculation",
    "a short word problem from something they like",
    "a worked answer with one mistake in it, for them to find",
    "the same kind of sum with one number missing",
];

fn aimed_at(misconception: Option<Misconception>) -> &'static str {
    match misconception {
        Some(Misconception::AddedAcross) => {
            "They added tops and bottoms: ask what the bottom number tells us."
        }
        Some(Misconception::KeptOneDenominator) => {
            "They kept one bottom number: ask if both are cut into pieces of the same size."
        }
        Some(Misconception::Inverted) => {
            "They gave it upside down: ask which number counts the pieces in the whole."
        }
        Some(Misconception::MultipliedInstead) => {
            "They multiplied: ask how many times one fits in the other."
        }
        Some(Misconception::DividedInstead) => "They divided: ask what 'of' or 'times' means here.",
        Some(Misconception::WrongSign) => "They did the opposite operation: have them read it again aloud.",
        Some(Misconception::TransposedSign) => {
            "A term crossed the '=' without changing its sign: ask what undoes '+ 3' on the other side."
        }
        Some(Misconception::DividedBackwards) => {
            "They divided the wrong way round: ask what number times the coefficient gives the other side."
        }
        Some(Misconception::ForgotCrossTerms) => {
            "The middle products are missing: have them multiply each term of the first bracket by each of the second."
        }
        Some(Misconception::MissingSolution) => {
            "Only part of the solution: ask whether another value works too."
        }
        None => "Ask one short question that points at the first step.",
    }
}

/// Applies the rules to one answer. `past` is every other exercise of this
/// session on the same skill. What the tutor is told is kept short: it is
/// paid for again on every later turn.
pub fn judge(current: Current<'_>, past: &[Past], reading: &Reading) -> Outcome {
    let keep = |next, say: &str, stage| Outcome {
        status: Status::Open,
        attempts: current.attempts,
        hints: current.hints,
        first_try: None,
        directive: Directive { stage, next, say: say.to_string(), reveal: None },
    };
    let now = if current.probe { Stage::Diagnose } else { Stage::Practice };
    if reading.verdict == Verdict::Unreadable {
        return keep(
            Next::AskAgain,
            "No answer was understood. Ask them to say just their answer again.",
            now,
        );
    }
    if reading.verdict == Verdict::NotAnAnswer {
        return keep(
            Next::AskAgain,
            "That was not an answer to this exercise: the student stated another calculation or asked \
             something of their own. It was not counted. Respond to what they actually said first, without \
             handing out its answer, then bring them back to this exercise.",
            now,
        );
    }
    // "No sé" is a request for help, not a wrong answer: in a probe it settles
    // that they cannot do it yet; in practice it gets the next hint.
    let dont_know = reading.verdict == Verdict::DontKnow;
    // The value decides whether they can do it; the form is a detail to fix.
    let right_value = matches!(reading.verdict, Verdict::Correct | Verdict::WrongForm);
    let with = |status: Status| {
        let mut all = past.to_vec();
        all.push(Past { probe: current.probe, status });
        stage(&all)
    };

    if current.probe {
        let status = if right_value { Status::Correct } else { Status::Failed };
        let after = with(status);
        let (next, say) = match after {
            Stage::Diagnose => (
                Next::Probe,
                "Do not say if it was right. Say 'anotado' and ask one more of the same kind. No hints."
                    .to_string(),
            ),
            Stage::Done => (
                Next::MoveOn,
                "They can already do this. Say so warmly and move to the next skill, or something harder."
                    .to_string(),
            ),
            Stage::Practice => (
                Next::Teach,
                format!(
                    "Teach it now: one worked example with other numbers, have them explain it back, then an exercise. {}",
                    aimed_at(reading.misconception)
                ),
            ),
        };
        return Outcome {
            status,
            attempts: current.attempts + 1,
            hints: 0,
            first_try: Some(right_value),
            directive: Directive { stage: after, next, say, reveal: None },
        };
    }

    if reading.verdict == Verdict::WrongForm {
        let form = match current.form {
            Form::Mixed => "a whole number and a fraction",
            Form::Expanded => "a sum with like terms together, no brackets",
            Form::Factored => "a product whose factors cannot be split further",
            Form::Any | Form::Simplified => "the simplest form",
        };
        let say =
            format!("Right amount. Say so and ask them to write it as {form}. Do not write it for them.");
        return keep(Next::FixForm, &say, Stage::Practice);
    }

    let attempts = current.attempts + i32::from(!dont_know);
    if reading.verdict == Verdict::Correct {
        let after = with(Status::Correct);
        let solved = past
            .iter()
            .filter(|item| !item.probe)
            .rev()
            .take_while(|item| item.status == Status::Correct)
            .count()
            + 1;
        let (next, say) = if after == Stage::Done {
            (
                Next::MoveOn,
                "Correct. Say what they did well in one sentence, then the next skill or wrap up."
                    .to_string(),
            )
        } else {
            (
                Next::NextExercise,
                format!("Correct. Say so briefly, then another one as {}.", ANGLES[solved % ANGLES.len()]),
            )
        };
        return Outcome {
            status: Status::Correct,
            attempts,
            hints: current.hints,
            first_try: Some(attempts == 1 && current.hints == 0),
            directive: Directive { stage: after, next, say, reveal: None },
        };
    }

    // Wrong, or asking for help: a hint, a second one, a worked example, and
    // then it is worked out together.
    let hints = current.hints + 1;
    let open = |next, say: String| Outcome {
        status: Status::Open,
        attempts,
        hints,
        first_try: None,
        directive: Directive { stage: Stage::Practice, next, say, reveal: None },
    };
    if hints < HINTS {
        let lead = if dont_know { "They asked for help" } else { "Not right yet" };
        return open(
            Next::Hint,
            format!(
                "{lead}; do not give the answer. {} Same exercise again.",
                aimed_at(reading.misconception)
            ),
        );
    }
    if hints == HINTS {
        return open(
            Next::WorkedExample,
            "Work a similar one with other numbers, step by step. Then they retry theirs. Do not solve theirs."
                .to_string(),
        );
    }
    Outcome {
        status: Status::Failed,
        attempts,
        hints,
        first_try: Some(false),
        directive: Directive {
            stage: with(Status::Failed),
            next: Next::Teach,
            say: "They are stuck; be kind. Work it out together to the answer given, re-explain the idea, then an easier one."
                .to_string(),
            reveal: Some(current.answer.to_string()),
        },
    }
}

// Storage and the agent's calls -------------------------------------------------

/// An exercise the platform can check: arithmetic, or the algebra of secondary school.
#[derive(Debug, PartialEq)]
enum Posed {
    Arithmetic(Exercise),
    Algebra(Problem),
}

impl Posed {
    fn parse(expression: &str) -> Option<Self> {
        Exercise::parse(expression)
            .map(Self::Arithmetic)
            .or_else(|| Problem::parse(expression).map(Self::Algebra))
    }

    fn answer(&self) -> String {
        match self {
            Self::Arithmetic(exercise) => exercise.answer.to_string(),
            Self::Algebra(problem) => problem.answer(),
        }
    }

    /// How the answer must be written. Algebra says it by itself: an
    /// expanded polynomial is to be factored, anything else to be expanded.
    fn form(&self, asked: Form) -> Form {
        match self {
            Self::Arithmetic(_) => asked,
            Self::Algebra(problem) => match problem.shape() {
                Shape::Expanded => Form::Expanded,
                Shape::Factored => Form::Factored,
                Shape::Any => Form::Any,
            },
        }
    }

    fn check(&self, form: Form, said: &str) -> Reading {
        match self {
            Self::Arithmetic(exercise) => check(exercise, form, said),
            Self::Algebra(problem) => problem.check(said),
        }
    }

    fn is_given_away_by(&self, expression: &str, said: &str) -> bool {
        match self {
            Self::Arithmetic(exercise) => exercise.is_given_away_by(expression, said),
            Self::Algebra(problem) => problem.is_given_away_by(said),
        }
    }
}

fn form_of(name: &str) -> Form {
    match name {
        "simplified" => Form::Simplified,
        "mixed" => Form::Mixed,
        "expanded" => Form::Expanded,
        "factored" => Form::Factored,
        _ => Form::Any,
    }
}

async fn past(
    db: &DatabaseConnection,
    session_id: Uuid,
    skill_id: Option<&str>,
    except: i64,
) -> Result<Vec<Past>, DbErr> {
    let same_skill = match skill_id {
        Some(id) => exercise_row::Column::SkillId.eq(id),
        None => exercise_row::Column::SkillId.is_null(),
    };
    let rows: Vec<(bool, String)> = exercise_row::Entity::find()
        .filter(exercise_row::Column::SessionId.eq(session_id))
        .filter(same_skill)
        .filter(exercise_row::Column::Id.ne(except))
        .order_by_asc(exercise_row::Column::Id)
        .select_only()
        .column(exercise_row::Column::Probe)
        .column(exercise_row::Column::Status)
        .into_tuple()
        .all(db)
        .await?;
    Ok(rows.into_iter().map(|(probe, status)| Past { probe, status: Status::stored(&status) }).collect())
}

/// The session's open exercise, the newest if there were ever two.
async fn open(db: &DatabaseConnection, session_id: Uuid) -> Result<Option<exercise_row::Model>, DbErr> {
    exercise_row::Entity::find()
        .filter(exercise_row::Column::SessionId.eq(session_id))
        .filter(exercise_row::Column::Status.eq(Status::Open.as_str()))
        .order_by_desc(exercise_row::Column::Id)
        .one(db)
        .await
}

/// Whether a skill is in the curriculum.
async fn known_skill(db: &DatabaseConnection, id: &str) -> Result<Option<String>, DbErr> {
    skill::Entity::find_by_id(id.to_string())
        .select_only()
        .column(skill::Column::Id)
        .into_tuple()
        .one(db)
        .await
}

fn running(session: &Session) -> ApiResult<()> {
    if session.status == "ended" {
        return Err(ApiError::Conflict("the session has ended"));
    }
    // The engine poses and checks its own exercises.
    crate::engine::free(session)
}

#[derive(Deserialize)]
pub struct ExerciseInput {
    /// The arithmetic, as the tutor would write it: "3/4 + 1/8".
    expression: String,
    /// The skill from the plan this exercise is for, if it is for one.
    #[serde(default)]
    skill_id: Option<String>,
    #[serde(default)]
    form: Form,
}

/// The tutor states the exercise it is about to ask. The answer is worked out
/// and kept here.
pub async fn pose(
    Agent(session): Agent,
    State(state): State<AppState>,
    Input(body): Input<ExerciseInput>,
) -> ApiResult<(StatusCode, Json<Value>)> {
    running(&session)?;
    let expression = body.expression.trim();
    if expression.is_empty() || expression.chars().count() > 120 {
        return Err(ApiError::bad_request("expression must have 1 to 120 characters"));
    }
    let Some(exercise) = Posed::parse(expression) else {
        // Counted, never quoted: how often the tutor asks for something this cannot check.
        tracing::info!(session_id = %session.id, "exercise not checked: not arithmetic or algebra it can work out");
        session::Entity::update_many()
            .col_expr(
                session::Column::UncheckedExercises,
                Expr::col(session::Column::UncheckedExercises).add(1),
            )
            .filter(session::Column::Id.eq(session.id))
            .exec(&state.db)
            .await?;
        return Err(ApiError::bad_request(
            "expression is not arithmetic or algebra that can be checked; carry on without checking",
        ));
    };
    // Only a skill of the curriculum counts. An id the tutor made up is
    // treated as none, so it does not start a track of its own.
    let skill_id = match body.skill_id.as_deref().map(str::trim).filter(|id| learning::is_slug(id)) {
        Some(id) => known_skill(&state.db, id).await?,
        None => None,
    };
    let skill_id = skill_id.as_deref();

    // Stating the open exercise again (a retry, or "¿me repites?") changes
    // nothing: its tries and hints stay.
    if let Some(exercise_row::Model { id, expression: open_expression, probe, .. }) =
        open(&state.db, session.id).await?
        && Posed::parse(&open_expression).is_some_and(|open| open == exercise)
    {
        let stage = if probe { Stage::Diagnose } else { Stage::Practice };
        let say = "Same exercise as before: carry on with it.";
        return Ok((
            StatusCode::OK,
            Json(json!({ "exercise": id, "stage": stage, "probe": probe, "say": say })),
        ));
    }
    // One exercise at a time: asking a new one leaves the last one behind.
    exercise_row::Entity::update_many()
        .col_expr(exercise_row::Column::Status, Expr::value(Status::Dropped.as_str()))
        .col_expr(exercise_row::Column::ResolvedAt, Expr::current_timestamp())
        .filter(exercise_row::Column::SessionId.eq(session.id))
        .filter(exercise_row::Column::Status.eq(Status::Open.as_str()))
        .exec(&state.db)
        .await?;
    let stage = stage(&past(&state.db, session.id, skill_id, 0).await?);
    let probe = stage == Stage::Diagnose;
    let form = exercise.form(if probe { Form::Any } else { body.form });
    let id = exercise_row::Entity::insert(exercise_row::ActiveModel {
        session_id: Set(session.id),
        skill_id: Set(skill_id.map(str::to_string)),
        expression: Set(expression.to_string()),
        answer: Set(exercise.answer()),
        form: Set(serde_json::to_value(form)
            .ok()
            .and_then(|value| value.as_str().map(str::to_string))
            .unwrap_or_default()),
        probe: Set(probe),
        ..Default::default()
    })
    .exec(&state.db)
    .await?
    .last_insert_id;
    let say = match stage {
        Stage::Diagnose => {
            "To see what they can do: ask it plainly, no hints, and do not say if the answer is right."
        }
        Stage::Practice => "Ask it and wait. Report their answer; never give the answer yourself.",
        Stage::Done => "Enough of this skill today; prefer the next skill in the plan.",
    };
    Ok((StatusCode::CREATED, Json(json!({ "exercise": id, "stage": stage, "probe": probe, "say": say }))))
}

#[derive(Deserialize)]
pub struct AnswerInput {
    /// What the student said or wrote, in their words.
    said: String,
}

/// The tutor reports the student's answer to the open exercise and is told
/// whether it is right and what to do next.
pub async fn answer(
    Agent(session): Agent,
    State(state): State<AppState>,
    Input(body): Input<AnswerInput>,
) -> ApiResult<Json<Value>> {
    running(&session)?;
    if body.said.chars().count() > 500 {
        return Err(ApiError::bad_request("said must have at most 500 characters"));
    }
    let row = open(&state.db, session.id)
        .await?
        .ok_or(ApiError::Conflict("no exercise is open; state one first"))?;
    let exercise =
        Posed::parse(&row.expression).ok_or(ApiError::Conflict("the open exercise cannot be checked"))?;
    let form = form_of(&row.form);
    let mut reading = exercise.check(form, &body.said);
    // A wrong number is a try only when it was said about this exercise.
    if reading.verdict == Verdict::Incorrect
        && matches!(exercise, Posed::Arithmetic(_))
        && is_about_something_else(&row.expression, &body.said)
    {
        reading = Reading { read: None, verdict: Verdict::NotAnAnswer, misconception: None };
    }
    let history = past(&state.db, session.id, row.skill_id.as_deref(), row.id).await?;
    let current =
        Current { probe: row.probe, attempts: row.attempts, hints: row.hints, answer: &row.answer, form };
    let outcome = judge(current, &history, &reading);

    let closed = outcome.status != Status::Open;
    let named = reading
        .misconception
        .and_then(|name| serde_json::to_value(name).ok())
        .and_then(|v| v.as_str().map(str::to_string));
    let mut update = exercise_row::Entity::update_many()
        .col_expr(exercise_row::Column::Status, Expr::value(outcome.status.as_str()))
        .col_expr(exercise_row::Column::Attempts, Expr::value(outcome.attempts))
        .col_expr(exercise_row::Column::Hints, Expr::value(outcome.hints))
        .col_expr(exercise_row::Column::FirstTry, Expr::value(outcome.first_try))
        .col_expr(
            exercise_row::Column::ResolvedAt,
            if closed { Expr::current_timestamp() } else { Expr::value(None::<chrono::DateTime<Utc>>) },
        )
        .filter(exercise_row::Column::Id.eq(row.id));
    // A misconception once seen stays, even when a later answer shows none.
    if let Some(named) = named {
        update = update.col_expr(exercise_row::Column::Misconception, Expr::value(named));
    }
    update.exec(&state.db).await?;

    // A closed exercise on a known skill is evidence for the learner model,
    // from the check itself and not from anyone's reading of the conversation.
    if let (true, Some(child_id), Some(skill_id)) = (closed, session.child_id, row.skill_id.as_deref())
        && known_skill(&state.db, skill_id).await?.is_some()
    {
        let solved = outcome.status == Status::Correct;
        let first_try = outcome.first_try == Some(true);
        let attempt = NewAttempt {
            skill_id: skill_id.to_string(),
            question_id: None,
            correct: solved && first_try,
            credit: Some(if !solved {
                0.0
            } else if first_try {
                1.0
            } else {
                CREDIT_WITH_HINTS
            }),
            choice: false,
            at: None,
        };
        learning::record(&state.db, child_id, Some(session.id), "tutor", &[attempt], Utc::now()).await?;
    }

    Ok(Json(json!({
        "verdict": reading.verdict,
        "read": reading.read,
        "misconception": reading.misconception,
        "attempts": outcome.attempts,
        "hints": outcome.hints,
        "closed": closed,
        "stage": outcome.directive.stage,
        "next": outcome.directive.next,
        "say": outcome.directive.say,
        "reveal": outcome.directive.reveal,
    })))
}

/// The exercise being worked on now, as the supervisor may know it: where it
/// stands, never its answer.
/// Marks the open exercise when something the tutor just said contains its
/// answer. Only the fact is kept. An answer said in the same moment the
/// exercise closes is missed; this undercounts rather than accuse.
pub async fn note_answer_said(
    db: &DatabaseConnection,
    session_id: Uuid,
    tutor_said: &[String],
) -> Result<(), DbErr> {
    if tutor_said.is_empty() {
        return Ok(());
    }
    let Some(row) = open(db, session_id).await? else { return Ok(()) };
    if row.answer_said {
        return Ok(());
    }
    let Some(exercise) = Posed::parse(&row.expression) else { return Ok(()) };
    if tutor_said.iter().any(|said| exercise.is_given_away_by(&row.expression, said)) {
        exercise_row::Entity::update_many()
            .col_expr(exercise_row::Column::AnswerSaid, Expr::value(true))
            .filter(exercise_row::Column::Id.eq(row.id))
            .exec(db)
            .await?;
    }
    Ok(())
}

pub async fn open_exercise(db: &DatabaseConnection, session_id: Uuid) -> Result<Option<Value>, DbErr> {
    Ok(open(db, session_id).await?.map(|row| {
        json!({
            "exercise": row.expression,
            "step": if row.probe { "finding out what the student can do: no hints, no right or wrong" } else { "practice with hints" },
            "tries": row.attempts,
            "hints_given": row.hints,
        })
    }))
}

/// The session's exercises with their answers, for the team's view. Never for
/// the student's screen.
pub async fn exercises(db: &DatabaseConnection, session_id: Uuid) -> Result<Vec<Value>, DbErr> {
    let rows = exercise_row::Entity::find()
        .filter(exercise_row::Column::SessionId.eq(session_id))
        .order_by_asc(exercise_row::Column::Id)
        .all(db)
        .await?;
    Ok(rows
        .into_iter()
        .map(|row| {
            json!({
                "skill_id": row.skill_id, "expression": row.expression, "answer": row.answer, "form": row.form,
                "probe": row.probe, "attempts": row.attempts, "hints": row.hints,
                "status": Status::stored(&row.status), "misconception": row.misconception,
            })
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    const PROBE_RIGHT: Past = Past { probe: true, status: Status::Correct };
    const PROBE_WRONG: Past = Past { probe: true, status: Status::Failed };
    const SOLVED: Past = Past { probe: false, status: Status::Correct };
    const STUCK: Past = Past { probe: false, status: Status::Failed };

    fn reading(verdict: Verdict, misconception: Option<Misconception>) -> Reading {
        Reading { read: Some("x".into()), verdict, misconception }
    }

    fn practice(attempts: i32, hints: i32) -> Current<'static> {
        Current { probe: false, attempts, hints, answer: "7/8", form: Form::Any }
    }

    const PROBE: Current<'static> =
        Current { probe: true, attempts: 0, hints: 0, answer: "7/8", form: Form::Any };

    #[test]
    fn a_skill_is_diagnosed_before_anything_else() {
        assert_eq!(stage(&[]), Stage::Diagnose);
        assert_eq!(stage(&[PROBE_RIGHT]), Stage::Diagnose);
        // An exercise left behind or still open is not an answer.
        assert_eq!(stage(&[PROBE_RIGHT, Past { probe: true, status: Status::Dropped }]), Stage::Diagnose);
        assert_eq!(stage(&[PROBE_RIGHT, Past { probe: true, status: Status::Open }]), Stage::Diagnose);
        // Both right: there is nothing to teach.
        assert_eq!(stage(&[PROBE_RIGHT, PROBE_RIGHT]), Stage::Done);
        // One wrong: practice, until enough are solved in a row since the last time they were stuck.
        assert_eq!(stage(&[PROBE_RIGHT, PROBE_WRONG]), Stage::Practice);
        assert_eq!(stage(&[PROBE_WRONG, PROBE_WRONG, SOLVED, SOLVED]), Stage::Practice);
        assert_eq!(stage(&[PROBE_WRONG, PROBE_WRONG, SOLVED, SOLVED, SOLVED]), Stage::Done);
        assert_eq!(
            stage(&[PROBE_WRONG, PROBE_WRONG, SOLVED, SOLVED, STUCK, SOLVED, SOLVED]),
            Stage::Practice
        );
        assert_eq!(stage(&[PROBE_WRONG, PROBE_WRONG, SOLVED, STUCK, SOLVED, SOLVED, SOLVED]), Stage::Done);
    }

    #[test]
    fn a_probe_gets_no_hint_and_no_right_or_wrong() {
        let first = judge(PROBE, &[], &reading(Verdict::Incorrect, Some(Misconception::AddedAcross)));
        assert_eq!((first.status, first.first_try, first.hints), (Status::Failed, Some(false), 0));
        assert_eq!((first.directive.stage, first.directive.next), (Stage::Diagnose, Next::Probe));
        assert!(first.directive.say.contains("Do not say if it was right"));
        assert_eq!(first.directive.reveal, None);

        // The second one settles it: wrong before, so it is taught, aimed at the mistake.
        let second =
            judge(PROBE, &[PROBE_WRONG], &reading(Verdict::Incorrect, Some(Misconception::AddedAcross)));
        assert_eq!((second.directive.stage, second.directive.next), (Stage::Practice, Next::Teach));
        assert!(second.directive.say.contains("tops and bottoms"));
        let known = judge(PROBE, &[PROBE_RIGHT], &reading(Verdict::Correct, None));
        assert_eq!(
            (known.status, known.directive.stage, known.directive.next),
            (Status::Correct, Stage::Done, Next::MoveOn)
        );

        // The right amount in another form shows they can do it.
        let form = judge(PROBE, &[PROBE_RIGHT], &reading(Verdict::WrongForm, None));
        assert_eq!((form.status, form.directive.next), (Status::Correct, Next::MoveOn));
        // What nobody could read is asked again and does not count.
        let unread = judge(PROBE, &[], &reading(Verdict::Unreadable, None));
        assert_eq!(
            (unread.status, unread.attempts, unread.directive.next),
            (Status::Open, 0, Next::AskAgain)
        );
    }

    #[test]
    fn a_wrong_answer_in_practice_gets_hints_then_an_example_then_help() {
        let taught = [PROBE_WRONG, PROBE_WRONG];
        let wrong = reading(Verdict::Incorrect, Some(Misconception::KeptOneDenominator));
        let mut current = practice(0, 0);
        let mut steps = Vec::new();
        for _ in 0..4 {
            let outcome = judge(current, &taught, &wrong);
            steps.push((outcome.status, outcome.directive.next, outcome.directive.reveal.clone()));
            current = Current { attempts: outcome.attempts, hints: outcome.hints, ..current };
        }
        assert_eq!(
            steps,
            [
                (Status::Open, Next::Hint, None),
                (Status::Open, Next::Hint, None),
                (Status::Open, Next::WorkedExample, None),
                // Only now, with the exercise closed, does the tutor get the answer.
                (Status::Failed, Next::Teach, Some("7/8".to_string())),
            ]
        );
        let hint = judge(practice(0, 0), &taught, &wrong);
        assert!(hint.directive.say.contains("kept one bottom number"));
        assert!(hint.directive.say.contains("do not give the answer"));

        // "No sé" gets the next hint without a wrong mark, and in a probe settles it.
        let asked = judge(practice(1, 1), &taught, &reading(Verdict::DontKnow, None));
        assert_eq!(
            (asked.status, asked.attempts, asked.hints, asked.directive.next),
            (Status::Open, 1, 2, Next::Hint)
        );
        assert!(asked.directive.say.starts_with("They asked for help"));
        let probe = judge(PROBE, &[PROBE_RIGHT], &reading(Verdict::DontKnow, None));
        assert_eq!((probe.status, probe.directive.next), (Status::Failed, Next::Teach));

        // Every instruction is short: it is paid for on each later turn.
        for outcome in [&hint, &asked, &probe] {
            assert!(outcome.directive.say.split_whitespace().count() <= 30, "{}", outcome.directive.say);
        }
    }

    #[test]
    fn solving_moves_to_another_angle_and_then_on() {
        let taught = [PROBE_WRONG, PROBE_WRONG];
        let right = reading(Verdict::Correct, None);
        let first = judge(practice(0, 0), &taught, &right);
        assert_eq!(
            (first.status, first.first_try, first.directive.next),
            (Status::Correct, Some(true), Next::NextExercise)
        );
        assert!(first.directive.say.contains("word problem"));
        // Solved after a hint is solved, but not at the first try.
        let helped = judge(practice(1, 1), &[PROBE_WRONG, PROBE_WRONG, SOLVED], &right);
        assert_eq!(
            (helped.first_try, helped.attempts, helped.directive.next),
            (Some(false), 2, Next::NextExercise)
        );
        assert!(helped.directive.say.contains("one mistake in it"));
        let last = judge(practice(0, 0), &[PROBE_WRONG, PROBE_WRONG, SOLVED, SOLVED], &right);
        assert_eq!((last.directive.stage, last.directive.next), (Stage::Done, Next::MoveOn));
    }

    #[test]
    fn the_right_amount_in_another_form_costs_nothing() {
        let current = Current { form: Form::Simplified, ..practice(1, 1) };
        let outcome = judge(current, &[PROBE_WRONG, PROBE_WRONG], &reading(Verdict::WrongForm, None));
        assert_eq!((outcome.status, outcome.attempts, outcome.hints), (Status::Open, 1, 1));
        assert_eq!(outcome.directive.next, Next::FixForm);
        assert!(outcome.directive.say.contains("simplest form"));
    }
}
