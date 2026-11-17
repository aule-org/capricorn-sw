//! Who is calling. Two kinds of caller exist today:
//!
//! - an **operator**, with the server's admin token. It stands in for family
//!   sign-in, which is not built yet;
//! - the **agent**, with a token that was created for one session and is
//!   useless for any other.

use axum::extract::rejection::JsonRejection;
use axum::extract::{FromRequest, FromRequestParts, Request};
use axum::http::header::AUTHORIZATION;
use axum::http::request::Parts;
use chrono::{DateTime, Utc};
use serde::de::DeserializeOwned;
use sha2::{Digest, Sha256};
use subtle::ConstantTimeEq;
use uuid::Uuid;

use crate::error::ApiError;
use crate::state::AppState;

fn bearer(parts: &Parts) -> Result<&str, ApiError> {
    parts
        .headers
        .get(AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "))
        .map(str::trim)
        .filter(|token| !token.is_empty())
        .ok_or(ApiError::Unauthorized)
}

pub fn token_hash(token: &str) -> String {
    hex::encode(Sha256::digest(token.as_bytes()))
}

/// Compares in constant time, whatever the lengths.
pub fn same_token(given: &str, expected: &str) -> bool {
    Sha256::digest(given.as_bytes()).ct_eq(&Sha256::digest(expected.as_bytes())).into()
}

pub struct Operator;

impl FromRequestParts<AppState> for Operator {
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, state: &AppState) -> Result<Self, Self::Rejection> {
        if same_token(bearer(parts)?, &state.config.admin_token) {
            Ok(Self)
        } else {
            Err(ApiError::Unauthorized)
        }
    }
}

#[derive(Clone, Debug, sqlx::FromRow)]
pub struct Session {
    pub id: Uuid,
    pub child_id: Option<Uuid>,
    pub audience: String,
    pub room: String,
    pub language: String,
    pub status: String,
    pub keep_transcript: bool,
    pub use_memory: bool,
    pub created_at: DateTime<Utc>,
    pub ended_at: Option<DateTime<Utc>>,
}

pub const SESSION_COLUMNS: &str =
    "id, child_id, audience, room, language, status, keep_transcript, use_memory, created_at, ended_at";

/// How long after it was created a session's token still works.
pub const SESSION_TOKEN_HOURS: i32 = 3;

/// The agent, acting for the one session its token belongs to.
pub struct Agent(pub Session);

impl FromRequestParts<AppState> for Agent {
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, state: &AppState) -> Result<Self, Self::Rejection> {
        let hash = token_hash(bearer(parts)?);
        let session: Option<Session> = sqlx::query_as(&format!(
            "select {SESSION_COLUMNS} from sessions
             where agent_token_hash = $1 and created_at > now() - make_interval(hours => $2)"
        ))
        .bind(hash)
        .bind(SESSION_TOKEN_HOURS)
        .fetch_optional(&state.pool)
        .await?;
        session.map(Self).ok_or(ApiError::Unauthorized)
    }
}

/// A JSON body, with a malformed one answered in the API's own error shape.
pub struct Input<T>(pub T);

impl<T: DeserializeOwned> FromRequest<AppState> for Input<T> {
    type Rejection = ApiError;

    async fn from_request(request: Request, state: &AppState) -> Result<Self, Self::Rejection> {
        match axum::Json::<T>::from_request(request, state).await {
            Ok(axum::Json(value)) => Ok(Self(value)),
            Err(rejection @ (JsonRejection::JsonDataError(_) | JsonRejection::JsonSyntaxError(_))) => {
                Err(ApiError::BadRequest(rejection.body_text()))
            }
            Err(_) => Err(ApiError::bad_request("the body must be JSON with Content-Type: application/json")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_exact_token_matches() {
        assert!(same_token("a-long-enough-token", "a-long-enough-token"));
        assert!(!same_token("a-long-enough-toke", "a-long-enough-token"));
        assert!(!same_token("", "a-long-enough-token"));
    }

    #[test]
    fn a_token_is_stored_as_its_hash() {
        assert_eq!(token_hash("abc"), "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
    }
}
