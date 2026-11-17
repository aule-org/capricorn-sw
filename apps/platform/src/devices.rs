//! Devices: how one proves who it is, how a family links one, and how it gets
//! firmware and sessions.
//!
//! A device is known by its serial number and holds a 32-byte HMAC key that
//! was burned into it when it was made and never leaves it. The server keeps a
//! copy and trusts a device only for what it signs with that key. Its MAC
//! address, or knowing its serial number, proves nothing.
//!
//! Linking a device to a family needs two halves, in either order:
//!
//! 1. the device shows a short code, and a signed-in guardian types it: that
//!    person can see the device;
//! 2. the device signs the challenge that came with the code: it is a real
//!    device, and the one the code was given to.
//!
//! The routes under `/ota/` speak the check-in and activation protocol of
//! xiaozhi-esp32, which the Capricorn firmware is built on.

use axum::Json;
use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode};
use chrono::Utc;
use hmac::{Hmac, Mac};
use rand::Rng;
use serde::Deserialize;
use serde_json::{Value, json};
use sha2::Sha256;
use sqlx::{Postgres, Transaction};
use uuid::Uuid;

use crate::auth::{Caller, Guardian, Input, Operator};
use crate::error::{ApiError, ApiResult};
use crate::families::{child_for, find_child};
use crate::state::AppState;
use crate::{events, sessions};

const CLAIM_MINUTES: i32 = 10;
/// No look-alike characters: the code is read from a small screen and typed.
const CODE_ALPHABET: &[u8] = b"ABCDEFGHJKLMNPQRSTUVWXYZ23456789";
const CODE_LENGTH: usize = 6;
/// Wrong codes one guardian may type in an hour.
const MAX_WRONG_CODES: u32 = 10;
/// How far a device's clock may be from the server's when it signs a request.
const CLOCK_SKEW_SECONDS: i64 = 120;

/// Whether `signature_hex` is HMAC-SHA256 of `message` under the device's key.
pub fn signed_by(key: &[u8], message: &[u8], signature_hex: &str) -> bool {
    let Ok(signature) = hex::decode(signature_hex.trim()) else { return false };
    let mut mac = Hmac::<Sha256>::new_from_slice(key).expect("HMAC accepts any key length");
    mac.update(message);
    mac.verify_slice(&signature).is_ok()
}

fn random_code() -> String {
    let mut rng = rand::rng();
    (0..CODE_LENGTH).map(|_| CODE_ALPHABET[rng.random_range(0..CODE_ALPHABET.len())] as char).collect()
}

fn random_challenge() -> String {
    hex::encode(rand::rng().random::<[u8; 16]>())
}

// --- the operator: devices as they are made, and firmware ---------------------------

#[derive(Deserialize)]
pub struct NewDevice {
    serial: String,
    /// The 32-byte key burned into the device, as 64 hex characters.
    hmac_key: String,
    /// The board, such as `capricorn-v0.2`. Firmware is published per model.
    model: String,
}

#[derive(Deserialize)]
pub struct NewDevices {
    devices: Vec<NewDevice>,
}

pub async fn register(
    _: Operator,
    State(state): State<AppState>,
    Input(body): Input<NewDevices>,
) -> ApiResult<(StatusCode, Json<Value>)> {
    if body.devices.is_empty() || body.devices.len() > 1000 {
        return Err(ApiError::bad_request("send 1 to 1000 devices"));
    }
    let mut tx = state.pool.begin().await?;
    let mut registered = 0;
    for device in &body.devices {
        let serial_ok = (4..=32).contains(&device.serial.len())
            && device.serial.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'));
        let key = hex::decode(&device.hmac_key).ok().filter(|key| key.len() == 32);
        let model = device.model.trim();
        let (true, Some(key), 1..=60) = (serial_ok, key, model.len()) else {
            return Err(ApiError::bad_request(format!(
                "device {:?} needs a serial, a 32-byte hmac_key in hex and a model",
                device.serial
            )));
        };
        // A serial that is already registered keeps its key: re-sending the
        // same list is harmless, and a key cannot be replaced by accident.
        registered += sqlx::query(
            "insert into devices (id, serial, hmac_key, model) values ($1, $2, $3, $4) on conflict (serial) do nothing",
        )
        .bind(Uuid::now_v7())
        .bind(&device.serial)
        .bind(key)
        .bind(model)
        .execute(&mut *tx)
        .await?
        .rows_affected();
    }
    tx.commit().await?;
    Ok((
        StatusCode::CREATED,
        Json(json!({ "registered": registered, "already_known": body.devices.len() as u64 - registered })),
    ))
}

