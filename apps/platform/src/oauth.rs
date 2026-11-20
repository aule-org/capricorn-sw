//! Sign-in with GitHub, Meta (Facebook) or Apple, next to Google and the
//! emailed code.
//!
//! The page sends the person to the provider, and the provider sends them back
//! to the page with a one-time code. The platform trades that code, with this
//! installation's secret, for who they are. As with Google, only an address the
//! provider vouches for is believed, and it is then treated exactly like one
//! that confirmed a code.

use std::time::Duration;

use async_trait::async_trait;
use hmac::{Hmac, KeyInit, Mac};
use jsonwebtoken::jwk::JwkSet;
use jsonwebtoken::{Algorithm, DecodingKey, EncodingKey, Header, Validation, decode, decode_header};
use reqwest::Url;
use serde::Deserialize;
use serde_json::{Value, json};
use sha2::Sha256;

use crate::google::{Identity, IdentityError};

/// What turns a provider's one-time code into the person it was given to.
#[async_trait]
pub trait CodeProvider: Send + Sync {
    /// The public id the page names when it sends someone to the provider.
    fn client_id(&self) -> &str;
    async fn identify(&self, code: &str, redirect_uri: &str) -> Result<Identity, IdentityError>;
}

fn http() -> reqwest::Client {
    reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        // GitHub's API refuses requests that do not name themselves.
        .user_agent("aule-platform")
        .build()
        .unwrap_or_default()
}

fn unreachable(error: reqwest::Error) -> IdentityError {
    IdentityError::Unreachable(error.to_string())
}

fn name_from(name: Option<&str>) -> Option<String> {
    name.map(str::trim).filter(|name| !name.is_empty()).map(str::to_string)
}

fn url(base: &str, path: &str, query: &[(&str, &str)]) -> Result<Url, IdentityError> {
    Url::parse_with_params(&format!("{base}{path}"), query)
        .map_err(|error| IdentityError::Unreachable(error.to_string()))
}

/// A provider's answer to the code: an access token, or why there is none. A
/// code that is wrong, used or expired is refused, not "unreachable".
fn access_token(answer: &Value) -> Result<String, IdentityError> {
    if let Some(token) = answer["access_token"].as_str().filter(|token| !token.is_empty()) {
        return Ok(token.to_string());
    }
    let error = answer["error"].as_str().or(answer["error"]["message"].as_str()).unwrap_or("no access token");
    Err(IdentityError::Refused(error.to_string()))
}

// ─────────── GitHub ───────────

pub struct Github {
    client_id: String,
    client_secret: String,
    /// `https://github.com` and `https://api.github.com`, or a test server.
    site: String,
    api: String,
    http: reqwest::Client,
}

#[derive(Debug, Deserialize)]
pub struct GithubEmail {
    email: String,
    #[serde(default)]
    primary: bool,
    #[serde(default)]
    verified: bool,
}

/// The account's primary address, if GitHub has verified it. Another verified
/// address would do as well, but the primary is the one its owner chose.
pub fn github_address(emails: &[GithubEmail]) -> Result<String, IdentityError> {
    emails
        .iter()
        .find(|email| email.primary && email.verified && !email.email.is_empty())
        .map(|email| email.email.clone())
        .ok_or_else(|| IdentityError::Refused("GitHub has no verified primary address for this account".into()))
}

impl Github {
    pub fn new(client_id: String, client_secret: String) -> Self {
        Self::with_urls(client_id, client_secret, "https://github.com".into(), "https://api.github.com".into())
    }

    pub fn with_urls(client_id: String, client_secret: String, site: String, api: String) -> Self {
        Self { client_id, client_secret, site, api, http: http() }
    }

    async fn get(&self, path: &str, token: &str) -> Result<reqwest::Response, IdentityError> {
        self.http
            .get(format!("{}{path}", self.api))
            .bearer_auth(token)
            .header("Accept", "application/vnd.github+json")
            .send()
            .await
            .and_then(reqwest::Response::error_for_status)
            .map_err(unreachable)
    }
}

