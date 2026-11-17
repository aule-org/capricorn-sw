//! What each child knows. The model itself is [`skill_model`], a pure
//! function of the child's history; this module stores that history, keeps
//! the difficulty of questions calibrated, and picks what to work on next.

pub mod skill_model;

use std::collections::{BTreeMap, BTreeSet};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::{PgPool, Postgres, Transaction};
use uuid::Uuid;

use crate::error::{ApiError, ApiResult};
use skill_model::{Correction, LearnerModel, Params, SkillTrace, TraceAttempt, TraceRequest, TraceResponse};

/// Skill and question ids are slugs, so one coming from a language model can
/// be checked before it is stored.
pub fn is_slug(id: &str) -> bool {
    let mut chars = id.chars();
    id.len() <= 80
        && chars.next().is_some_and(|first| first.is_ascii_lowercase() || first.is_ascii_digit())
        && chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, '.' | '_' | '-'))
}

#[derive(Clone, Debug, Deserialize)]
pub struct NewAttempt {
    pub skill_id: String,
    #[serde(default)]
    pub question_id: Option<String>,
    pub correct: bool,
    /// Partial credit from 0 to 1. Defaults to 1 or 0 from `correct`.
    #[serde(default)]
    pub credit: Option<f64>,
    /// A multiple-choice question, which can be answered right by chance.
    #[serde(default)]
    pub choice: bool,
    #[serde(default)]
    pub at: Option<DateTime<Utc>>,
}

impl NewAttempt {
    pub fn validate(&self) -> Result<(), String> {
        if !is_slug(&self.skill_id) {
            return Err(format!("skill_id {:?} is not a valid id", self.skill_id));
        }
        if self.question_id.as_deref().is_some_and(|id| !is_slug(id)) {
            return Err("question_id is not a valid id".into());
        }
        if self.credit.is_some_and(|credit| !(0.0..=1.0).contains(&credit)) {
            return Err("credit must be a number between 0 and 1".into());
        }
        Ok(())
    }
}

#[derive(sqlx::FromRow)]
struct AttemptRow {
    skill_id: String,
    question_id: Option<String>,
    correct: bool,
    credit: Option<f64>,
    choice: bool,
    difficulty: Option<f64>,
    at: DateTime<Utc>,
}

#[derive(sqlx::FromRow)]
struct CorrectionRow {
    skill_id: String,
    level: f64,
    at: DateTime<Utc>,
}

async fn history(
    tx: &mut Transaction<'_, Postgres>,
    child_id: Uuid,
) -> Result<(Vec<TraceAttempt>, Vec<Correction>), sqlx::Error> {
    let attempts: Vec<AttemptRow> = sqlx::query_as(
        "select skill_id, question_id, correct, credit, choice, difficulty, at
         from attempts where child_id = $1 order by at, id limit $2",
    )
    .bind(child_id)
    .bind(skill_model::MAX_ATTEMPTS as i64)
    .fetch_all(&mut **tx)
    .await?;
    let corrections: Vec<CorrectionRow> = sqlx::query_as(
        "select skill_id, level, at from corrections where child_id = $1 order by at, id limit $2",
    )
    .bind(child_id)
    .bind(skill_model::MAX_CORRECTIONS as i64)
    .fetch_all(&mut **tx)
    .await?;
    Ok((
        attempts
            .into_iter()
            .map(|row| TraceAttempt {
                skill_id: row.skill_id,
                question_id: row.question_id,
                correct: row.correct,
                credit: row.credit,
                at: row.at,
                choice: row.choice,
                difficulty: row.difficulty,
            })
            .collect(),
        corrections
            .into_iter()
            .map(|row| Correction { skill_id: row.skill_id, at: row.at, level: row.level })
            .collect(),
    ))
}

