//! The language model behind the supervisor. Every call asks for one JSON
//! object that follows a schema, so the answer can be checked before it is used.

use std::time::Duration;

use async_trait::async_trait;
use serde_json::{Value, json};

use crate::config::Supervisor;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tier {
    /// Plans and reviews, outside the conversation.
    Deep,
    /// Guidance while a session is running.
    Live,
}

pub struct Request<'a> {
    pub tier: Tier,
    pub system: &'a str,
    pub user: &'a str,
    /// Name and JSON Schema of the object to return.
    pub answer: &'a str,
    pub schema: &'a Value,
    pub max_tokens: u32,
}

#[derive(Debug, thiserror::Error)]
pub enum LlmError {
    #[error("the language model could not be reached: {0}")]
    Transport(String),
    #[error("the language model answered with status {0}")]
    Status(u16),
    #[error("the language model did not return the requested object")]
    NoAnswer,
}

#[async_trait]
pub trait Llm: Send + Sync {
    async fn answer(&self, request: Request<'_>) -> Result<Value, LlmError>;
}

/// Claude through the Anthropic Messages API. The schema is passed as a tool
/// the model is required to call, which makes its input the structured answer.
pub struct Anthropic {
    client: reqwest::Client,
    settings: Supervisor,
    base_url: String,
}

impl Anthropic {
    pub fn new(settings: Supervisor) -> Self {
        Self { client: reqwest::Client::new(), settings, base_url: "https://api.anthropic.com".into() }
    }
}

pub fn anthropic_body(model: &str, request: &Request<'_>) -> Value {
    json!({
        "model": model,
        "max_tokens": request.max_tokens,
        "system": request.system,
        "messages": [{ "role": "user", "content": request.user }],
        "tools": [{
            "name": request.answer,
            "description": "Return the answer in this structure.",
            "input_schema": request.schema,
        }],
        "tool_choice": { "type": "tool", "name": request.answer },
    })
}

pub fn anthropic_answer(response: &Value, answer: &str) -> Option<Value> {
    response["content"]
        .as_array()?
        .iter()
        .find(|block| block["type"] == "tool_use" && block["name"] == answer)
        .map(|block| block["input"].clone())
        .filter(Value::is_object)
}

#[async_trait]
impl Llm for Anthropic {
    async fn answer(&self, request: Request<'_>) -> Result<Value, LlmError> {
        let (model, timeout) = match request.tier {
            Tier::Deep => (&self.settings.model, Duration::from_secs(60)),
            Tier::Live => (&self.settings.live_model, Duration::from_secs(8)),
        };
        let response = self
            .client
            .post(format!("{}/v1/messages", self.base_url))
            .header("x-api-key", &self.settings.api_key)
            .header("anthropic-version", "2023-06-01")
            .timeout(timeout)
            .json(&anthropic_body(model, &request))
            .send()
            .await
            // The error text of a transport failure never includes the body.
            .map_err(|error| LlmError::Transport(error.without_url().to_string()))?;
        if !response.status().is_success() {
            // The body can echo what was sent, so it is not logged.
            return Err(LlmError::Status(response.status().as_u16()));
        }
        let body: Value = response.json().await.map_err(|error| LlmError::Transport(error.to_string()))?;
        anthropic_answer(&body, request.answer).ok_or(LlmError::NoAnswer)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_request_forces_the_structured_answer() {
        let schema = json!({ "type": "object" });
        let request = Request {
            tier: Tier::Live,
            system: "be brief",
            user: "hello",
            answer: "guidance",
            schema: &schema,
            max_tokens: 200,
        };

        let body = anthropic_body("claude-haiku-5-5", &request);

        assert_eq!(body["tool_choice"], json!({ "type": "tool", "name": "guidance" }));
        assert_eq!(body["tools"][0]["input_schema"], schema);
        assert_eq!(body["messages"][0]["content"], "hello");
    }

    #[test]
    fn the_answer_is_the_input_of_the_forced_tool() {
        let response = json!({ "content": [
            { "type": "text", "text": "thinking aloud" },
            { "type": "tool_use", "name": "guidance", "input": { "text": "slow down" } },
        ]});

        assert_eq!(anthropic_answer(&response, "guidance"), Some(json!({ "text": "slow down" })));
        assert_eq!(anthropic_answer(&response, "other"), None);
        assert_eq!(anthropic_answer(&json!({ "content": [] }), "guidance"), None);
        assert_eq!(anthropic_answer(&json!({}), "guidance"), None);
    }
}