#[async_trait]
impl CodeProvider for Github {
    fn client_id(&self) -> &str {
        &self.client_id
    }

    async fn identify(&self, code: &str, redirect_uri: &str) -> Result<Identity, IdentityError> {
        let answer: Value = self
            .http
            .post(format!("{}/login/oauth/access_token", self.site))
            .header("Accept", "application/json")
            .form(&[
                ("client_id", self.client_id.as_str()),
                ("client_secret", self.client_secret.as_str()),
                ("code", code),
                ("redirect_uri", redirect_uri),
            ])
            .send()
            .await
            .and_then(reqwest::Response::error_for_status)
            .map_err(unreachable)?
            .json()
            .await
            .map_err(unreachable)?;
        let token = access_token(&answer)?;
        let emails: Vec<GithubEmail> = self.get("/user/emails", &token).await?.json().await.map_err(unreachable)?;
        let email = github_address(&emails)?;
        let user: Value = self.get("/user", &token).await?.json().await.map_err(unreachable)?;
        let name = name_from(user["name"].as_str()).or_else(|| name_from(user["login"].as_str()));
        Ok(Identity { email, name })
    }
}

// ─────────── Meta ───────────

const GRAPH_VERSION: &str = "v21.0";

pub struct Meta {
    app_id: String,
    app_secret: String,
    /// `https://graph.facebook.com`, or a test server.
    graph: String,
    http: reqwest::Client,
}

/// Facebook only gives an address the person confirmed; an account made with a
/// phone number has none, and then there is nobody to sign in.
pub fn meta_identity(me: &Value) -> Result<Identity, IdentityError> {
    let email = me["email"].as_str().map(str::trim).filter(|email| !email.is_empty());
    let email = email.ok_or_else(|| IdentityError::Refused("the Facebook account has no address".into()))?;
    Ok(Identity { email: email.to_string(), name: name_from(me["name"].as_str()) })
}

/// Proves to the Graph API that the call comes from the app's own server.
fn appsecret_proof(token: &str, secret: &str) -> String {
    let mut mac = Hmac::<Sha256>::new_from_slice(secret.as_bytes()).expect("HMAC accepts any key length");
    mac.update(token.as_bytes());
    hex::encode(mac.finalize().into_bytes())
}

impl Meta {
    pub fn new(app_id: String, app_secret: String) -> Self {
        Self::with_graph(app_id, app_secret, "https://graph.facebook.com".into())
    }

    pub fn with_graph(app_id: String, app_secret: String, graph: String) -> Self {
        Self { app_id, app_secret, graph, http: http() }
    }
}

#[async_trait]
impl CodeProvider for Meta {
    fn client_id(&self) -> &str {
        &self.app_id
    }

    async fn identify(&self, code: &str, redirect_uri: &str) -> Result<Identity, IdentityError> {
        let exchange = url(
            &self.graph,
            &format!("/{GRAPH_VERSION}/oauth/access_token"),
            &[
                ("client_id", &self.app_id),
                ("client_secret", &self.app_secret),
                ("redirect_uri", redirect_uri),
                ("code", code),
            ],
        )?;
        // A refused code comes back as a 400 with the reason in the body.
        let answer: Value =
            self.http.get(exchange).send().await.map_err(unreachable)?.json().await.map_err(unreachable)?;
        let token = access_token(&answer)?;
        let proof = appsecret_proof(&token, &self.app_secret);
        let me = url(
            &self.graph,
            &format!("/{GRAPH_VERSION}/me"),
            &[("fields", "name,email"), ("access_token", &token), ("appsecret_proof", &proof)],
        )?;
        let me: Value = self
            .http
            .get(me)
            .send()
            .await
            .and_then(reqwest::Response::error_for_status)
            .map_err(unreachable)?
            .json()
            .await
            .map_err(unreachable)?;
        meta_identity(&me)
    }
}

// ─────────── Apple ───────────