#[derive(serde::Serialize, sqlx::FromRow)]
struct DeviceView {
    id: Uuid,
    serial: String,
    model: String,
    family_id: Option<Uuid>,
    child_id: Option<Uuid>,
    claimed_at: Option<chrono::DateTime<Utc>>,
    firmware_version: Option<String>,
    last_seen_at: Option<chrono::DateTime<Utc>>,
}

const DEVICE_VIEW: &str =
    "id, serial, model, family_id, child_id, claimed_at, firmware_version, last_seen_at";

/// A guardian sees their family's devices; the operator sees the latest 200.
pub async fn list(caller: Caller, State(state): State<AppState>) -> ApiResult<Json<Value>> {
    let devices: Vec<DeviceView> = match caller {
        Caller::Guardian { family_id, .. } => {
            sqlx::query_as(&format!(
                "select {DEVICE_VIEW} from devices where family_id = $1 order by claimed_at"
            ))
            .bind(family_id)
            .fetch_all(&state.pool)
            .await?
        }
        Caller::Operator => {
            sqlx::query_as(&format!("select {DEVICE_VIEW} from devices order by created_at desc limit 200"))
                .fetch_all(&state.pool)
                .await?
        }
    };
    Ok(Json(json!({ "devices": devices })))
}

#[derive(Deserialize)]
pub struct NewFirmware {
    model: String,
    /// A semantic version, such as 1.4.0.
    version: String,
    url: String,
    sha256: String,
}

pub async fn publish_firmware(
    _: Operator,
    State(state): State<AppState>,
    Input(body): Input<NewFirmware>,
) -> ApiResult<StatusCode> {
    let version = semver::Version::parse(body.version.trim())
        .map_err(|_| ApiError::bad_request("version must be a semantic version such as 1.4.0"))?;
    let sha256 = body.sha256.trim().to_lowercase();
    let model = body.model.trim();
    if !body.url.starts_with("https://")
        || sha256.len() != 64
        || !sha256.chars().all(|c| c.is_ascii_hexdigit())
        || !(1..=60).contains(&model.len())
    {
        return Err(ApiError::bad_request("firmware needs a model, an https url and a sha256 in hex"));
    }
    let inserted = sqlx::query(
        "insert into firmware (id, model, version, major, minor, patch, url, sha256)
         values ($1, $2, $3, $4, $5, $6, $7, $8) on conflict (model, version) do nothing",
    )
    .bind(Uuid::now_v7())
    .bind(model)
    .bind(version.to_string())
    .bind(version.major as i32)
    .bind(version.minor as i32)
    .bind(version.patch as i32)
    .bind(&body.url)
    .bind(sha256)
    .execute(&state.pool)
    .await?;
    if inserted.rows_affected() == 0 {
        // A published version is never replaced: devices may already run it.
        return Err(ApiError::Conflict("that version is already published for that model"));
    }
    Ok(StatusCode::CREATED)
}

// --- the guardian: claiming and managing a device -------------------------------------

#[derive(Deserialize)]
pub struct ClaimInput {
    code: String,
    #[serde(default)]
    child_id: Option<Uuid>,
}

pub async fn claim(
    guardian: Guardian,
    State(state): State<AppState>,
    Input(body): Input<ClaimInput>,
) -> ApiResult<Json<Value>> {
    let too_many = || ApiError::Forbidden("too many wrong codes; try again in an hour");
    {
        let attempts = state.claim_attempts.lock().expect("lock");
        if attempts
            .get(&guardian.id)
            .is_some_and(|(count, since)| *count >= MAX_WRONG_CODES && since.elapsed().as_secs() < 3600)
        {
            return Err(too_many());
        }
    }
    if let Some(child_id) = body.child_id {
        let caller = Caller::Guardian { id: guardian.id, family_id: guardian.family_id };
        child_for(&state.pool, &caller, child_id).await?;
    }
    let code = body.code.trim().to_uppercase();

    let mut tx = state.pool.begin().await?;
    let device_id: Option<Uuid> = sqlx::query_scalar(
        "update device_claims set guardian_id = $2, child_id = $3
         where code = $1 and expires_at > now() returning device_id",
    )
    .bind(&code)
    .bind(guardian.id)
    .bind(body.child_id)
    .fetch_optional(&mut *tx)
    .await?;
    let Some(device_id) = device_id else {
        let mut attempts = state.claim_attempts.lock().expect("lock");
        let entry = attempts.entry(guardian.id).or_insert((0, std::time::Instant::now()));
        if entry.1.elapsed().as_secs() >= 3600 {
            *entry = (0, std::time::Instant::now());
        }
        entry.0 += 1;
        return Err(ApiError::NotFound("a device showing that code"));
    };
    let claimed = complete_if_ready(&mut tx, device_id).await?;
    tx.commit().await?;
    Ok(Json(
        json!({ "device_id": device_id, "status": if claimed { "claimed" } else { "waiting_for_device" } }),
    ))
}

