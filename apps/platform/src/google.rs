//! Sign-in with Google, as a second way in next to the emailed code.
//!
//! The app gets a signed ID token from Google and sends it here. It is only
//! believed if Google signed it, it was issued for this installation's own
//! client, it has not expired, and Google says the address is verified. The
//! address is then treated exactly like one that confirmed a code.

use std::time::{Duration, Instant};

use async_trait::async_trait;
use jsonwebtoken::jwk::JwkSet;
use jsonwebtoken::{Algorithm, DecodingKey, Validation, decode, decode_header};
use serde::Deserialize;
use tokio::sync::Mutex;

const KEYS_URL: &str = "https://www.googleapis.com/oauth2/v3/certs";
const ISSUERS: [&str; 2] = ["https://accounts.google.com", "accounts.google.com"];
const KEYS_FOR: Duration = Duration::from_secs(60 * 60);
const CLIENT_SUFFIX: &str = ".apps.googleusercontent.com";

/// Who a credential belongs to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Identity {
    pub email: String,
    pub name: Option<String>,
}

#[derive(Debug, thiserror::Error)]
pub enum IdentityError {
    /// The credential is not one to trust. Says why, for the log only.
    #[error("the credential was refused: {0}")]
    Refused(String),
    #[error("the identity provider could not be reached: {0}")]
    Unreachable(String),
}

#[async_trait]
pub trait IdentityProvider: Send + Sync {
    async fn identify(&self, credential: &str) -> Result<Identity, IdentityError>;
}

pub fn is_client_id(value: &str) -> bool {
    value.strip_suffix(CLIENT_SUFFIX).is_some_and(|id| {
        !id.is_empty() && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    })
}

#[derive(Debug, Deserialize)]
struct Claims {
    #[serde(default)]
    email: Option<String>,
    #[serde(default)]
    email_verified: Option<bool>,
    #[serde(default)]
    name: Option<String>,
}

/// What the token says about its holder, once its signature has been checked.
fn identity(claims: Claims) -> Result<Identity, IdentityError> {
    if claims.email_verified != Some(true) {
        return Err(IdentityError::Refused("Google has not verified the address".into()));
    }
    let email = claims.email.filter(|email| !email.is_empty());
    let email = email.ok_or_else(|| IdentityError::Refused("the token has no address".into()))?;
    let name = claims.name.map(|name| name.trim().to_string()).filter(|name| !name.is_empty());
    Ok(Identity { email, name })
}

pub struct Google {
    client_id: String,
    keys_url: String,
    http: reqwest::Client,
    keys: Mutex<Option<(Instant, JwkSet)>>,
}

impl Google {
    pub fn new(client_id: String) -> Self {
        Self::with_keys_url(client_id, KEYS_URL.to_string())
    }

    pub fn with_keys_url(client_id: String, keys_url: String) -> Self {
        let http = reqwest::Client::builder().timeout(Duration::from_secs(10)).build().unwrap_or_default();
        Self { client_id, keys_url, http, keys: Mutex::new(None) }
    }

    /// Google's signing key with this id. Keys are kept for an hour, and
    /// fetched again at once when a token names one that is not known yet:
    /// Google rotates them.
    async fn key(&self, id: &str) -> Result<DecodingKey, IdentityError> {
        let mut keys = self.keys.lock().await;
        let known = |set: &JwkSet| set.find(id).map(DecodingKey::from_jwk);
        if let Some((fetched, set)) = keys.as_ref()
            && fetched.elapsed() < KEYS_FOR
            && let Some(key) = known(set)
        {
            return key.map_err(|error| IdentityError::Unreachable(error.to_string()));
        }
        let unreachable = |error: reqwest::Error| IdentityError::Unreachable(error.to_string());
        let set: JwkSet = self
            .http
            .get(&self.keys_url)
            .send()
            .await
            .and_then(reqwest::Response::error_for_status)
            .map_err(unreachable)?
            .json()
            .await
            .map_err(unreachable)?;
        let key = known(&set);
        *keys = Some((Instant::now(), set));
        key.ok_or_else(|| IdentityError::Refused("signed with a key Google does not publish".into()))?
            .map_err(|error| IdentityError::Unreachable(error.to_string()))
    }
}

