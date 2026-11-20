//! Sign-in for guardians, without passwords: a code sent to their email, or
//! their Google account where the installation has set that up.
//!
//! Asking for a code answers the same way whether or not the address has an
//! account, a code works once and for ten minutes, a few wrong guesses burn
//! it, and an address only gets a few codes an hour. A new address becomes a
//! new family when its first code is confirmed.

use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;
use axum::http::request::Parts;
use rand::{Rng, RngExt};
use serde::Deserialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use subtle::ConstantTimeEq;
use uuid::Uuid;

use crate::auth::{Guardian, Input, bearer, token_hash};
use crate::error::{ApiError, ApiResult};
use crate::google::IdentityError;
use crate::state::AppState;

const CODE_MINUTES: i32 = 10;
const MAX_ATTEMPTS: i32 = 5;
const MAX_CODES_PER_HOUR: i32 = 5;
const SESSION_DAYS: i32 = 30;

pub fn normalized_email(email: &str) -> ApiResult<String> {
    let email = email.trim().to_lowercase();
    let mut parts = email.split('@');
    let valid = matches!((parts.next(), parts.next(), parts.next()), (Some(local), Some(domain), None)
        if !local.is_empty() && domain.contains('.') && !domain.starts_with('.') && !domain.ends_with('.'));
    if !valid || email.len() > 254 || email.chars().any(char::is_whitespace) {
        return Err(ApiError::bad_request("email is not an email address"));
    }
    Ok(email)
}

/// The code is short, so its hash is tied to the address it was sent to.
fn code_hash(email: &str, code: &str) -> String {
    hex::encode(Sha256::digest(format!("{email}:{code}").as_bytes()))
}

fn texts(language: &str, code: &str) -> (String, String) {
    match language {
        "en" => (
            format!("{code} is your Aule code"),
            format!(
                "Your code to sign in to Aule is {code}.\n\nIt works for {CODE_MINUTES} minutes. If you did not ask for it, you can ignore this message."
            ),
        ),
        _ => (
            format!("{code} es tu código de Aule"),
            format!(
                "Tu código para entrar a Aule es {code}.\n\nVale por {CODE_MINUTES} minutos. Si no lo pediste, puedes ignorar este mensaje."
            ),
        ),
    }
}

#[derive(Deserialize)]
pub struct StartInput {
    email: String,
    #[serde(default)]
    language: Option<String>,
}

pub async fn start(State(state): State<AppState>, Input(body): Input<StartInput>) -> ApiResult<StatusCode> {
    let email = normalized_email(&body.email)?;
    let mailer = state.mailer.as_ref().ok_or(ApiError::Unavailable("sign-in needs mail to be configured"))?;
    let code = format!("{:06}", rand::rng().random_range(0..1_000_000));

    // One statement decides whether this address may get another code now.
    let allowed: Option<i32> = sqlx::query_scalar(
        "insert into login_codes (email, code_hash, expires_at)
         values ($1, $2, now() + make_interval(mins => $3))
         on conflict (email) do update set
           code_hash = excluded.code_hash, expires_at = excluded.expires_at, attempts = 0,
           sent_count = case when login_codes.window_started_at < now() - interval '1 hour'
                             then 1 else login_codes.sent_count + 1 end,
           window_started_at = case when login_codes.window_started_at < now() - interval '1 hour'
                                    then now() else login_codes.window_started_at end
         where login_codes.window_started_at < now() - interval '1 hour' or login_codes.sent_count < $4
         returning sent_count",
    )
    .bind(&email)
    .bind(code_hash(&email, &code))
    .bind(CODE_MINUTES)
    .bind(MAX_CODES_PER_HOUR)
    .fetch_optional(&state.pool)
    .await?;

    if allowed.is_some() {
        let language = body.language.as_deref().unwrap_or("es");
        let (subject, text) = texts(language, &code);
        if let Err(error) = mailer.send(&email, &subject, &text).await {
            tracing::error!(%error, "a sign-in code could not be sent");
            return Err(ApiError::Unavailable("the code could not be sent; try again later"));
        }
    }
    // The same answer whether a code was sent, the limit was reached, or the
    // address is new.
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize)]
pub struct VerifyInput {
    email: String,
    code: String,
    /// Needed the first time an address signs in: the guardian's name.
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    language: Option<String>,
}

