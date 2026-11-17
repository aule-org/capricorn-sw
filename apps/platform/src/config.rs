//! Settings, read from the environment once at start. Every problem is
//! reported together and the process does not start with one.

use std::collections::HashMap;
use std::net::SocketAddr;

#[derive(Clone, Debug)]
pub struct Config {
    pub database_url: String,
    pub addr: SocketAddr,
    /// Bearer token of the operator API. Stands in for family sign-in, which
    /// is not built yet.
    pub admin_token: String,
    pub livekit: Option<LiveKit>,
    pub agent_name: String,
    pub session_minutes: u32,
    /// Hours a transcript is kept when the guardian agreed to keep it.
    pub transcript_hours: u32,
    pub supervisor: Option<Supervisor>,
    pub embeddings: Option<Embeddings>,
    pub mail: Option<Mail>,
    pub events_webhook: Option<EventsWebhook>,
    /// Path of the Firebase service account key, for push notifications.
    pub fcm_service_account: Option<String>,
    /// Minutes east of UTC that devices are told, for their clock.
    pub timezone_offset_minutes: i32,
}

#[derive(Clone, Debug)]
pub struct Mail {
    /// For example `smtps://user:password@smtp.example.org:465`.
    pub smtp_url: String,
    pub from: String,
}

/// Where events are delivered outside the platform, signed with the secret.
#[derive(Clone, Debug)]
pub struct EventsWebhook {
    pub url: String,
    pub secret: String,
}

#[derive(Clone, Debug)]
pub struct LiveKit {
    /// URL the person's browser or device connects to.
    pub public_url: String,
    pub api_key: String,
    pub api_secret: String,
}

#[derive(Clone, Debug)]
pub struct Supervisor {
    pub api_key: String,
    /// Plans and reviews: runs outside the conversation, so it can think.
    pub model: String,
    /// Guidance during a session: has to keep up with the conversation.
    pub live_model: String,
}

#[derive(Clone, Debug)]
pub struct Embeddings {
    /// Base URL of an OpenAI-compatible embeddings API, e.g. a local Ollama.
    pub url: String,
    pub model: String,
    pub api_key: Option<String>,
}

#[derive(Debug, thiserror::Error)]
#[error("{0}")]
pub struct ConfigError(pub String);

pub const MIN_ADMIN_TOKEN_LENGTH: usize = 32;

impl Config {
    pub fn from_env() -> Result<Self, ConfigError> {
        Self::from_map(&std::env::vars().collect())
    }