#[async_trait]
impl IdentityProvider for Google {
    async fn identify(&self, credential: &str) -> Result<Identity, IdentityError> {
        let refused = |error: jsonwebtoken::errors::Error| IdentityError::Refused(error.to_string());
        let header = decode_header(credential).map_err(refused)?;
        let key_id = header.kid.ok_or_else(|| IdentityError::Refused("the token names no key".into()))?;
        let key = self.key(&key_id).await?;

        // Only RS256, whatever the token's own header asks for.
        let mut validation = Validation::new(Algorithm::RS256);
        validation.set_audience(&[&self.client_id]);
        validation.set_issuer(&ISSUERS);
        validation.set_required_spec_claims(&["exp", "aud", "iss"]);
        identity(decode::<Claims>(credential, &key, &validation).map_err(refused)?.claims)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn claims(email: Option<&str>, verified: Option<bool>, name: Option<&str>) -> Claims {
        Claims { email: email.map(str::to_string), email_verified: verified, name: name.map(str::to_string) }
    }

    #[test]
    fn only_a_verified_address_is_an_identity() {
        assert_eq!(
            identity(claims(Some("ana@example.org"), Some(true), Some(" Ana "))).unwrap(),
            Identity { email: "ana@example.org".into(), name: Some("Ana".into()) }
        );
        assert_eq!(identity(claims(Some("ana@example.org"), Some(true), Some("  "))).unwrap().name, None);
        for refused in [
            claims(Some("ana@example.org"), Some(false), None),
            claims(Some("ana@example.org"), None, None),
            claims(None, Some(true), None),
            claims(Some(""), Some(true), None),
        ] {
            assert!(matches!(identity(refused), Err(IdentityError::Refused(_))));
        }
    }

    #[test]
    fn a_client_id_looks_like_one_from_google() {
        assert!(is_client_id("1020937711543-c97vi0s8gbjl7me47n1r1v160jv411lk.apps.googleusercontent.com"));
        for bad in
            ["", ".apps.googleusercontent.com", "abc", "a b.apps.googleusercontent.com", "GOCSPX-secret"]
        {
            assert!(!is_client_id(bad), "{bad}");
        }
    }

    #[tokio::test]
    async fn a_token_that_is_not_a_signed_token_is_refused_without_asking_google() {
        let google =
            Google::with_keys_url("x.apps.googleusercontent.com".into(), "http://127.0.0.1:1/".into());
        for credential in ["", "not a token", "a.b.c"] {
            assert!(
                matches!(google.identify(credential).await, Err(IdentityError::Refused(_))),
                "{credential}"
            );
        }
        // No key id: refused before any request, which would fail as unreachable here.
        let unsigned = "eyJhbGciOiJSUzI1NiJ9.eyJlbWFpbCI6ImFAYi5jIn0.c2ln";
        assert!(matches!(google.identify(unsigned).await, Err(IdentityError::Refused(_))));
        // A key id nobody has fetched: Google is asked, and here cannot be reached.
        let named = "eyJhbGciOiJSUzI1NiIsImtpZCI6ImsxIn0.eyJlbWFpbCI6ImFAYi5jIn0.c2ln";
        assert!(matches!(google.identify(named).await, Err(IdentityError::Unreachable(_))));
    }

    /// Against Google itself, with a real token: `gcloud auth print-identity-token`
    /// gives one issued for the gcloud client. Run with
    /// `GOOGLE_TEST_TOKEN=... GOOGLE_TEST_AUDIENCE=... cargo test -- --ignored`.
    #[tokio::test]
    #[ignore = "needs a real Google ID token"]
    async fn a_real_token_is_accepted_only_for_the_client_it_was_issued_for() {
        let token = std::env::var("GOOGLE_TEST_TOKEN").unwrap();
        let audience = std::env::var("GOOGLE_TEST_AUDIENCE").unwrap();
        let identity = Google::new(audience).identify(&token).await.unwrap();
        assert!(identity.email.contains('@'));

        let other = Google::new("someone-else.apps.googleusercontent.com".into());
        assert!(matches!(other.identify(&token).await, Err(IdentityError::Refused(_))));

        // One changed character in the signature is enough to refuse it.
        let last = if token.ends_with('A') { "B" } else { "A" };
        let forged = format!("{}{last}", &token[..token.len() - 1]);
        let same = Google::new(std::env::var("GOOGLE_TEST_AUDIENCE").unwrap());
        assert!(matches!(same.identify(&forged).await, Err(IdentityError::Refused(_))));
    }
}