/// The model after the child's whole history, in the same order
/// [`skill_model::trace`] replays it: by time, attempts before corrections.
pub fn replay(attempts: &[TraceAttempt], corrections: &[Correction], params: Params) -> LearnerModel {
    enum Event<'a> {
        Attempt(&'a TraceAttempt),
        Correction(&'a Correction),
    }
    let mut events: Vec<(DateTime<Utc>, u8, Event)> = attempts
        .iter()
        .map(|attempt| (attempt.at, 0, Event::Attempt(attempt)))
        .chain(corrections.iter().map(|correction| (correction.at, 1, Event::Correction(correction))))
        .collect();
    events.sort_by_key(|(at, kind, _)| (*at, *kind));

    let mut model = LearnerModel::new(params);
    for (at, _, event) in events {
        match event {
            Event::Attempt(attempt) => {
                let credit = attempt.credit.unwrap_or(if attempt.correct { 1.0 } else { 0.0 });
                model.observe(
                    &attempt.skill_id,
                    attempt.difficulty.unwrap_or(0.0),
                    attempt.choice,
                    credit,
                    at,
                );
            }
            Event::Correction(correction) => model.correct(&correction.skill_id, correction.level, at),
        }
    }
    model
}

/// What the child knows now.
pub async fn trace_child(pool: &PgPool, child_id: Uuid, now: DateTime<Utc>) -> ApiResult<TraceResponse> {
    let mut tx = pool.begin().await?;
    let (attempts, corrections) = history(&mut tx, child_id).await?;
    tx.commit().await?;
    skill_model::trace(&TraceRequest { now: Some(now), attempts, corrections, params: Params::default() })
        .map_err(ApiError::Internal)
}

/// Reads the calibrated difficulty of a skill or question, locking its row so
/// two answers to the same question update it one after the other.
async fn locked_difficulty(
    tx: &mut Transaction<'_, Postgres>,
    skill_id: &str,
    question_id: &str,
) -> Result<(f64, i32), sqlx::Error> {
    sqlx::query("insert into item_difficulty (skill_id, question_id) values ($1, $2) on conflict do nothing")
        .bind(skill_id)
        .bind(question_id)
        .execute(&mut **tx)
        .await?;
    sqlx::query_as(
        "select value, answers from item_difficulty where skill_id = $1 and question_id = $2 for update",
    )
    .bind(skill_id)
    .bind(question_id)
    .fetch_one(&mut **tx)
    .await
}

/// Records answers. Each one moves the child's model and, with the same
/// signal, the difficulty of the question and of its skill: an answer most
/// children get wrong becomes a harder question for everyone.
pub async fn record(
    pool: &PgPool,
    child_id: Uuid,
    session_id: Option<Uuid>,
    source: &str,
    attempts: &[NewAttempt],
    now: DateTime<Utc>,
) -> ApiResult<usize> {
    for attempt in attempts {
        attempt.validate().map_err(ApiError::BadRequest)?;
    }
    let mut ordered: Vec<(DateTime<Utc>, &NewAttempt)> =
        attempts.iter().map(|attempt| (attempt.at.unwrap_or(now).min(now), attempt)).collect();
    ordered.sort_by_key(|(at, _)| *at);

    let params = Params::default();
    let mut tx = pool.begin().await?;
    // One writer per child at a time, so the replay below is the real history.
    sqlx::query("select pg_advisory_xact_lock(hashtextextended($1::text, 0))")
        .bind(child_id)
        .execute(&mut *tx)
        .await?;
    let (past, corrections) = history(&mut tx, child_id).await?;
    let mut model = replay(&past, &corrections, params);

    for (at, attempt) in &ordered {
        let (skill_value, skill_answers) = locked_difficulty(&mut tx, &attempt.skill_id, "").await?;
        let question = match attempt.question_id.as_deref() {
            Some(question_id) => {
                Some((question_id, locked_difficulty(&mut tx, &attempt.skill_id, question_id).await?))
            }
            None => None,
        };
        let difficulty = skill_value + question.map_or(0.0, |(_, (value, _))| value);
        let credit = attempt.credit.unwrap_or(if attempt.correct { 1.0 } else { 0.0 });
        let signal = model.observe(&attempt.skill_id, difficulty, attempt.choice, credit, *at);

        sqlx::query(
            "insert into attempts
               (id, child_id, session_id, skill_id, question_id, correct, credit, choice, difficulty, source, at)
             values ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11)",
        )
        .bind(Uuid::now_v7())
        .bind(child_id)
        .bind(session_id)
        .bind(&attempt.skill_id)
        .bind(&attempt.question_id)
        .bind(attempt.correct)
        .bind(attempt.credit)
        .bind(attempt.choice)
        .bind(difficulty)
        .bind(source)
        .bind(at)
        .execute(&mut *tx)
        .await?;

        let mut updates = vec![("", skill_value, skill_answers)];
        if let Some((question_id, (value, answers))) = question {
            updates.push((question_id, value, answers));
        }
        for (question_id, value, answers) in updates {
            sqlx::query(
                "update item_difficulty set value = $3, answers = answers + 1
                 where skill_id = $1 and question_id = $2",
            )
            .bind(&attempt.skill_id)
            .bind(question_id)
            .bind(params.item_update(value, answers.max(0) as u32, signal))
            .execute(&mut *tx)
            .await?;
        }
    }
    tx.commit().await?;
    Ok(ordered.len())
}

