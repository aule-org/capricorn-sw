//! What every request handler and job can reach.

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};

use sqlx::PgPool;
use uuid::Uuid;

use crate::config::Config;
use crate::llm::{Anthropic, Llm};
use crate::mail::{Mailer, SmtpMailer};
use crate::memory::{Embedder, HttpEmbedder};

#[derive(Clone)]
pub struct AppState {
    pub pool: PgPool,
    pub config: Arc<Config>,
    /// The supervisor's model. Without one, plans come from the learner model
    /// alone and there is no guidance or review.
    pub llm: Option<Arc<dyn Llm>>,
    pub embedder: Option<Arc<dyn Embedder>>,
    /// Without one nobody can sign in and notices are only shown in the app.
    pub mailer: Option<Arc<dyn Mailer>>,
    /// Sessions the supervisor is reading right now, so a burst of turns
    /// starts one reading, not one per turn.
    pub watching: Arc<Mutex<HashSet<Uuid>>>,
    /// Wrong device codes typed by each guardian in the current hour.
    pub claim_attempts: Arc<Mutex<HashMap<Uuid, (u32, std::time::Instant)>>>,
}

impl AppState {
    pub fn new(pool: PgPool, config: Config) -> Self {
        let llm =
            config.supervisor.clone().map(|settings| Arc::new(Anthropic::new(settings)) as Arc<dyn Llm>);
        let embedder = config
            .embeddings
            .clone()
            .map(|settings| Arc::new(HttpEmbedder::new(settings)) as Arc<dyn Embedder>);
        let mailer = config.mail.clone().and_then(|settings| match SmtpMailer::new(&settings) {
            Ok(mailer) => Some(Arc::new(mailer) as Arc<dyn Mailer>),
            Err(error) => {
                tracing::error!(%error, "mail is off: the SMTP settings could not be used");
                None
            }
        });
        Self {
            pool,
            config: Arc::new(config),
            llm,
            embedder,
            mailer,
            watching: Arc::default(),
            claim_attempts: Arc::default(),
        }
    }
}
