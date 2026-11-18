//! What the tutor remembers about a child between sessions.
//!
//! A memory is a short statement derived from a session ("multiplies by
//! counting on fingers", "loves football examples"), never the transcript. It
//! is stored with an embedding so the supervisor can ask for the memories
//! closest to what the child is saying now.
//!
//! The embedding model is whatever speaks the OpenAI embeddings API at the
//! configured URL. Pointing it at a model you run yourself keeps a child's
//! memories from leaving your server. Without one, memories are still stored
//! and recalled by date.

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sqlx::{PgConnection, PgPool};
use uuid::Uuid;

use crate::config::Embeddings;
use crate::families::current_consent;

/// The size of `memories.embedding`. Changing the model to one of another size
/// needs a migration and re-embedding.
pub const DIMENSIONS: usize = 1024;
pub const KINDS: [&str; 5] = ["fact", "preference", "struggle", "success", "summary"];
pub const MAX_CONTENT_CHARS: usize = 600;
/// Cosine distance under which a new memory repeats one the child already has.
const SAME_MEMORY_DISTANCE: f64 = 0.08;

#[derive(Debug, thiserror::Error)]
pub enum MemoryError {
    #[error("the embedding model could not be reached: {0}")]
    Transport(String),
    #[error("the embedding model answered with status {0}")]
    Status(u16),
    #[error("the embedding model returned {0}")]
    Shape(String),
    #[error(transparent)]
    Database(#[from] sqlx::Error),
}

#[async_trait]
pub trait Embedder: Send + Sync {
    async fn embed(&self, texts: &[String]) -> Result<Vec<Vec<f32>>, MemoryError>;
}

pub struct HttpEmbedder {
    client: reqwest::Client,
    settings: Embeddings,
}

impl HttpEmbedder {
    pub fn new(settings: Embeddings) -> Self {
        Self { client: reqwest::Client::new(), settings }
    }
}

pub fn parse_embeddings(body: &Value, expected: usize) -> Result<Vec<Vec<f32>>, MemoryError> {
    let data = body["data"].as_array().ok_or_else(|| MemoryError::Shape("no data array".into()))?;
    if data.len() != expected {
        return Err(MemoryError::Shape(format!("{} embeddings for {expected} texts", data.len())));
    }
    data.iter()
        .map(|item| {
            let vector: Vec<f32> = serde_json::from_value(item["embedding"].clone())
                .map_err(|_| MemoryError::Shape("an embedding that is not a list of numbers".into()))?;
            if vector.len() != DIMENSIONS || vector.iter().any(|value| !value.is_finite()) {
                return Err(MemoryError::Shape(format!(
                    "an embedding of {} dimensions, not {DIMENSIONS}",
                    vector.len()
                )));
            }
            Ok(vector)
        })
        .collect()
}

#[async_trait]
impl Embedder for HttpEmbedder {
    async fn embed(&self, texts: &[String]) -> Result<Vec<Vec<f32>>, MemoryError> {
        if texts.is_empty() {
            return Ok(Vec::new());
        }
        let mut request = self
            .client
            .post(format!("{}/embeddings", self.settings.url))
            .timeout(std::time::Duration::from_secs(20))
            .json(&json!({ "model": self.settings.model, "input": texts }));
        if let Some(api_key) = &self.settings.api_key {
            request = request.bearer_auth(api_key);
        }
        let response =
            request.send().await.map_err(|error| MemoryError::Transport(error.without_url().to_string()))?;
        if !response.status().is_success() {
            return Err(MemoryError::Status(response.status().as_u16()));
        }
        let body: Value = response.json().await.map_err(|error| MemoryError::Transport(error.to_string()))?;
        parse_embeddings(&body, texts.len())
    }
}

/// pgvector's text form, bound as text and cast with `::vector`.
pub fn vector_literal(vector: &[f32]) -> String {
    let values: Vec<String> = vector.iter().map(f32::to_string).collect();
    format!("[{}]", values.join(","))
}

#[derive(Clone, Debug, Serialize, sqlx::FromRow)]
pub struct Memory {
    pub id: Uuid,
    pub kind: String,
    pub content: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct NewMemory {
    pub kind: String,
    pub content: String,
}

impl NewMemory {
    /// Trimmed and cut to size; `None` if nothing usable is left.
    pub fn cleaned(&self) -> Option<Self> {
        let content: String = self.content.trim().chars().take(MAX_CONTENT_CHARS).collect();
        (KINDS.contains(&self.kind.as_str()) && !content.is_empty())
            .then(|| Self { kind: self.kind.clone(), content })
    }
}

async fn storage_allowed(
    connection: &mut PgConnection,
    child_id: Uuid,
    session_id: Option<Uuid>,
) -> Result<bool, sqlx::Error> {
    if !current_consent(&mut *connection, child_id).await?["memory"] {
        return Ok(false);
    }
    match session_id {
        Some(session_id) => Ok(sqlx::query_scalar::<_, bool>(
            "select use_memory from sessions where id = $1 and child_id = $2",
        )
        .bind(session_id)
        .bind(child_id)
        .fetch_optional(connection)
        .await?
        .unwrap_or(false)),
        None => Ok(true),
    }
}

/// Stores the memories of a session and returns how many were new. A memory
/// that repeats one the child already has is skipped.
pub async fn remember(
    pool: &PgPool,
    embedder: Option<&dyn Embedder>,
    child_id: Uuid,
    session_id: Option<Uuid>,
    memories: &[NewMemory],
) -> Result<usize, MemoryError> {
    let memories: Vec<NewMemory> = memories.iter().filter_map(NewMemory::cleaned).collect();
    if memories.is_empty() {
        return Ok(0);
    }
    {
        let mut connection = pool.acquire().await?;
        if !storage_allowed(&mut connection, child_id, session_id).await? {
            return Ok(0);
        }
    }
    let embeddings = match embedder {
        Some(embedder) => {
            let texts: Vec<String> = memories.iter().map(|memory| memory.content.clone()).collect();
            Some(embedder.embed(&texts).await?)
        }
        None => None,
    };

    // Never hold the child lock while waiting for the embedding provider.
    // Recheck after it returns, serialized with consent changes.
    let mut tx = pool.begin().await?;
    let child = sqlx::query_scalar::<_, Uuid>("select id from children where id = $1 for update")
        .bind(child_id)
        .fetch_optional(&mut *tx)
        .await?;
    if child.is_none() || !storage_allowed(&mut tx, child_id, session_id).await? {
        return Ok(0);
    }
    let mut stored = 0;
    for (index, memory) in memories.iter().enumerate() {
        let vector = embeddings.as_ref().map(|all| vector_literal(&all[index]));
        if let Some(vector) = &vector {
            let nearest: Option<f64> = sqlx::query_scalar(
                "select min(embedding <=> $2::vector) from memories where child_id = $1 and embedding is not null",
            )
            .bind(child_id)
            .bind(vector)
            .fetch_one(&mut *tx)
            .await?;
            if nearest.is_some_and(|distance| distance < SAME_MEMORY_DISTANCE) {
                continue;
            }
        }
        sqlx::query(
            "insert into memories (id, child_id, session_id, kind, content, embedding)
             values ($1, $2, $3, $4, $5, $6::vector)",
        )
        .bind(Uuid::now_v7())
        .bind(child_id)
        .bind(session_id)
        .bind(&memory.kind)
        .bind(&memory.content)
        .bind(vector)
        .execute(&mut *tx)
        .await?;
        stored += 1;
    }
    tx.commit().await?;
    Ok(stored)
}

/// The memories closest in meaning to `query`, or the latest ones when there
/// is no embedding model or it fails: recall must not stop a session.
pub async fn recall(
    pool: &PgPool,
    embedder: Option<&dyn Embedder>,
    child_id: Uuid,
    query: &str,
    limit: i64,
) -> Result<Vec<Memory>, sqlx::Error> {
    if let (Some(embedder), false) = (embedder, query.trim().is_empty()) {
        match embedder.embed(&[query.to_string()]).await {
            Ok(vectors) => {
                return sqlx::query_as(
                    "select id, kind, content, created_at from memories
                     where child_id = $1 and embedding is not null
                     order by embedding <=> $2::vector limit $3",
                )
                .bind(child_id)
                .bind(vector_literal(&vectors[0]))
                .bind(limit)
                .fetch_all(pool)
                .await;
            }
            Err(error) => tracing::warn!(%error, "recall fell back to the latest memories"),
        }
    }
    latest(pool, child_id, limit).await
}

pub async fn latest(pool: &PgPool, child_id: Uuid, limit: i64) -> Result<Vec<Memory>, sqlx::Error> {
    sqlx::query_as(
        "select id, kind, content, created_at from memories
         where child_id = $1 order by created_at desc limit $2",
    )
    .bind(child_id)
    .bind(limit)
    .fetch_all(pool)
    .await
}

pub async fn forget(pool: &PgPool, child_id: Uuid, memory_id: Uuid) -> Result<bool, sqlx::Error> {
    let result = sqlx::query("delete from memories where id = $1 and child_id = $2")
        .bind(memory_id)
        .bind(child_id)
        .execute(pool)
        .await?;
    Ok(result.rows_affected() > 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_vector_is_written_the_way_pgvector_reads_it() {
        assert_eq!(vector_literal(&[0.5, -1.0, 0.0]), "[0.5,-1,0]");
    }

    #[test]
    fn embeddings_of_the_wrong_shape_are_refused() {
        let good: Vec<f32> = vec![0.1; DIMENSIONS];
        let body = json!({ "data": [{ "embedding": good }] });
        assert_eq!(parse_embeddings(&body, 1).unwrap().len(), 1);

        assert!(parse_embeddings(&body, 2).is_err());
        assert!(parse_embeddings(&json!({ "data": [{ "embedding": [0.1, 0.2] }] }), 1).is_err());
        assert!(parse_embeddings(&json!({ "data": [{ "embedding": "x" }] }), 1).is_err());
        assert!(parse_embeddings(&json!({}), 1).is_err());
    }

    #[test]
    fn a_memory_is_cleaned_before_it_is_stored() {
        let memory = NewMemory { kind: "fact".into(), content: format!("  {}  ", "a".repeat(700)) };
        assert_eq!(memory.cleaned().unwrap().content.chars().count(), MAX_CONTENT_CHARS);

        assert!(NewMemory { kind: "gossip".into(), content: "x".into() }.cleaned().is_none());
        assert!(NewMemory { kind: "fact".into(), content: "   ".into() }.cleaned().is_none());
    }
}