#[derive(Clone, Debug, Serialize, Deserialize, sqlx::FromRow)]
pub struct Skill {
    pub id: String,
    pub subject: String,
    pub name: String,
    #[serde(default)]
    pub grade: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub prerequisites: Vec<String>,
}

pub async fn catalog(pool: &PgPool) -> Result<Vec<Skill>, sqlx::Error> {
    sqlx::query_as("select id, subject, name, grade, description, prerequisites from skills order by id")
        .fetch_all(pool)
        .await
}

pub async fn upsert_skills(pool: &PgPool, skills: &[Skill]) -> ApiResult<usize> {
    for skill in skills {
        if !is_slug(&skill.id) || skill.prerequisites.iter().any(|id| !is_slug(id)) {
            return Err(ApiError::bad_request(format!(
                "skill id {:?} or one of its prerequisites is not valid",
                skill.id
            )));
        }
    }
    let mut tx = pool.begin().await?;
    for skill in skills {
        sqlx::query(
            "insert into skills (id, subject, name, grade, description, prerequisites)
             values ($1, $2, $3, $4, $5, $6)
             on conflict (id) do update set subject = excluded.subject, name = excluded.name,
               grade = excluded.grade, description = excluded.description, prerequisites = excluded.prerequisites",
        )
        .bind(&skill.id)
        .bind(&skill.subject)
        .bind(&skill.name)
        .bind(&skill.grade)
        .bind(&skill.description)
        .bind(&skill.prerequisites)
        .execute(&mut *tx)
        .await?;
    }
    tx.commit().await?;
    Ok(skills.len())
}

/// Why a skill is worth working on in the next session.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Reason {
    /// Known, and about to be forgotten.
    ReviewDue,
    /// Started and not yet mastered.
    InProgress,
    /// Not seen yet, and everything it builds on is mastered.
    Ready,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Candidate {
    pub skill_id: String,
    pub name: Option<String>,
    pub reason: Reason,
    /// Chance of a right answer today on an average question, if known.
    pub p_now: Option<f64>,
    /// Difficulty (logit) at which the child would get about four in five right.
    pub target_difficulty: Option<f64>,
}

pub const MAX_CANDIDATES_PER_REASON: usize = 3;

/// The skills worth offering to whoever plans the session, most useful first
/// within each reason. It narrows the choice; it does not make it.
pub fn candidates(
    trace: &[SkillTrace],
    catalog: &[Skill],
    grade: Option<&str>,
    now: DateTime<Utc>,
) -> Vec<Candidate> {
    let names: BTreeMap<&str, &Skill> = catalog.iter().map(|skill| (skill.id.as_str(), skill)).collect();
    let traced: BTreeMap<&str, &SkillTrace> =
        trace.iter().map(|skill| (skill.skill_id.as_str(), skill)).collect();
    let mastered: BTreeSet<&str> =
        trace.iter().filter(|skill| skill.mastered).map(|skill| skill.skill_id.as_str()).collect();
    let from_trace = |skill: &SkillTrace, reason| Candidate {
        skill_id: skill.skill_id.clone(),
        name: names.get(skill.skill_id.as_str()).map(|entry| entry.name.clone()),
        reason,
        p_now: Some(skill.p_now),
        target_difficulty: Some(skill.target_difficulty),
    };

    // Most forgotten first.
    let mut due: Vec<&SkillTrace> = trace
        .iter()
        .filter(|skill| skill.p_known >= 0.6 && skill.review_at.is_some_and(|review_at| review_at <= now))
        .collect();
    due.sort_by(|a, b| a.p_now.total_cmp(&b.p_now));
    let due_ids: BTreeSet<&str> = due.iter().map(|skill| skill.skill_id.as_str()).collect();

    // Closest to mastery first: the next success is the most motivating one.
    let mut in_progress: Vec<&SkillTrace> =
        trace.iter().filter(|skill| !skill.mastered && !due_ids.contains(skill.skill_id.as_str())).collect();
    in_progress.sort_by(|a, b| b.p_now.total_cmp(&a.p_now));

    // The child's own grade first, then catalog order.
    let mut ready: Vec<&Skill> = catalog
        .iter()
        .filter(|skill| !traced.contains_key(skill.id.as_str()))
        .filter(|skill| skill.prerequisites.iter().all(|id| mastered.contains(id.as_str())))
        .collect();
    ready.sort_by_key(|skill| grade.is_none() || skill.grade.as_deref() != grade);

    let take = MAX_CANDIDATES_PER_REASON;
    due.into_iter()
        .take(take)
        .map(|skill| from_trace(skill, Reason::ReviewDue))
        .chain(in_progress.into_iter().take(take).map(|skill| from_trace(skill, Reason::InProgress)))
        .chain(ready.into_iter().take(take).map(|skill| Candidate {
            skill_id: skill.id.clone(),
            name: Some(skill.name.clone()),
            reason: Reason::Ready,
            p_now: None,
            target_difficulty: None,
        }))
        .collect()
}