/// Links the device once both halves are in: a guardian typed its code and
/// the device signed its challenge.
async fn complete_if_ready(tx: &mut Transaction<'_, Postgres>, device_id: Uuid) -> Result<bool, sqlx::Error> {
    let ready: Option<(Uuid, Option<Uuid>, Uuid)> = sqlx::query_as(
        "select device_claims.guardian_id, device_claims.child_id, guardians.family_id
         from device_claims join guardians on guardians.id = device_claims.guardian_id
         where device_claims.device_id = $1 and device_claims.proved_at is not null
           and device_claims.expires_at > now() for update of device_claims",
    )
    .bind(device_id)
    .fetch_optional(&mut **tx)
    .await?;
    let Some((guardian_id, child_id, family_id)) = ready else { return Ok(false) };

    let (model, firmware_version): (String, Option<String>) = sqlx::query_as(
        "update devices set family_id = $2, child_id = $3, claimed_at = now() where id = $1
         returning model, firmware_version",
    )
    .bind(device_id)
    .bind(family_id)
    .bind(child_id)
    .fetch_one(&mut **tx)
    .await?;
    sqlx::query("delete from device_claims where device_id = $1").bind(device_id).execute(&mut **tx).await?;

    let mut data = json!({
        "device_id": device_id,
        "family_id": family_id,
        "claimed_by": guardian_id,
        "hardware_revision": model,
    });
    if let Some(version) = firmware_version {
        data["firmware_version"] = json!(version);
    }
    events::publish(&mut **tx, events::DEVICE_CLAIMED, device_id, data).await?;
    Ok(true)
}

async fn device_for(state: &AppState, caller: &Caller, device_id: Uuid) -> ApiResult<()> {
    let family_id: Option<Option<Uuid>> = sqlx::query_scalar("select family_id from devices where id = $1")
        .bind(device_id)
        .fetch_optional(&state.pool)
        .await?;
    match (caller, family_id) {
        (Caller::Operator, Some(_)) => Ok(()),
        (Caller::Guardian { .. }, Some(Some(family_id))) if caller.may_access(family_id) => Ok(()),
        _ => Err(ApiError::NotFound("device")),
    }
}

#[derive(Deserialize)]
pub struct AssignInput {
    /// The child who uses the device, or null for nobody.
    child_id: Option<Uuid>,
}

