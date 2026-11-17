//! Families, the children in them, and what their guardians have consented to.

use std::collections::BTreeMap;

use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use chrono::{DateTime, Datelike, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sqlx::PgPool;
use uuid::Uuid;

use crate::auth::{Caller, Input, Operator};
use crate::error::{ApiError, ApiResult};
use crate::state::AppState;
use crate::supervisor::ChildContext;

pub const PURPOSES: [&str; 3] = ["voice", "transcripts", "memory"];

fn text(value: &str, field: &str, max_chars: usize) -> ApiResult<String> {
    let value = value.trim();
    if value.is_empty() || value.chars().count() > max_chars {
        return Err(ApiError::bad_request(format!("{field} must have 1 to {max_chars} characters")));
    }
    Ok(value.to_string())
}

#[derive(Deserialize)]
pub struct NewGuardian {
    email: String,
    name: String,
}

#[derive(Deserialize)]
pub struct NewFamily {
    name: String,
    guardian: NewGuardian,
}

pub async fn create_family(
    _: Operator,
    State(state): State<AppState>,
    Input(body): Input<NewFamily>,
) -> ApiResult<(StatusCode, Json<Value>)> {
    let name = text(&body.name, "name", 120)?;
    let guardian_name = text(&body.guardian.name, "guardian.name", 120)?;
    let email = body.guardian.email.trim().to_lowercase();
    if email.len() > 254 || email.split('@').filter(|part| !part.is_empty()).count() != 2 {
        return Err(ApiError::bad_request("guardian.email is not an email address"));
    }

    let (family_id, guardian_id) = (Uuid::now_v7(), Uuid::now_v7());
    let mut tx = state.pool.begin().await?;
    sqlx::query("insert into families (id, name) values ($1, $2)")
        .bind(family_id)
        .bind(&name)
        .execute(&mut *tx)
        .await?;
    let inserted = sqlx::query(
        "insert into guardians (id, family_id, email, name) values ($1, $2, $3, $4) on conflict (email) do nothing",
    )
    .bind(guardian_id)
    .bind(family_id)
    .bind(&email)
    .bind(&guardian_name)
    .execute(&mut *tx)
    .await?;
    if inserted.rows_affected() == 0 {
        return Err(ApiError::Conflict("a guardian with that email already exists"));
    }
    tx.commit().await?;
    Ok((StatusCode::CREATED, Json(json!({ "family_id": family_id, "guardian_id": guardian_id }))))
}

#[derive(Clone, Debug, Serialize, sqlx::FromRow)]
pub struct Child {
    pub id: Uuid,
    pub family_id: Uuid,
    pub nickname: String,
    pub birth_year: Option<i32>,
    pub grade: Option<String>,
    pub language: String,
    pub interests: Vec<String>,
    pub notes: Option<String>,
    pub created_at: DateTime<Utc>,
}

const CHILD_COLUMNS: &str =
    "id, family_id, nickname, birth_year, grade, language, interests, notes, created_at";

impl Child {
    pub fn age(&self, now: DateTime<Utc>) -> Option<i32> {
        self.birth_year.map(|year| now.year() - year)
    }

    /// What the supervisor may know: no name, no ids.
    pub fn context(&self, now: DateTime<Utc>) -> ChildContext {
        ChildContext {
            age: self.age(now),
            grade: self.grade.clone(),
            language: self.language.clone(),
            interests: self.interests.clone(),
            notes: self.notes.clone(),
        }
    }
}

pub async fn find_child(pool: &PgPool, child_id: Uuid) -> ApiResult<Child> {
    sqlx::query_as(&format!("select {CHILD_COLUMNS} from children where id = $1"))
        .bind(child_id)
        .fetch_optional(pool)
        .await?
        .ok_or(ApiError::NotFound("child"))
}

/// The child, if the caller may see them. A child of another family does not
/// exist as far as a guardian can tell.
pub async fn child_for(pool: &PgPool, caller: &Caller, child_id: Uuid) -> ApiResult<Child> {
    let child = find_child(pool, child_id).await?;
    if caller.may_access(child.family_id) { Ok(child) } else { Err(ApiError::NotFound("child")) }
}

#[derive(Deserialize)]
pub struct ChildInput {
    nickname: String,
    #[serde(default)]
    birth_year: Option<i32>,
    #[serde(default)]
    grade: Option<String>,
    #[serde(default)]
    language: Option<String>,
    #[serde(default)]
    interests: Vec<String>,
    #[serde(default)]
    notes: Option<String>,
}

struct CheckedChild {
    nickname: String,
    birth_year: Option<i32>,
    grade: Option<String>,
    language: String,
    interests: Vec<String>,
    notes: Option<String>,
}

impl ChildInput {
    fn checked(self, now: DateTime<Utc>) -> ApiResult<CheckedChild> {
        let optional = |value: Option<String>, field: &str, max: usize| {
            value.filter(|value| !value.trim().is_empty()).map(|value| text(&value, field, max)).transpose()
        };
        // A student from about 3 to 19 years old.
        if self.birth_year.is_some_and(|year| !(now.year() - 19..=now.year() - 3).contains(&year)) {
            return Err(ApiError::bad_request("birth_year is not a school student's"));
        }
        let language = self.language.unwrap_or_else(|| "es".to_string());
        if !["es", "en"].contains(&language.as_str()) {
            return Err(ApiError::bad_request("language must be es or en"));
        }
        let grade = self.grade.map(|grade| grade.trim().to_uppercase()).filter(|grade| !grade.is_empty());
        if grade.as_deref().is_some_and(|grade| !crate::learning::is_grade(grade)) {
            return Err(ApiError::bad_request("grade must be P1 to P6 (primary) or S1 to S5 (secondary)"));
        }
        if self.interests.len() > 10 {
            return Err(ApiError::bad_request("at most 10 interests"));
        }
        Ok(CheckedChild {
            nickname: text(&self.nickname, "nickname", 40)?,
            birth_year: self.birth_year,
            grade,
            language,
            interests: self
                .interests
                .iter()
                .map(|item| text(item, "an interest", 40))
                .collect::<ApiResult<_>>()?,
            notes: optional(self.notes, "notes", 1000)?,
        })
    }
}

pub async fn create_child(
    caller: Caller,
    State(state): State<AppState>,
    Path(family_id): Path<Uuid>,
    Input(body): Input<ChildInput>,
) -> ApiResult<(StatusCode, Json<Child>)> {
    if !caller.may_access(family_id) {
        return Err(ApiError::NotFound("family"));
    }
    let child = body.checked(Utc::now())?;
    let family: Option<Uuid> = sqlx::query_scalar("select id from families where id = $1")
        .bind(family_id)
        .fetch_optional(&state.pool)
        .await?;
    family.ok_or(ApiError::NotFound("family"))?;
    let created: Child = sqlx::query_as(&format!(
        "insert into children (id, family_id, nickname, birth_year, grade, language, interests, notes)
         values ($1, $2, $3, $4, $5, $6, $7, $8) returning {CHILD_COLUMNS}"
    ))
    .bind(Uuid::now_v7())
    .bind(family_id)
    .bind(child.nickname)
    .bind(child.birth_year)
    .bind(child.grade)
    .bind(child.language)
    .bind(child.interests)
    .bind(child.notes)
    .fetch_one(&state.pool)
    .await?;
    Ok((StatusCode::CREATED, Json(created)))
}

pub async fn update_child(
    caller: Caller,
    State(state): State<AppState>,
    Path(child_id): Path<Uuid>,
    Input(body): Input<ChildInput>,
) -> ApiResult<Json<Child>> {
    child_for(&state.pool, &caller, child_id).await?;
    let child = body.checked(Utc::now())?;
    let updated: Option<Child> = sqlx::query_as(&format!(
        "update children set nickname = $2, birth_year = $3, grade = $4, language = $5, interests = $6, notes = $7
         where id = $1 returning {CHILD_COLUMNS}"
    ))
    .bind(child_id)
    .bind(child.nickname)
    .bind(child.birth_year)
    .bind(child.grade)
    .bind(child.language)
    .bind(child.interests)
    .bind(child.notes)
    .fetch_optional(&state.pool)
    .await?;
    updated.map(Json).ok_or(ApiError::NotFound("child"))
}

pub async fn get_child(
    caller: Caller,
    State(state): State<AppState>,
    Path(child_id): Path<Uuid>,
) -> ApiResult<Json<Value>> {
    let child = child_for(&state.pool, &caller, child_id).await?;
    let consent = current_consent(&state.pool, child_id).await?;
    Ok(Json(json!({ "child": child, "consent": consent })))
}

/// Erases the child and everything derived from them: sessions, transcripts,
/// answers, memories and reviews go with the row. The consent records stay,
/// as the proof of what was agreed while the data existed.
pub async fn delete_child(
    caller: Caller,
    State(state): State<AppState>,
    Path(child_id): Path<Uuid>,
) -> ApiResult<StatusCode> {
    child_for(&state.pool, &caller, child_id).await?;
    let deleted =
        sqlx::query("delete from children where id = $1").bind(child_id).execute(&state.pool).await?;
    if deleted.rows_affected() == 0 {
        return Err(ApiError::NotFound("child"));
    }
    Ok(StatusCode::NO_CONTENT)
}

/// The latest answer for each purpose. A purpose never answered is not granted.
pub async fn current_consent(pool: &PgPool, child_id: Uuid) -> Result<BTreeMap<String, bool>, sqlx::Error> {
    let rows: Vec<(String, bool)> = sqlx::query_as(
        "select distinct on (purpose) purpose, granted from consent_records
         where child_id = $1 order by purpose, created_at desc, id desc",
    )
    .bind(child_id)
    .fetch_all(pool)
    .await?;
    let mut consent: BTreeMap<String, bool> =
        PURPOSES.iter().map(|purpose| (purpose.to_string(), false)).collect();
    consent.extend(rows);
    Ok(consent)
}

#[derive(Deserialize)]
pub struct ConsentInput {
    purpose: String,
    granted: bool,
    policy_version: String,
    #[serde(default)]
    guardian_id: Option<Uuid>,
    #[serde(default)]
    locale: Option<String>,
    /// Where the guardian answered, such as "paper-form". A signed-in
    /// guardian's answer is always recorded as "app".
    #[serde(default)]
    source: Option<String>,
}

pub async fn record_consent(
    caller: Caller,
    State(state): State<AppState>,
    Path(child_id): Path<Uuid>,
    Input(body): Input<ConsentInput>,
) -> ApiResult<(StatusCode, Json<Value>)> {
    if !PURPOSES.contains(&body.purpose.as_str()) {
        return Err(ApiError::bad_request(format!("purpose must be one of: {}", PURPOSES.join(", "))));
    }
    let child = child_for(&state.pool, &caller, child_id).await?;
    // A guardian answers for themselves; only the operator records an answer
    // given some other way, and must say how.
    let (guardian_id, source) = match caller.guardian_id() {
        Some(guardian_id) => (Some(guardian_id), "app".to_string()),
        None => {
            let source = body.source.as_deref().ok_or_else(|| ApiError::bad_request("source is required"))?;
            (body.guardian_id, text(source, "source", 40)?)
        }
    };
    if let Some(guardian_id) = guardian_id {
        let belongs: Option<Uuid> =
            sqlx::query_scalar("select id from guardians where id = $1 and family_id = $2")
                .bind(guardian_id)
                .bind(child.family_id)
                .fetch_optional(&state.pool)
                .await?;
        belongs.ok_or(ApiError::Forbidden("that guardian is not in the child's family"))?;
    }
    sqlx::query(
        "insert into consent_records (id, family_id, child_id, guardian_id, purpose, granted, policy_version, locale, source)
         values ($1, $2, $3, $4, $5, $6, $7, $8, $9)",
    )
    .bind(Uuid::now_v7())
    .bind(child.family_id)
    .bind(child_id)
    .bind(guardian_id)
    .bind(&body.purpose)
    .bind(body.granted)
    .bind(text(&body.policy_version, "policy_version", 40)?)
    .bind(body.locale.as_deref().map(|locale| text(locale, "locale", 20)).transpose()?.unwrap_or_else(|| "es-PE".into()))
    .bind(source)
    .execute(&state.pool)
    .await?;

    // Taking consent back takes the data back too.
    if !body.granted {
        match body.purpose.as_str() {
            "memory" => {
                sqlx::query("delete from memories where child_id = $1")
                    .bind(child_id)
                    .execute(&state.pool)
                    .await?;
            }
            "transcripts" => {
                sqlx::query(
                    "delete from session_turns using sessions
                     where session_turns.session_id = sessions.id and sessions.child_id = $1 and sessions.status = 'ended'",
                )
                .bind(child_id)
                .execute(&state.pool)
                .await?;
            }
            _ => {}
        }
    }
    let consent = current_consent(&state.pool, child_id).await?;
    Ok((StatusCode::CREATED, Json(json!({ "consent": consent }))))
}
