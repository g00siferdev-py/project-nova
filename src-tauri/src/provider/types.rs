//! Shared request/response types for LLM backends (OpenAI-shaped + Ollama).

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// One turn in the chat transcript (OpenAI-style roles: system, user, assistant, tool).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatTurn {
    pub role: String,
    pub content: String,
    /// When set, OpenAI `/v1/chat/completions` uses this JSON object as the message (must include `role`). Plain `content` is ignored for that turn.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub openai_message: Option<Value>,
    /// When set, Ollama `/api/chat` uses this object instead of `{ role, content }` for that turn.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ollama_message: Option<Value>,
    /// When set, Anthropic Messages API uses this object as one `messages[]` entry (must include `role` and `content` per API).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub anthropic_message: Option<Value>,
}

impl ChatTurn {
    #[must_use]
    pub fn text(role: impl Into<String>, content: impl Into<String>) -> Self {
        Self {
            role: role.into(),
            content: content.into(),
            openai_message: None,
            ollama_message: None,
            anthropic_message: None,
        }
    }
}

/// OpenAI-style tool definition (JSON Schema parameters).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolDefinition {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub parameters: serde_json::Value,
}

/// A tool invocation returned by the model.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolCall {
    pub id: String,
    pub name: String,
    /// Raw JSON arguments string from the API.
    pub arguments_json: String,
}

#[derive(Debug, Clone, Default)]
pub struct CompletionRequest {
    pub messages: Vec<ChatTurn>,
    pub tools: Option<Vec<ToolDefinition>>,
    pub max_tokens: Option<u32>,
    pub temperature: Option<f32>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TokenUsage {
    pub prompt_tokens: Option<u32>,
    pub completion_tokens: Option<u32>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CompletionResponse {
    pub content: String,
    pub tool_calls: Vec<ToolCall>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub finish_reason: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub usage: Option<TokenUsage>,
}

/// Static metadata for settings UI and context budgeting.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelInfo {
    pub provider_id: String,
    pub model_id: String,
    /// Approximate context window when known (tokens).
    pub context_window_tokens: Option<u32>,
}

/// One chunk when [`LLMProviderEngine::stream`](super::LLMProviderEngine::stream) is used.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StreamChunk {
    pub delta: String,
    pub done: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderDescriptor {
    pub id: String,
    pub label: String,
    pub local_first: bool,
    pub requires_api_key: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatSendResult {
    pub reply: String,
    pub tool_calls: Vec<ToolCall>,
    pub provider_id: String,
    pub model_id: String,
}
