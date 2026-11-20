//! How a session moves through an exercise: find out first, teach if needed,
//! practise until it holds.
//!
//! The rules come from the tutoring demo that preceded this platform, where a
//! rule engine and not a language model decided the flow. They are adapted to
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
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sqlx::PgPool;
use uuid::Uuid;

use crate::auth::{Agent, Input, Session};
use crate::error::{ApiError, ApiResult};
use crate::exercise::{Exercise, Form, Misconception, Reading, Verdict, check};
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

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, sqlx::Type)]
#[serde(rename_all = "snake_case")]
#[sqlx(type_name = "text", rename_all = "snake_case")]
pub enum Status {
    Open,
    Correct,
    Failed,
    Dropped,
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
            "No number was understood. Ask them to say just their answer again.",
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
        let form =
            if current.form == Form::Mixed { "a whole number and a fraction" } else { "the simplest form" };
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

#[derive(sqlx::FromRow)]
struct Row {
    id: i64,
    skill_id: Option<String>,
    expression: String,
    answer: String,
    form: String,
    probe: bool,
    attempts: i32,
    hints: i32,
}

fn form_of(name: &str) -> Form {
    match name {
        "simplified" => Form::Simplified,
        "mixed" => Form::Mixed,
        _ => Form::Any,
    }
}

async fn past(
    pool: &PgPool,
    session_id: Uuid,
    skill_id: Option<&str>,
    except: i64,
) -> Result<Vec<Past>, sqlx::Error> {
    let rows: Vec<(bool, Status)> = sqlx::query_as(
        "select probe, status from session_exercises
         where session_id = $1 and skill_id is not distinct from $2 and id <> $3 order by id",
    )
    .bind(session_id)
    .bind(skill_id)
    .bind(except)
    .fetch_all(pool)
    .await?;
    Ok(rows.into_iter().map(|(probe, status)| Past { probe, status }).collect())
}

fn running(session: &Session) -> ApiResult<()> {
    if session.status == "ended" { Err(ApiError::Conflict("the session has ended")) } else { Ok(()) }
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
    let Some(exercise) = Exercise::parse(expression) else {
        // Counted, never quoted: how often the tutor asks for something this cannot check.
        tracing::info!(session_id = %session.id, "exercise not checked: not plain arithmetic");
        sqlx::query("update sessions set unchecked_exercises = unchecked_exercises + 1 where id = $1")
            .bind(session.id)
            .execute(&state.pool)
            .await?;
        return Err(ApiError::bad_request(
            "expression is not arithmetic that can be checked; carry on without checking",
        ));
    };
    // Only a skill of the curriculum counts. An id the tutor made up is
    // treated as none, so it does not start a track of its own.
    let skill_id = match body.skill_id.as_deref().map(str::trim).filter(|id| learning::is_slug(id)) {
        Some(id) => {
            sqlx::query_scalar::<_, String>("select id from skills where id = $1")
                .bind(id)
                .fetch_optional(&state.pool)
                .await?
        }
        None => None,
    };
    let skill_id = skill_id.as_deref();

    // Stating the open exercise again (a retry, or "¿me repites?") changes
    // nothing: its tries and hints stay.
    let open: Option<(i64, String, bool)> = sqlx::query_as(
        "select id, expression, probe from session_exercises
         where session_id = $1 and status = 'open' order by id desc limit 1",
    )
    .bind(session.id)
    .fetch_optional(&state.pool)
    .await?;
    if let Some((id, open_expression, probe)) = open
        && Exercise::parse(&open_expression).is_some_and(|open| open == exercise)
    {
        let stage = if probe { Stage::Diagnose } else { Stage::Practice };
        let say = "Same exercise as before: carry on with it.";
        return Ok((
            StatusCode::OK,
            Json(json!({ "exercise": id, "stage": stage, "probe": probe, "say": say })),
        ));
    }
    // One exercise at a time: asking a new one leaves the last one behind.
    sqlx::query("update session_exercises set status = 'dropped', resolved_at = now() where session_id = $1 and status = 'open'")
        .bind(session.id)
        .execute(&state.pool)
        .await?;
    let stage = stage(&past(&state.pool, session.id, skill_id, 0).await?);
    let probe = stage == Stage::Diagnose;
    let form = if probe { Form::Any } else { body.form };
    let id: i64 = sqlx::query_scalar(
        "insert into session_exercises (session_id, skill_id, expression, answer, form, probe)
         values ($1, $2, $3, $4, $5, $6) returning id",
    )
    .bind(session.id)
    .bind(skill_id)
    .bind(expression)
    .bind(exercise.answer.to_string())
    .bind(
        serde_json::to_value(form)
            .ok()
            .and_then(|value| value.as_str().map(str::to_string))
            .unwrap_or_default(),
    )
    .bind(probe)
    .fetch_one(&state.pool)
    .await?;
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
    let row: Option<Row> = sqlx::query_as(
        "select id, skill_id, expression, answer, form, probe, attempts, hints
         from session_exercises where session_id = $1 and status = 'open' order by id desc limit 1",
    )
    .bind(session.id)
    .fetch_optional(&state.pool)
    .await?;
    let row = row.ok_or(ApiError::Conflict("no exercise is open; state one first"))?;
    let exercise =
        Exercise::parse(&row.expression).ok_or(ApiError::Conflict("the open exercise cannot be checked"))?;
    let form = form_of(&row.form);
    let reading = check(&exercise, form, &body.said);
    let history = past(&state.pool, session.id, row.skill_id.as_deref(), row.id).await?;
    let current =
        Current { probe: row.probe, attempts: row.attempts, hints: row.hints, answer: &row.answer, form };
    let outcome = judge(current, &history, &reading);

    let closed = outcome.status != Status::Open;
    let named = reading
        .misconception
        .and_then(|name| serde_json::to_value(name).ok())
        .and_then(|v| v.as_str().map(str::to_string));
    sqlx::query(
        "update session_exercises set status = $2, attempts = $3, hints = $4, first_try = $5,
           misconception = coalesce($6, misconception), resolved_at = case when $7 then now() else null end
         where id = $1",
    )
    .bind(row.id)
    .bind(outcome.status)
    .bind(outcome.attempts)
    .bind(outcome.hints)
    .bind(outcome.first_try)
    .bind(named)
    .bind(closed)
    .execute(&state.pool)
    .await?;

    // A closed exercise on a known skill is evidence for the learner model,
    // from the check itself and not from anyone's reading of the conversation.
    if let (true, Some(child_id), Some(skill_id)) = (closed, session.child_id, row.skill_id.as_deref()) {
        let known: Option<String> = sqlx::query_scalar("select id from skills where id = $1")
            .bind(skill_id)
            .fetch_optional(&state.pool)
            .await?;
        if known.is_some() {
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
            learning::record(&state.pool, child_id, Some(session.id), "tutor", &[attempt], Utc::now())
                .await?;
        }
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
pub async fn open_exercise(pool: &PgPool, session_id: Uuid) -> Result<Option<Value>, sqlx::Error> {
    let open: Option<(String, bool, i32, i32)> = sqlx::query_as(
        "select expression, probe, attempts, hints from session_exercises
         where session_id = $1 and status = 'open' order by id desc limit 1",
    )
    .bind(session_id)
    .fetch_optional(pool)
    .await?;
    Ok(open.map(|(expression, probe, attempts, hints)| {
        json!({
            "exercise": expression,
            "step": if probe { "finding out what the student can do: no hints, no right or wrong" } else { "practice with hints" },
            "tries": attempts,
            "hints_given": hints,
        })
    }))
}

/// The session's exercises with their answers, for the team's view. Never for
/// the student's screen.
pub async fn exercises(pool: &PgPool, session_id: Uuid) -> Result<Vec<Value>, sqlx::Error> {
    type Line = (Option<String>, String, String, String, bool, i32, i32, Status, Option<String>);
    let rows: Vec<Line> = sqlx::query_as(
        "select skill_id, expression, answer, form, probe, attempts, hints, status, misconception
         from session_exercises where session_id = $1 order by id",
    )
    .bind(session_id)
    .fetch_all(pool)
    .await?;
    Ok(rows
        .into_iter()
        .map(|(skill_id, expression, answer, form, probe, attempts, hints, status, misconception)| {
            json!({
                "skill_id": skill_id, "expression": expression, "answer": answer, "form": form,
                "probe": probe, "attempts": attempts, "hints": hints, "status": status,
                "misconception": misconception,
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