    pub fn from_map(env: &HashMap<String, String>) -> Result<Self, ConfigError> {
        let get = |name: &str| env.get(name).map(|value| value.trim()).filter(|value| !value.is_empty());
        let mut problems: Vec<String> = Vec::new();

        let database_url = get("DATABASE_URL").unwrap_or_default().to_string();
        if !database_url.starts_with("postgres://") && !database_url.starts_with("postgresql://") {
            problems.push("DATABASE_URL must be a postgres:// URL".into());
        }

        let addr = get("CAPRICORN_PLATFORM_ADDR").unwrap_or("127.0.0.1:8091");
        let addr = addr.parse::<SocketAddr>().unwrap_or_else(|_| {
            problems.push("CAPRICORN_PLATFORM_ADDR must look like 127.0.0.1:8091".into());
            SocketAddr::from(([127, 0, 0, 1], 8091))
        });

        let admin_token = get("CAPRICORN_ADMIN_TOKEN").unwrap_or_default().to_string();
        if admin_token.len() < MIN_ADMIN_TOKEN_LENGTH {
            problems.push(format!(
                "CAPRICORN_ADMIN_TOKEN must have at least {MIN_ADMIN_TOKEN_LENGTH} characters"
            ));
        }

        let mut whole = |name: &str, default: u32, min: u32, max: u32| {
            let value = get(name).map_or(Some(default), |raw| raw.parse::<u32>().ok());
            match value {
                Some(value) if (min..=max).contains(&value) => value,
                _ => {
                    problems.push(format!("{name} must be a whole number from {min} to {max}"));
                    default
                }
            }
        };
        let session_minutes = whole("CAPRICORN_SESSION_MINUTES", 20, 1, 120);
        let transcript_hours = whole("CAPRICORN_TRANSCRIPT_HOURS", 24, 1, 24);

        let livekit = match (get("LIVEKIT_API_KEY"), get("LIVEKIT_API_SECRET")) {
            (Some(api_key), Some(api_secret)) => {
                let public_url =
                    get("CAPRICORN_LIVEKIT_PUBLIC_URL").or(get("LIVEKIT_URL")).unwrap_or_default();
                if !public_url.starts_with("ws://") && !public_url.starts_with("wss://") {
                    problems.push(
                        "CAPRICORN_LIVEKIT_PUBLIC_URL (or LIVEKIT_URL) must be a ws:// or wss:// URL".into(),
                    );
                }
                Some(LiveKit {
                    public_url: public_url.to_string(),
                    api_key: api_key.to_string(),
                    api_secret: api_secret.to_string(),
                })
            }
            (None, None) => None,
            _ => {
                problems.push("LIVEKIT_API_KEY and LIVEKIT_API_SECRET must be set together".into());
                None
            }
        };

        let supervisor = get("ANTHROPIC_API_KEY").map(|api_key| Supervisor {
            api_key: api_key.to_string(),
            model: get("CAPRICORN_SUPERVISOR_MODEL").unwrap_or("claude-sonnet-5-5").to_string(),
            live_model: get("CAPRICORN_SUPERVISOR_LIVE_MODEL").unwrap_or("claude-haiku-5-5").to_string(),
        });

        let embeddings = get("CAPRICORN_EMBEDDINGS_URL").map(|url| Embeddings {
            url: url.trim_end_matches('/').to_string(),
            model: get("CAPRICORN_EMBEDDINGS_MODEL").unwrap_or("bge-m3").to_string(),
            api_key: get("CAPRICORN_EMBEDDINGS_API_KEY").map(str::to_string),
        });
        if let Some(embeddings) = &embeddings
            && !embeddings.url.starts_with("http://")
            && !embeddings.url.starts_with("https://")
        {
            problems.push("CAPRICORN_EMBEDDINGS_URL must be an http:// or https:// URL".into());
        }

        let mail = match (get("CAPRICORN_SMTP_URL"), get("CAPRICORN_MAIL_FROM")) {
            (Some(smtp_url), Some(from)) => {
                if !smtp_url.starts_with("smtp://") && !smtp_url.starts_with("smtps://") {
                    problems.push("CAPRICORN_SMTP_URL must be an smtp:// or smtps:// URL".into());
                }
                if !from.contains('@') {
                    problems.push("CAPRICORN_MAIL_FROM must be an email address".into());
                }
                Some(Mail { smtp_url: smtp_url.to_string(), from: from.to_string() })
            }
            (None, None) => None,
            _ => {
                problems.push("CAPRICORN_SMTP_URL and CAPRICORN_MAIL_FROM must be set together".into());
                None
            }
        };

        let events_webhook =
            match (get("CAPRICORN_EVENTS_WEBHOOK_URL"), get("CAPRICORN_EVENTS_WEBHOOK_SECRET")) {
                (Some(url), Some(secret)) => {
                    if !url.starts_with("https://") && !url.starts_with("http://127.0.0.1") {
                        problems.push(
                            "CAPRICORN_EVENTS_WEBHOOK_URL must be https:// (or http://127.0.0.1)".into(),
                        );
                    }
                    if secret.len() < 32 {
                        problems
                            .push("CAPRICORN_EVENTS_WEBHOOK_SECRET must have at least 32 characters".into());
                    }
                    Some(EventsWebhook { url: url.to_string(), secret: secret.to_string() })
                }
                (None, None) => None,
                _ => {
                    problems.push(
                    "CAPRICORN_EVENTS_WEBHOOK_URL and CAPRICORN_EVENTS_WEBHOOK_SECRET must be set together"
                        .into(),
                );
                    None
                }
            };

        // Lima, where Capricorn is made, unless told otherwise.
        let timezone_offset_minutes = match get("CAPRICORN_TIMEZONE_OFFSET_MINUTES").map(str::parse::<i32>) {
            None => -300,
            Some(Ok(minutes)) if (-720..=840).contains(&minutes) => minutes,
            Some(_) => {
                problems
                    .push("CAPRICORN_TIMEZONE_OFFSET_MINUTES must be a whole number from -720 to 840".into());
                -300
            }
        };

        if !problems.is_empty() {
            return Err(ConfigError(problems.join("; ")));
        }
        Ok(Self {
            database_url,
            addr,
            admin_token,
            livekit,
            agent_name: get("CAPRICORN_AGENT_NAME").unwrap_or("capricorn-tutor").to_string(),
            session_minutes,
            transcript_hours,
            supervisor,
            embeddings,
            mail,
            events_webhook,
            fcm_service_account: get("CAPRICORN_FCM_SERVICE_ACCOUNT").map(str::to_string),
            timezone_offset_minutes,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    pub fn complete() -> HashMap<String, String> {
        [
            ("DATABASE_URL", "postgres://capricorn@localhost/capricorn"),
            ("CAPRICORN_ADMIN_TOKEN", "0123456789abcdef0123456789abcdef"),
        ]
        .into_iter()
        .map(|(name, value)| (name.to_string(), value.to_string()))
        .collect()
    }

    #[test]
    fn the_minimum_is_a_database_and_an_admin_token() {
        let config = Config::from_map(&complete()).unwrap();

        assert_eq!(config.addr.to_string(), "127.0.0.1:8091");
        assert_eq!(config.agent_name, "capricorn-tutor");
        assert_eq!((config.session_minutes, config.transcript_hours), (20, 24));
        assert!(config.livekit.is_none() && config.supervisor.is_none() && config.embeddings.is_none());
        assert!(config.mail.is_none() && config.events_webhook.is_none());
        assert_eq!(config.timezone_offset_minutes, -300);
    }

    #[test]
    fn every_problem_is_reported_at_once() {
        let env = [
            ("CAPRICORN_ADMIN_TOKEN", "short"),
            ("LIVEKIT_API_KEY", "key"),
            ("CAPRICORN_TRANSCRIPT_HOURS", "72"),
            ("CAPRICORN_SMTP_URL", "smtps://mail.example.org"),
            ("CAPRICORN_EVENTS_WEBHOOK_URL", "http://example.org/hook"),
            ("CAPRICORN_EVENTS_WEBHOOK_SECRET", "short"),
        ]
        .into_iter()
        .map(|(name, value)| (name.to_string(), value.to_string()))
        .collect();

        let message = Config::from_map(&env).unwrap_err().to_string();

        for expected in [
            "DATABASE_URL",
            "CAPRICORN_ADMIN_TOKEN must have at least 32",
            "must be set together",
            "CAPRICORN_TRANSCRIPT_HOURS must be a whole number from 1 to 24",
            "CAPRICORN_SMTP_URL and CAPRICORN_MAIL_FROM must be set together",
            "CAPRICORN_EVENTS_WEBHOOK_URL must be https://",
            "CAPRICORN_EVENTS_WEBHOOK_SECRET must have at least 32",
        ] {
            assert!(message.contains(expected), "{expected} not in {message}");
        }
    }

    #[test]
    fn optional_parts_turn_on_with_their_settings() {
        let mut env = complete();
        for (name, value) in [
            ("LIVEKIT_URL", "ws://127.0.0.1:7880"),
            ("CAPRICORN_LIVEKIT_PUBLIC_URL", "wss://live.example.org"),
            ("LIVEKIT_API_KEY", "key"),
            ("LIVEKIT_API_SECRET", "secret"),
            ("ANTHROPIC_API_KEY", "sk-test"),
            ("CAPRICORN_EMBEDDINGS_URL", "http://127.0.0.1:11434/v1/"),
        ] {
            env.insert(name.to_string(), value.to_string());
        }

        let config = Config::from_map(&env).unwrap();

        assert_eq!(config.livekit.unwrap().public_url, "wss://live.example.org");
        assert_eq!(config.supervisor.unwrap().live_model, "claude-haiku-5-5");
        let embeddings = config.embeddings.unwrap();
        assert_eq!(
            (embeddings.url.as_str(), embeddings.model.as_str()),
            ("http://127.0.0.1:11434/v1", "bge-m3")
        );
    }
}
