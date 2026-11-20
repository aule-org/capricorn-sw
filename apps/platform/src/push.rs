//! Push notifications to guardians' phones, through Firebase Cloud Messaging.
//!
//! A push can be read on a locked screen by anyone holding the phone, so it
//! only says that there is something to read: the notice itself, with the
//! child's name and what happened, stays in the app.

use std::sync::Mutex;
use std::time::{Duration, Instant};

use async_trait::async_trait;
use axum::extract::State;
use axum::http::StatusCode;
use jsonwebtoken::{Algorithm, EncodingKey, Header};
use sea_orm::ActiveValue::Set;
use sea_orm::sea_query::{Expr, OnConflict};
use sea_orm::{
    ColumnTrait, ConnectionTrait, DatabaseConnection, DbBackend, DbErr, EntityTrait, QueryFilter, Statement,
};
use serde::Deserialize;
use serde_json::{Value, json};
use uuid::Uuid;

use crate::auth::{Guardian, Input};
use crate::entity::push_token;
use crate::error::{ApiError, ApiResult};
use crate::state::AppState;

pub const PLATFORMS: [&str; 3] = ["android", "ios", "web"];
const SCOPE: &str = "https://www.googleapis.com/auth/firebase.messaging";

#[derive(Debug, PartialEq, Eq)]
pub enum Delivery {
    Sent,
    /// The push service no longer knows this token: the app was removed or
    /// the token was replaced. It should be forgotten.
    Gone,
}

#[derive(Debug, thiserror::Error)]
#[error("the push could not be sent: {0}")]
pub struct PushError(pub String);

pub struct Push<'a> {
    pub title: &'a str,
    pub body: &'a str,
    /// What the app needs to open the right screen. Ids only.
    pub data: Value,
}

#[async_trait]
pub trait Pusher: Send + Sync {
    async fn send(&self, token: &str, push: &Push<'_>) -> Result<Delivery, PushError>;
}

/// What a push says, by kind of notice and language. Fixed text, no names.
pub fn text(kind: &str, language: &str) -> (&'static str, &'static str) {
    match (kind, language) {
        ("attention", "en") => ("Aule", "There is a note that needs your attention."),
        ("attention", _) => ("Aule", "Hay un aviso que necesita tu atención."),
        ("session_stopped", "en") => ("Aule", "A session ended early. There is a note for you."),
        ("session_stopped", _) => ("Aule", "Una sesión terminó antes de tiempo. Hay un aviso para ti."),
        (_, "en") => ("Aule", "A session has finished. The summary is ready."),
        _ => ("Aule", "Terminó una sesión. El resumen ya está listo."),
    }
}

/// The key file Google gives for a service account.
#[derive(Clone, Deserialize)]
pub struct ServiceAccount {
    pub project_id: String,
    pub client_email: String,
    pub private_key: String,
    #[serde(default = "default_token_uri")]
    pub token_uri: String,
}

fn default_token_uri() -> String {
    "https://oauth2.googleapis.com/token".to_string()
}

impl std::fmt::Debug for ServiceAccount {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Never the key.
        formatter.debug_struct("ServiceAccount").field("project_id", &self.project_id).finish_non_exhaustive()
    }
}

pub struct Fcm {
    client: reqwest::Client,
    account: ServiceAccount,
    key: EncodingKey,
    access: Mutex<Option<(String, Instant)>>,
}

/// The signed request that asks Google for an access token.
pub fn assertion(account: &ServiceAccount, key: &EncodingKey, now: i64) -> Result<String, PushError> {
    let claims = json!({
        "iss": account.client_email,
        "scope": SCOPE,
        "aud": account.token_uri,
        "iat": now,
        "exp": now + 3600,
    });
    jsonwebtoken::encode(&Header::new(Algorithm::RS256), &claims, key)
        .map_err(|error| PushError(error.to_string()))
}

pub fn message(token: &str, push: &Push<'_>) -> Value {
    // FCM only carries string values in `data`.
    let data: serde_json::Map<String, Value> = push
        .data
        .as_object()
        .into_iter()
        .flatten()
        .map(|(name, value)| {
            (name.clone(), json!(value.as_str().map_or_else(|| value.to_string(), str::to_string)))
        })
        .collect();
    json!({ "message": {
        "token": token,
        "notification": { "title": push.title, "body": push.body },
        "data": data,
    }})
}