const APPLE: &str = "https://appleid.apple.com";
/// Apple accepts a client secret valid for up to six months; one is made for
/// each sign-in, valid for five minutes.
const SECRET_SECONDS: i64 = 300;

pub struct Apple {
    services_id: String,
    team_id: String,
    key_id: String,
    key: EncodingKey,
    /// `https://appleid.apple.com`, or a test server.
    site: String,
    http: reqwest::Client,
}

#[derive(Debug, Deserialize)]
struct AppleClaims {
    #[serde(default)]
    email: Option<String>,
    /// Apple has sent this both as a boolean and as the string "true".
    #[serde(default)]
    email_verified: Option<Value>,
}

fn apple_identity(claims: AppleClaims) -> Result<Identity, IdentityError> {
    let verified = matches!(&claims.email_verified, Some(Value::Bool(true)))
        || matches!(&claims.email_verified, Some(Value::String(text)) if text == "true");
    if !verified {
        return Err(IdentityError::Refused("Apple has not verified the address".into()));
    }
    let email = claims.email.filter(|email| !email.is_empty());
    let email = email.ok_or_else(|| IdentityError::Refused("the token has no address".into()))?;
    // Apple gives the name only to the page, the first time; the page passes it on.
    Ok(Identity { email, name: None })
}

/// A Services ID is a reverse domain name, like `com.example.signin`.
pub fn is_services_id(value: &str) -> bool {
    value.contains('.')
        && value.len() <= 120
        && value.chars().all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-')
}

/// A Team ID or a Key ID: ten letters and digits.
pub fn is_apple_id(value: &str) -> bool {
    value.len() == 10 && value.chars().all(|c| c.is_ascii_uppercase() || c.is_ascii_digit())
}

impl Apple {
    /// `key_pem` is the `.p8` file Apple gave for Sign in with Apple.
    pub fn new(services_id: String, team_id: String, key_id: String, key_pem: &[u8]) -> Result<Self, String> {
        Self::with_site(services_id, team_id, key_id, key_pem, APPLE.into())
    }

    pub fn with_site(
        services_id: String,
        team_id: String,
        key_id: String,
        key_pem: &[u8],
        site: String,
    ) -> Result<Self, String> {
        let key = EncodingKey::from_ec_pem(key_pem).map_err(|error| error.to_string())?;
        Ok(Self { services_id, team_id, key_id, key, site, http: http() })
    }

    /// The client secret Apple asks for: a token signed with the app's key.
    pub fn client_secret(&self, now: i64) -> Result<String, IdentityError> {
        let mut header = Header::new(Algorithm::ES256);
        header.kid = Some(self.key_id.clone());
        let claims = json!({
            "iss": self.team_id,
            "iat": now,
            "exp": now + SECRET_SECONDS,
            "aud": APPLE,
            "sub": self.services_id,
        });
        jsonwebtoken::encode(&header, &claims, &self.key)
            .map_err(|error| IdentityError::Unreachable(format!("the client secret could not be signed: {error}")))
    }

    async fn verified(&self, id_token: &str) -> Result<Identity, IdentityError> {
        let refused = |error: jsonwebtoken::errors::Error| IdentityError::Refused(error.to_string());
        let header = decode_header(id_token).map_err(refused)?;
        let key_id = header.kid.ok_or_else(|| IdentityError::Refused("the token names no key".into()))?;
        // Sign-ins are rare enough to fetch Apple's keys each time.
        let keys: JwkSet = self
            .http
            .get(format!("{}/auth/keys", self.site))
            .send()
            .await
            .and_then(reqwest::Response::error_for_status)
            .map_err(unreachable)?
            .json()
            .await
            .map_err(unreachable)?;
        let jwk = keys
            .find(&key_id)
            .ok_or_else(|| IdentityError::Refused("signed with a key Apple does not publish".into()))?;
        let key = DecodingKey::from_jwk(jwk).map_err(|error| IdentityError::Unreachable(error.to_string()))?;
        let mut validation = Validation::new(Algorithm::RS256);
        validation.set_audience(&[&self.services_id]);
        validation.set_issuer(&[APPLE]);
        validation.set_required_spec_claims(&["exp", "aud", "iss"]);
        apple_identity(decode::<AppleClaims>(id_token, &key, &validation).map_err(refused)?.claims)
    }
}

