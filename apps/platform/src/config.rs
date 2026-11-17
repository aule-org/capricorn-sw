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
        if let Some(embeddings) = &embeddings {
            if !embeddings.url.starts_with("http://") && !embeddings.url.starts_with("https://") {
                problems.push("CAPRICORN_EMBEDDINGS_URL must be an http:// or https:// URL".into());
            }
        }

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
    }

    #[test]
    fn every_problem_is_reported_at_once() {
        let env = [
            ("CAPRICORN_ADMIN_TOKEN", "short"),
            ("LIVEKIT_API_KEY", "key"),
            ("CAPRICORN_TRANSCRIPT_HOURS", "72"),
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