/// Whether an answer from FCM means the token is dead, rather than that the
/// send failed for a reason that may pass.
pub fn is_gone(status: u16, body: &Value) -> bool {
    let code = body["error"]["details"]
        .as_array()
        .into_iter()
        .flatten()
        .find_map(|detail| detail["errorCode"].as_str())
        .unwrap_or_default();
    status == 404 || code == "UNREGISTERED" || (status == 400 && code == "INVALID_ARGUMENT")
}

impl Fcm {
    pub fn new(account: ServiceAccount) -> Result<Self, PushError> {
        let key = EncodingKey::from_rsa_pem(account.private_key.as_bytes())
            .map_err(|_| PushError("the service account's private key could not be read".into()))?;
        Ok(Self { client: reqwest::Client::new(), account, key, access: Mutex::new(None) })
    }

    async fn access_token(&self) -> Result<String, PushError> {
        if let Some((token, expires)) = self.access.lock().expect("lock").as_ref()
            && Instant::now() < *expires
        {
            return Ok(token.clone());
        }
        let assertion = assertion(&self.account, &self.key, chrono::Utc::now().timestamp())?;
        let response = self
            .client
            .post(&self.account.token_uri)
            .timeout(Duration::from_secs(10))
            .form(&[
                ("grant_type", "urn:ietf:params:oauth:grant-type:jwt-bearer"),
                ("assertion", assertion.as_str()),
            ])
            .send()
            .await
            .map_err(|error| PushError(error.without_url().to_string()))?;
        if !response.status().is_success() {
            return Err(PushError(format!("the token service answered {}", response.status().as_u16())));
        }
        let body: Value = response.json().await.map_err(|error| PushError(error.to_string()))?;
        let token =
            body["access_token"].as_str().ok_or_else(|| PushError("no access token".into()))?.to_string();
        let lifetime = body["expires_in"].as_u64().unwrap_or(3600).saturating_sub(60);
        *self.access.lock().expect("lock") =
            Some((token.clone(), Instant::now() + Duration::from_secs(lifetime)));
        Ok(token)
    }
}

#[async_trait]
impl Pusher for Fcm {
    async fn send(&self, token: &str, push: &Push<'_>) -> Result<Delivery, PushError> {
        let access = self.access_token().await?;
        let response = self
            .client
            .post(format!("https://fcm.googleapis.com/v1/projects/{}/messages:send", self.account.project_id))
            .timeout(Duration::from_secs(10))
            .bearer_auth(access)
            .json(&message(token, push))
            .send()
            .await
            .map_err(|error| PushError(error.without_url().to_string()))?;
        let status = response.status().as_u16();
        if response.status().is_success() {
            return Ok(Delivery::Sent);
        }
        let body: Value = response.json().await.unwrap_or(Value::Null);
        if is_gone(status, &body) {
            Ok(Delivery::Gone)
        } else {
            Err(PushError(format!("FCM answered {status}")))
        }
    }
}

/// Tells every phone of the family's guardians who want pushes that a notice
/// is waiting, and forgets tokens the push service no longer knows. Returns
/// whether any phone was reached.
pub async fn announce(
    db: &DatabaseConnection,
    pusher: &dyn Pusher,
    family_id: Uuid,
    notice_id: Uuid,
    kind: &str,
) -> Result<bool, DbErr> {
    let rows = db
        .query_all_raw(Statement::from_sql_and_values(
            DbBackend::Postgres,
            "select push_tokens.token, guardians.language from push_tokens
             join guardians on guardians.id = push_tokens.guardian_id
             where guardians.family_id = $1 and guardians.notices_by_push",
            [family_id.into()],
        ))
        .await?;
    let mut reached = false;
    for row in rows {
        let token: String = row.try_get("", "token")?;
        let language: String = row.try_get("", "language")?;
        let (title, body) = text(kind, &language);
        let push = Push { title, body, data: json!({ "notice_id": notice_id, "kind": kind }) };
        match pusher.send(&token, &push).await {
            Ok(Delivery::Sent) => reached = true,
            Ok(Delivery::Gone) => {
                push_token::Entity::delete_by_id(token.clone()).exec(db).await?;
            }
            // The notice is still in the app; the push is not retried.
            Err(error) => tracing::warn!(%notice_id, %error, "a notice was not pushed"),
        }
    }
    Ok(reached)
}