#[async_trait]
impl CodeProvider for Apple {
    fn client_id(&self) -> &str {
        &self.services_id
    }

    async fn identify(&self, code: &str, redirect_uri: &str) -> Result<Identity, IdentityError> {
        let secret = self.client_secret(chrono::Utc::now().timestamp())?;
        // A refused code comes back as a 400 with the reason in the body.
        let answer: Value = self
            .http
            .post(format!("{}/auth/token", self.site))
            .form(&[
                ("client_id", self.services_id.as_str()),
                ("client_secret", secret.as_str()),
                ("code", code),
                ("grant_type", "authorization_code"),
                ("redirect_uri", redirect_uri),
            ])
            .send()
            .await
            .map_err(unreachable)?
            .json()
            .await
            .map_err(unreachable)?;
        let id_token = answer["id_token"]
            .as_str()
            .ok_or_else(|| IdentityError::Refused(answer["error"].as_str().unwrap_or("no id token").to_string()))?;
        self.verified(id_token).await
    }
}

#[cfg(test)]
mod tests {
    use axum::routing::{get, post};
    use axum::{Json, Router};

    use super::*;

    fn email(address: &str, primary: bool, verified: bool) -> GithubEmail {
        GithubEmail { email: address.into(), primary, verified }
    }

    #[test]
    fn only_a_verified_primary_github_address_is_believed() {
        let emails = [email("old@example.org", false, true), email("ana@example.org", true, true)];
        assert_eq!(github_address(&emails).unwrap(), "ana@example.org");
        for refused in [
            vec![],
            vec![email("ana@example.org", true, false)],
            vec![email("ana@example.org", false, true)],
            vec![email("", true, true)],
        ] {
            assert!(matches!(github_address(&refused), Err(IdentityError::Refused(_))));
        }
    }

    #[test]
    fn a_facebook_account_without_an_address_is_nobody() {
        let ana = meta_identity(&json!({ "name": " Ana ", "email": "ana@example.org" })).unwrap();
        assert_eq!(ana, Identity { email: "ana@example.org".into(), name: Some("Ana".into()) });
        assert!(matches!(meta_identity(&json!({ "name": "Ana" })), Err(IdentityError::Refused(_))));
        assert!(matches!(meta_identity(&json!({ "email": "  " })), Err(IdentityError::Refused(_))));
    }

    #[test]
    fn apple_says_verified_as_a_boolean_or_as_text() {
        let claims = |verified: Option<Value>| AppleClaims { email: Some("a@privaterelay.appleid.com".into()), email_verified: verified };
        assert!(apple_identity(claims(Some(json!(true)))).is_ok());
        assert!(apple_identity(claims(Some(json!("true")))).is_ok());
        for refused in [None, Some(json!(false)), Some(json!("false")), Some(json!(1))] {
            assert!(matches!(apple_identity(claims(refused)), Err(IdentityError::Refused(_))));
        }
        let nobody = AppleClaims { email: None, email_verified: Some(json!(true)) };
        assert!(matches!(apple_identity(nobody), Err(IdentityError::Refused(_))));
    }

    #[test]
    fn apple_ids_look_like_apple_ids() {
        assert!(is_services_id("com.meetaule.signin"));
        assert!(!is_services_id("signin") && !is_services_id("com.meet aule"));
        assert!(is_apple_id("A1B2C3D4E5"));
        assert!(!is_apple_id("a1b2c3d4e5") && !is_apple_id("A1B2C3D4E") && !is_apple_id("A1B2C3D4E5F"));
    }