#[cfg(test)]
mod tests {
    use chrono::TimeZone;

    use super::*;

    fn day(day: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 11, day, 15, 0, 0).unwrap()
    }

    fn answers(skill: &str, results: &[bool], first_day: u32) -> Vec<TraceAttempt> {
        results
            .iter()
            .enumerate()
            .map(|(index, correct)| TraceAttempt {
                skill_id: skill.into(),
                question_id: None,
                correct: *correct,
                credit: None,
                at: day(first_day) + chrono::TimeDelta::minutes(index as i64),
                choice: false,
                difficulty: None,
            })
            .collect()
    }

    fn skill(id: &str, grade: &str, prerequisites: &[&str]) -> Skill {
        Skill {
            id: id.into(),
            subject: "math".into(),
            name: format!("Skill {id}"),
            grade: Some(grade.into()),
            description: None,
            prerequisites: prerequisites.iter().map(|id| id.to_string()).collect(),
        }
    }

    fn traced(attempts: Vec<TraceAttempt>, now: DateTime<Utc>) -> Vec<SkillTrace> {
        skill_model::trace(&TraceRequest {
            now: Some(now),
            attempts,
            corrections: vec![],
            params: Params::default(),
        })
        .unwrap()
        .skills
    }

    #[test]
    fn ids_are_slugs() {
        for id in ["fractions.add", "mult-2", "a", "3_digits"] {
            assert!(is_slug(id), "{id}");
        }
        for id in ["", "Fractions", "a b", "-a", "á", &"a".repeat(81), "a/b", "a;drop"] {
            assert!(!is_slug(id), "{id}");
        }
    }

    #[test]
    fn replay_matches_the_model() {
        let attempts =
            [answers("add", &[true, false, true, true], 1), answers("sub", &[false, false], 2)].concat();
        let corrections = vec![Correction { skill_id: "sub".into(), at: day(3), level: 0.9 }];
        let now = day(4);

        let model = replay(&attempts, &corrections, Params::default());
        let trace = skill_model::trace(&TraceRequest {
            now: Some(now),
            attempts,
            corrections,
            params: Params::default(),
        })
        .unwrap();

        assert!((model.ability - trace.ability).abs() < 1e-4);
        assert_eq!(model.skills.len(), trace.skills.len());
        assert!(model.skills["sub"].corrected);
    }

    #[test]
    fn a_new_child_is_offered_skills_with_no_prerequisites_from_their_grade_first() {
        let catalog = [skill("add", "2", &[]), skill("count", "1", &[]), skill("mult", "3", &["add"])];

        let offered = candidates(&[], &catalog, Some("1"), day(1));

        let ids: Vec<&str> = offered.iter().map(|candidate| candidate.skill_id.as_str()).collect();
        assert_eq!(ids, ["count", "add"]);
        assert!(
            offered.iter().all(|candidate| candidate.reason == Reason::Ready && candidate.p_now.is_none())
        );
    }

    #[test]
    fn mastering_a_skill_unlocks_what_builds_on_it_and_later_makes_it_due() {
        let catalog = [skill("add", "2", &[]), skill("mult", "3", &["add"])];
        let attempts = answers("add", &[true; 8], 1);

        let soon = candidates(&traced(attempts.clone(), day(1)), &catalog, None, day(1));
        assert_eq!(soon.len(), 1);
        assert_eq!((soon[0].skill_id.as_str(), soon[0].reason), ("mult", Reason::Ready));

        let later = candidates(&traced(attempts, day(25)), &catalog, None, day(25));
        assert_eq!((later[0].skill_id.as_str(), later[0].reason), ("add", Reason::ReviewDue));
        assert_eq!(later[0].name.as_deref(), Some("Skill add"));
    }

    #[test]
    fn unfinished_skills_come_closest_to_mastery_first_and_each_reason_is_capped() {
        let attempts = [
            answers("weak", &[false, false, false], 1),
            answers("close", &[true, true, false, true], 1),
            answers("a", &[false], 1),
            answers("b", &[false], 1),
        ]
        .concat();

        let offered = candidates(&traced(attempts, day(1)), &[], None, day(1));

        assert_eq!(offered.len(), MAX_CANDIDATES_PER_REASON);
        assert_eq!(offered[0].skill_id, "close");
        assert!(offered.iter().all(|candidate| candidate.reason == Reason::InProgress));
        assert!(offered.iter().all(|candidate| candidate.skill_id != "weak"));
    }
}