#[derive(Deserialize)]
pub struct TokenInput {
    token: String,
    #[serde(default)]
    platform: Option<String>,
}

/// The app registers this phone, each time it starts. A token that was
/// another guardian's (someone else signed in on this phone before) moves.
pub async fn register(
    guardian: Guardian,
    State(state): State<AppState>,
    Input(body): Input<TokenInput>,
) -> ApiResult<StatusCode> {
    let token = body.token.trim();
    let platform = body.platform.as_deref().unwrap_or_default();
    if !(20..=4096).contains(&token.len()) || !PLATFORMS.contains(&platform) {
        return Err(ApiError::bad_request("a push token and a platform (android, ios or web) are needed"));
    }
    push_token::Entity::insert(push_token::ActiveModel {
        token: Set(token.to_string()),
        guardian_id: Set(guardian.id),
        platform: Set(platform.to_string()),
        ..Default::default()
    })
    .on_conflict(
        OnConflict::column(push_token::Column::Token)
            .update_columns([push_token::Column::GuardianId, push_token::Column::Platform])
            .value(push_token::Column::LastSeenAt, Expr::current_timestamp())
            .to_owned(),
    )
    .exec_without_returning(&state.db)
    .await?;
    Ok(StatusCode::NO_CONTENT)
}

/// The app forgets this phone, when the guardian signs out of it.
pub async fn unregister(
    guardian: Guardian,
    State(state): State<AppState>,
    Input(body): Input<TokenInput>,
) -> ApiResult<StatusCode> {
    push_token::Entity::delete_many()
        .filter(push_token::Column::Token.eq(body.token.trim()))
        .filter(push_token::Column::GuardianId.eq(guardian.id))
        .exec(&state.db)
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_push_never_names_the_child_or_what_happened() {
        for kind in ["attention", "session_summary"] {
            for language in ["es", "en", "fr"] {
                let (title, body) = text(kind, language);
                assert_eq!(title, "Aule");
                assert!(!body.is_empty());
            }
        }
        assert_ne!(text("attention", "es").1, text("session_summary", "es").1);
        assert!(text("attention", "en").1.contains("attention"));
    }

    #[test]
    fn the_message_carries_ids_as_strings() {
        let push = Push { title: "Aule", body: "Hi", data: json!({ "notice_id": "n-1", "count": 2 }) };

        let message = message("device-token", &push);

        assert_eq!(message["message"]["token"], "device-token");
        assert_eq!(message["message"]["notification"], json!({ "title": "Aule", "body": "Hi" }));
        assert_eq!(message["message"]["data"], json!({ "notice_id": "n-1", "count": "2" }));
    }

    #[test]
    fn a_dead_token_is_told_apart_from_a_failed_send() {
        let error = |code: &str| json!({ "error": { "details": [{ "errorCode": code }] } });

        assert!(is_gone(404, &Value::Null));
        assert!(is_gone(404, &error("UNREGISTERED")));
        assert!(is_gone(400, &error("INVALID_ARGUMENT")));
        assert!(!is_gone(400, &error("OTHER")));
        assert!(!is_gone(429, &error("QUOTA_EXCEEDED")));
        assert!(!is_gone(503, &Value::Null));
        assert!(!is_gone(401, &Value::Null));
    }

    #[test]
    fn the_key_is_not_printed() {
        let account = ServiceAccount {
            project_id: "aule".into(),
            client_email: "push@aule.iam.gserviceaccount.com".into(),
            private_key: "-----BEGIN PRIVATE KEY-----secret".into(),
            token_uri: default_token_uri(),
        };

        let printed = format!("{account:?}");

        assert!(printed.contains("aule") && !printed.contains("secret"));
        assert!(Fcm::new(account).is_err(), "a key that is not a key is refused at start");
    }
}