    // A P-256 key made for these tests only, signing nothing anywhere else
    // (openssl ecparam -genkey -name prime256v1, then pkcs8 -topk8).
    const TEST_KEY: &str = "-----BEGIN PRIVATE KEY-----
MIGHAgEAMBMGByqGSM49AgEGCCqGSM49AwEHBG0wawIBAQQgytQeNvmdbX97AvHL
TjFwZdGbVlGANL2mGKLdoCBHUlOhRANCAARiLwJDmF342Q+TcHlDe3RijSUT4U9f
MX+eAkPssY+ec+cjc0UVmS/g1Wv2U06qEbq0QEVRVq73UE9enc1FkI2a
-----END PRIVATE KEY-----
";
    const TEST_PUBLIC: &str = "-----BEGIN PUBLIC KEY-----
MFkwEwYHKoZIzj0CAQYIKoZIzj0DAQcDQgAEYi8CQ5hd+NkPk3B5Q3t0Yo0lE+FP
XzF/ngJD7LGPnnPnI3NFFZkv4NVr9lNOqhG6tEBFUVau91BPXp3NRZCNmg==
-----END PUBLIC KEY-----
";

    #[test]
    fn the_apple_client_secret_is_signed_with_the_key_and_names_the_app() {
        let apple =
            Apple::new("com.example.signin".into(), "TEAM123456".into(), "KEY1234567".into(), TEST_KEY.as_bytes())
                .unwrap();
        let secret = apple.client_secret(1_800_000_000).unwrap();
        assert_eq!(decode_header(&secret).unwrap().kid.as_deref(), Some("KEY1234567"));
        let mut validation = Validation::new(Algorithm::ES256);
        validation.set_audience(&[APPLE]);
        validation.validate_exp = false;
        let key = DecodingKey::from_ec_pem(TEST_PUBLIC.as_bytes()).unwrap();
        let claims = decode::<Value>(&secret, &key, &validation).unwrap().claims;
        assert_eq!(claims["iss"], "TEAM123456");
        assert_eq!(claims["sub"], "com.example.signin");
        assert_eq!(claims["exp"].as_i64().unwrap() - claims["iat"].as_i64().unwrap(), SECRET_SECONDS);
        assert!(Apple::new("a.b".into(), "T".into(), "K".into(), b"not a key").is_err());
    }

    async fn serve(router: Router) -> String {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
        format!("http://{address}")
    }

    #[tokio::test]
    async fn a_github_code_becomes_the_account_s_verified_address() {
        let site = Router::new()
            .route(
                "/login/oauth/access_token",
                post(|body: String| async move {
                    let answer = if body.contains("code=good") && body.contains("client_secret=s3cret") {
                        json!({ "access_token": "gho_test" })
                    } else {
                        // GitHub answers a bad code with 200 and an error.
                        json!({ "error": "bad_verification_code" })
                    };
                    Json(answer)
                }),
            )
            .route(
                "/user/emails",
                get(|| async { Json(json!([{ "email": "ana@example.org", "primary": true, "verified": true }])) }),
            )
            .route("/user", get(|| async { Json(json!({ "login": "ana", "name": null })) }));
        let base = serve(site).await;
        let github = Github::with_urls("id".into(), "s3cret".into(), base.clone(), base);
        let ana = github.identify("good", "https://example.org/callback").await.unwrap();
        assert_eq!(ana, Identity { email: "ana@example.org".into(), name: Some("ana".into()) });
        assert!(matches!(github.identify("bad", "https://example.org/callback").await, Err(IdentityError::Refused(_))));
    }

    #[tokio::test]
    async fn a_refused_facebook_code_is_refused_not_unreachable() {
        let graph = Router::new().route(
            &format!("/{GRAPH_VERSION}/oauth/access_token"),
            get(|| async {
                (
                    axum::http::StatusCode::BAD_REQUEST,
                    Json(json!({ "error": { "message": "This authorization code has been used." } })),
                )
            }),
        );
        let meta = Meta::with_graph("1".into(), "s".into(), serve(graph).await);
        assert!(matches!(meta.identify("used", "https://example.org/callback").await, Err(IdentityError::Refused(_))));
    }
}
