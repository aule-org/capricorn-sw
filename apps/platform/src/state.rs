//! What every request handler and job can reach.

use std::collections::HashSet;
use std::sync::{Arc, Mutex};

use sqlx::PgPool;
use uuid::Uuid;

use crate::config::Config;
use crate::llm::{Anthropic, Llm};
use crate::memory::{Embedder, HttpEmbedder};

#[derive(Clone)]
pub struct AppState {
    pub pool: PgPool,
    pub config: Arc<Config>,
    /// The supervisor's model. Without one, plans come from the learner model
    /// alone and there is no guidance or review.
    pub llm: Option<Arc<dyn Llm>>,
    pub embedder: Option<Arc<dyn Embedder>>,
    /// Sessions the supervisor is reading right now, so a burst of turns
    /// starts one reading, not one per turn.
    pub watching: Arc<Mutex<HashSet<Uuid>>>,
}

impl AppState {
    pub fn new(pool: PgPool, config: Config) -> Self {
        let llm =
            config.supervisor.clone().map(|settings| Arc::new(Anthropic::new(settings)) as Arc<dyn Llm>);
        let embedder = config
            .embeddings
            .clone()
            .map(|settings| Arc::new(HttpEmbedder::new(settings)) as Arc<dyn Embedder>);
        Self { pool, config: Arc::new(config), llm, embedder, watching: Arc::default() }
    }
}