pub async fn assign(
    caller: Caller,
    State(state): State<AppState>,
    Path(device_id): Path<Uuid>,
    Input(body): Input<AssignInput>,
) -> ApiResult<StatusCode> {
    device_for(&state, &caller, device_id).await?;
    if let Some(child_id) = body.child_id {
        let child = child_for(&state.pool, &caller, child_id).await?;
        // The child must be in the family the device belongs to.
        let same: bool = sqlx::query_scalar("select family_id = $2 from devices where id = $1")
            .bind(device_id)
            .bind(child.family_id)
            .fetch_one(&state.pool)
            .await
            .map(|same: Option<bool>| same.unwrap_or(false))?;
        if !same {
            return Err(ApiError::NotFound("child"));
        }
    }
    sqlx::query("update devices set child_id = $2 where id = $1")
        .bind(device_id)
        .bind(body.child_id)
        .execute(&state.pool)
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

/// Unlinks the device from its family. It shows a new code the next time it checks in.
pub async fn release(
    caller: Caller,
    State(state): State<AppState>,
    Path(device_id): Path<Uuid>,
) -> ApiResult<StatusCode> {
    device_for(&state, &caller, device_id).await?;
    sqlx::query("update devices set family_id = null, child_id = null, claimed_at = null where id = $1")
        .bind(device_id)
        .execute(&state.pool)
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

// --- the device itself ------------------------------------------------------------------

/// The newest firmware for a model, if it is newer than what the device runs.
/// A version that cannot be read counts as the oldest, and a device is never
/// offered a version older than its own.
async fn newer_firmware(
    state: &AppState,
    model: &str,
    current: &str,
) -> Result<Option<(String, String)>, sqlx::Error> {
    let latest: Option<(String, String, i32, i32, i32)> = sqlx::query_as(
        "select version, url, major, minor, patch from firmware where model = $1
         order by major desc, minor desc, patch desc limit 1",
    )
    .bind(model)
    .fetch_optional(&state.pool)
    .await?;
    let current = semver::Version::parse(current).map_or((0, 0, 0), |v| (v.major, v.minor, v.patch));
    Ok(latest
        .filter(|(_, _, major, minor, patch)| (*major as u64, *minor as u64, *patch as u64) > current)
        .map(|(version, url, ..)| (version, url)))
}

fn serial_of(headers: &HeaderMap) -> Option<&str> {
    headers
        .get("Serial-Number")
        .and_then(|value| value.to_str().ok())
        .map(str::trim)
        .filter(|s| !s.is_empty())
}

/// The device's regular check-in: the time, whether there is newer firmware,
/// and, while it is not linked to a family, the code to show.
pub async fn check_in(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Option<Json<Value>>,
) -> ApiResult<Json<Value>> {
    let mut answer = json!({
        "server_time": {
            "timestamp": Utc::now().timestamp_millis(),
            "timezone_offset": state.config.timezone_offset_minutes,
        }
    });
    // A device that is not one of ours still gets the time, and nothing else.
    let Some(serial) = serial_of(&headers) else { return Ok(Json(answer)) };
    let current = body
        .as_ref()
        .and_then(|Json(body)| body["application"]["version"].as_str())
        .map(|version| version.trim().chars().take(40).collect::<String>())
        .unwrap_or_default();
    let device: Option<(Uuid, String, Option<chrono::DateTime<Utc>>)> = sqlx::query_as(
        "update devices set last_seen_at = now(), firmware_version = coalesce(nullif($2, ''), firmware_version)
         where serial = $1 returning id, model, claimed_at",
    )
    .bind(serial)
    .bind(&current)
    .fetch_optional(&state.pool)
    .await?;
    let Some((device_id, model, claimed_at)) = device else { return Ok(Json(answer)) };

    answer["firmware"] = match newer_firmware(&state, &model, &current).await? {
        Some((version, url)) => json!({ "version": version, "url": url }),
        None => json!({ "version": current, "url": "" }),
    };

    if claimed_at.is_none() {
        // The code stays the same while it is valid, so the screen does not
        // change under the person typing it.
        let (code, challenge): (String, String) = sqlx::query_as(
            "insert into device_claims (device_id, code, challenge, expires_at)
             values ($1, $2, $3, now() + make_interval(mins => $4))
             on conflict (device_id) do update set
               code = case when device_claims.expires_at > now() then device_claims.code else excluded.code end,
               challenge = case when device_claims.expires_at > now() then device_claims.challenge else excluded.challenge end,
               proved_at = case when device_claims.expires_at > now() then device_claims.proved_at end,
               guardian_id = case when device_claims.expires_at > now() then device_claims.guardian_id end,
               child_id = case when device_claims.expires_at > now() then device_claims.child_id end,
               expires_at = case when device_claims.expires_at > now() then device_claims.expires_at else excluded.expires_at end
             returning code, challenge",
        )
        .bind(device_id)
        .bind(random_code())
        .bind(random_challenge())
        .bind(CLAIM_MINUTES)
        .fetch_one(&state.pool)
        .await?;
        answer["activation"] = json!({
            "code": code,
            "message": format!("capricorn\n{code}"),
            "challenge": challenge,
            "timeout_ms": 30_000,
        });
    }
    Ok(Json(answer))
}

#[derive(Deserialize)]
pub struct ActivateInput {
    serial_number: String,
    challenge: String,
    hmac: String,
}

/// The device signs its challenge. 200 once it is linked to a family, 202
/// while it is still waiting for a guardian to type the code.
pub async fn activate(
    State(state): State<AppState>,
    Input(body): Input<ActivateInput>,
) -> ApiResult<StatusCode> {
    let refused = || ApiError::Forbidden("the device could not be verified");
    let device: Option<(Uuid, Vec<u8>, Option<chrono::DateTime<Utc>>)> =
        sqlx::query_as("select id, hmac_key, claimed_at from devices where serial = $1")
            .bind(body.serial_number.trim())
            .fetch_optional(&state.pool)
            .await?;
    let Some((device_id, key, claimed_at)) = device else { return Err(refused()) };
    if !signed_by(&key, body.challenge.as_bytes(), &body.hmac) {
        return Err(refused());
    }
    if claimed_at.is_some() {
        return Ok(StatusCode::OK);
    }
    let mut tx = state.pool.begin().await?;
    let proved = sqlx::query(
        "update device_claims set proved_at = coalesce(proved_at, now())
         where device_id = $1 and challenge = $2 and expires_at > now()",
    )
    .bind(device_id)
    .bind(&body.challenge)
    .execute(&mut *tx)
    .await?;
    if proved.rows_affected() == 0 {
        // Signed correctly, but for a challenge that is no longer the current one.
        return Err(refused());
    }
    let claimed = complete_if_ready(&mut tx, device_id).await?;
    tx.commit().await?;
    Ok(if claimed { StatusCode::OK } else { StatusCode::ACCEPTED })
}

type SigningDevice = (Uuid, Vec<u8>, Option<Uuid>, Option<chrono::DateTime<Utc>>);

#[derive(Deserialize)]
pub struct DeviceSessionInput {
    serial_number: String,
    /// Seconds since 1970, from the device's clock (set at check-in).
    timestamp: i64,
    /// HMAC-SHA256 of `session:<serial>:<timestamp>` under the device's key, in hex.
    hmac: String,
}

/// A linked device asks for a session for its child. The request is signed
/// with the device's key and a fresh timestamp, and cannot be replayed.
pub async fn start_session(
    State(state): State<AppState>,
    Input(body): Input<DeviceSessionInput>,
) -> ApiResult<(StatusCode, Json<Value>)> {
    let refused = || ApiError::Forbidden("the device could not be verified");
    let serial = body.serial_number.trim();
    let device: Option<SigningDevice> =
        sqlx::query_as("select id, hmac_key, child_id, claimed_at from devices where serial = $1")
            .bind(serial)
            .fetch_optional(&state.pool)
            .await?;
    let Some((device_id, key, child_id, claimed_at)) = device else { return Err(refused()) };
    let message = format!("session:{serial}:{}", body.timestamp);
    if (Utc::now().timestamp() - body.timestamp).abs() > CLOCK_SKEW_SECONDS
        || !signed_by(&key, message.as_bytes(), &body.hmac)
    {
        return Err(refused());
    }
    // Each signed timestamp works once: a copy of this request is useless.
    let fresh = sqlx::query(
        "update devices set last_signed_at = $2, last_seen_at = now() where id = $1 and last_signed_at < $2",
    )
    .bind(device_id)
    .bind(body.timestamp)
    .execute(&state.pool)
    .await?;
    if fresh.rows_affected() == 0 {
        return Err(refused());
    }
    let (Some(_), Some(child_id)) = (claimed_at, child_id) else {
        return Err(ApiError::Conflict("the device is not linked to a child yet"));
    };
    let child = find_child(&state.pool, child_id).await?;
    let session = sessions::open_session(&state, Some(child), "child", None, Some(device_id)).await?;
    Ok((StatusCode::CREATED, Json(session)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_holder_of_the_key_can_sign() {
        // RFC 4231, test case 2.
        let (key, message) = (b"Jefe".as_slice(), b"what do ya want for nothing?".as_slice());
        let signature = "5bdcc146bf60754e6a042426089575c75a003f089d2739839dec58b964ec3843";

        assert!(signed_by(key, message, signature));
        assert!(signed_by(key, message, &signature.to_uppercase()));
        assert!(!signed_by(b"other", message, signature));
        assert!(!signed_by(key, b"another message", signature));
        assert!(!signed_by(key, message, "not hex"));
        assert!(!signed_by(key, message, ""));
    }

    #[test]
    fn a_code_is_easy_to_read_and_type() {
        let code = random_code();
        assert_eq!(code.len(), CODE_LENGTH);
        assert!(code.bytes().all(|c| CODE_ALPHABET.contains(&c)));
        assert!(!CODE_ALPHABET.iter().any(|c| b"01OI".contains(c)));
        assert_ne!(random_challenge(), random_challenge());
    }
}