pub async fn verify(
    State(state): State<AppState>,
    Input(body): Input<VerifyInput>,
) -> ApiResult<Json<Value>> {
    let email = normalized_email(&body.email)?;
    let wrong = || ApiError::Forbidden("that code is wrong or has expired");

    let mut tx = state.pool.begin().await?;
    let pending: Option<(String, i32)> = sqlx::query_as(
        "select code_hash, attempts from login_codes where email = $1 and expires_at > now() for update",
    )
    .bind(&email)
    .fetch_optional(&mut *tx)
    .await?;
    let Some((expected, attempts)) = pending else { return Err(wrong()) };
    if attempts >= MAX_ATTEMPTS {
        return Err(wrong());
    }
    let given = code_hash(&email, body.code.trim());
    if !bool::from(given.as_bytes().ct_eq(expected.as_bytes())) {
        sqlx::query("update login_codes set attempts = attempts + 1 where email = $1")
            .bind(&email)
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        return Err(wrong());
    }

    let account = enter(&mut tx, &email, body.name.as_deref(), body.language.as_deref()).await?;
    sqlx::query("delete from login_codes where email = $1").bind(&email).execute(&mut *tx).await?;
    tx.commit().await?;
    Ok(Json(account))
}

/// Opens a session for an address that has just been proven, by a code or by
/// Google. An address seen for the first time becomes a new family, and for
/// that it needs the guardian's name.
async fn enter(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    email: &str,
    name: Option<&str>,
    language: Option<&str>,
) -> ApiResult<Value> {
    let existing: Option<(Uuid, Uuid)> =
        sqlx::query_as("select id, family_id from guardians where email = $1")
            .bind(email)
            .fetch_optional(&mut **tx)
            .await?;
    let (guardian_id, family_id, new_account) = match existing {
        Some((guardian_id, family_id)) => (guardian_id, family_id, false),
        None => {
            let name = name.map(str::trim).filter(|name| !name.is_empty() && name.chars().count() <= 120);
            let Some(name) = name else {
                return Err(ApiError::bad_request("name is needed to create the account"));
            };
            let language = language.filter(|language| ["es", "en"].contains(language)).unwrap_or("es");
            let (family_id, guardian_id) = (Uuid::now_v7(), Uuid::now_v7());
            sqlx::query("insert into families (id, name) values ($1, $2)")
                .bind(family_id)
                .bind(name)
                .execute(&mut **tx)
                .await?;
            sqlx::query(
                "insert into guardians (id, family_id, email, name, language) values ($1, $2, $3, $4, $5)",
            )
            .bind(guardian_id)
            .bind(family_id)
            .bind(email)
            .bind(name)
            .bind(language)
            .execute(&mut **tx)
            .await?;
            (guardian_id, family_id, true)
        }
    };

    let mut secret = [0u8; 32];
    rand::rng().fill_bytes(&mut secret);
    let token = hex::encode(secret);
    sqlx::query(
        "insert into guardian_sessions (token_hash, guardian_id, expires_at)
         values ($1, $2, now() + make_interval(days => $3))",
    )
    .bind(token_hash(&token))
    .bind(guardian_id)
    .bind(SESSION_DAYS)
    .execute(&mut **tx)
    .await?;

    Ok(json!({
        "token": token,
        "guardian_id": guardian_id,
        "family_id": family_id,
        "new_account": new_account,
        "expires_in_days": SESSION_DAYS,
    }))
}

const NO_GOOGLE: ApiError = ApiError::Unavailable("sign-in with Google is not set up here");

/// The client an app must ask Google to issue credentials for. It is not a
/// secret: it is part of every sign-in page.
pub async fn google_client(State(state): State<AppState>) -> ApiResult<Json<Value>> {
    let client_id = state.config.google_client_id.as_deref().ok_or(NO_GOOGLE)?;
    Ok(Json(json!({ "client_id": client_id })))
}

#[derive(Deserialize)]
pub struct GoogleInput {
    /// The ID token Google gave the app.
    credential: String,
    /// Used instead of the name in the Google account, for a new address.
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    language: Option<String>,
}

pub async fn google(
    State(state): State<AppState>,
    Input(body): Input<GoogleInput>,
) -> ApiResult<Json<Value>> {
    let provider = state.google.as_ref().ok_or(NO_GOOGLE)?;
    if body.credential.len() > 4096 {
        return Err(ApiError::bad_request("credential is not a Google ID token"));
    }
    let identity = provider.identify(&body.credential).await.map_err(|error| match error {
        IdentityError::Refused(reason) => {
            tracing::info!(%reason, "a Google sign-in was refused");
            ApiError::Forbidden("that Google sign-in could not be confirmed")
        }
        IdentityError::Unreachable(reason) => {
            tracing::error!(%reason, "Google could not be asked about a sign-in");
            ApiError::Unavailable("Google could not be reached; try again later")
        }
    })?;
    let email = normalized_email(&identity.email)?;
    let name =
        body.name.as_deref().map(str::trim).filter(|name| !name.is_empty()).or(identity.name.as_deref());

    let mut tx = state.pool.begin().await?;
    let account = enter(&mut tx, &email, name, body.language.as_deref()).await?;
    tx.commit().await?;
    Ok(Json(account))
}

/// Ends the session this request was made with.
pub async fn sign_out(_: Guardian, State(state): State<AppState>, parts: Parts) -> ApiResult<StatusCode> {
    sqlx::query("delete from guardian_sessions where token_hash = $1")
        .bind(token_hash(bearer(&parts)?))
        .execute(&state.pool)
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

pub async fn me(guardian: Guardian, State(state): State<AppState>) -> ApiResult<Json<Value>> {
    let (email, name, language, notices_by_email, notices_by_push): (String, String, String, bool, bool) = sqlx::query_as(
        "select email::text, name, language, notices_by_email, notices_by_push from guardians where id = $1",
    )
            .bind(guardian.id)
            .fetch_one(&state.pool)
            .await?;
    let children: Vec<(Uuid, String)> =
        sqlx::query_as("select id, nickname from children where family_id = $1 order by created_at")
            .bind(guardian.family_id)
            .fetch_all(&state.pool)
            .await?;
    let unread: i64 =
        sqlx::query_scalar("select count(*) from notices where family_id = $1 and read_at is null")
            .bind(guardian.family_id)
            .fetch_one(&state.pool)
            .await?;
    Ok(Json(json!({
        "guardian": { "id": guardian.id, "email": email, "name": name, "language": language, "notices_by_email": notices_by_email, "notices_by_push": notices_by_push },
        "family_id": guardian.family_id,
        "children": children.into_iter().map(|(id, nickname)| json!({ "id": id, "nickname": nickname })).collect::<Vec<_>>(),
        "unread_notices": unread,
    })))
}

#[derive(Deserialize)]
pub struct SettingsInput {
    #[serde(default)]
    language: Option<String>,
    #[serde(default)]
    notices_by_email: Option<bool>,
    #[serde(default)]
    notices_by_push: Option<bool>,
}

pub async fn update_me(
    guardian: Guardian,
    State(state): State<AppState>,
    Input(body): Input<SettingsInput>,
) -> ApiResult<StatusCode> {
    if body.language.as_deref().is_some_and(|language| !["es", "en"].contains(&language)) {
        return Err(ApiError::bad_request("language must be es or en"));
    }
    sqlx::query(
        "update guardians set language = coalesce($2, language),
           notices_by_email = coalesce($3, notices_by_email), notices_by_push = coalesce($4, notices_by_push)
         where id = $1",
    )
    .bind(guardian.id)
    .bind(body.language)
    .bind(body.notices_by_email)
    .bind(body.notices_by_push)
    .execute(&state.pool)
    .await?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize)]
pub struct EraseInput {
    /// The guardian's own email, typed again, so this cannot happen by accident.
    confirm_email: String,
}

/// Erases the family: its guardians, children and everything derived from
/// them. Devices go back to unclaimed. The consent records stay as proof.
pub async fn erase_family(
    guardian: Guardian,
    State(state): State<AppState>,
    Input(body): Input<EraseInput>,
) -> ApiResult<StatusCode> {
    let email: String = sqlx::query_scalar("select email::text from guardians where id = $1")
        .bind(guardian.id)
        .fetch_one(&state.pool)
        .await?;
    if normalized_email(&body.confirm_email)? != email {
        return Err(ApiError::bad_request("confirm_email must be the address you signed in with"));
    }
    let mut tx = state.pool.begin().await?;
    sqlx::query("update devices set claimed_at = null where family_id = $1")
        .bind(guardian.family_id)
        .execute(&mut *tx)
        .await?;
    sqlx::query("delete from families where id = $1").bind(guardian.family_id).execute(&mut *tx).await?;
    tx.commit().await?;
    Ok(StatusCode::NO_CONTENT)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn emails_are_normalized_and_checked() {
        assert_eq!(normalized_email("  Ana@Example.ORG ").unwrap(), "ana@example.org");
        for bad in [
            "",
            "ana",
            "ana@",
            "@example.org",
            "ana@example",
            "a b@example.org",
            "a@b@example.org",
            "ana@.org",
        ] {
            assert!(normalized_email(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn a_code_only_matches_for_the_address_it_was_sent_to() {
        assert_eq!(code_hash("a@example.org", "123456"), code_hash("a@example.org", "123456"));
        assert_ne!(code_hash("a@example.org", "123456"), code_hash("b@example.org", "123456"));
    }

    #[test]
    fn the_message_is_in_the_guardians_language() {
        assert!(texts("en", "123456").0.starts_with("123456 is your"));
        assert!(texts("es", "123456").1.contains("123456"));
        assert!(texts("fr", "1").0.contains("código"));
    }
}
