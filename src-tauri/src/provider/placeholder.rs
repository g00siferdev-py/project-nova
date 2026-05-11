//! Offline placeholder when no remote model is configured.

use async_trait::async_trait;

use super::engine::LLMProviderEngine;
use super::error::ProviderError;
use super::types::{
    CompletionRequest, CompletionResponse, ModelInfo, StreamChunk,
};

/// Deterministic local fallback when no remote model is configured.
pub struct PlaceholderEngine {
    label: &'static str,
}

impl PlaceholderEngine {
    #[must_use]
    pub const fn new() -> Self {
        Self {
            label: "placeholder",
        }
    }
}

impl Default for PlaceholderEngine {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl LLMProviderEngine for PlaceholderEngine {
    fn provider_id(&self) -> &'static str {
        self.label
    }

    fn model_info(&self) -> ModelInfo {
        ModelInfo {
            provider_id: "placeholder".to_string(),
            model_id: "none".to_string(),
            context_window_tokens: None,
        }
    }

    async fn complete(&self, request: &CompletionRequest) -> Result<CompletionResponse, ProviderError> {
        let user = request
            .messages
            .iter()
            .rev()
            .find(|m| m.role == "user")
            .map(|m| m.content.as_str())
            .unwrap_or("");
        let snippet: String = user.chars().take(400).collect();
        Ok(CompletionResponse {
            content: format!(
                "[{0}] No live model — configure OpenAI or Ollama in preferences. You said: {snippet}",
                self.label
            ),
            tool_calls: vec![],
            finish_reason: Some("stop".into()),
            usage: None,
        })
    }

    async fn stream(
        &self,
        request: &CompletionRequest,
        tx: tokio::sync::mpsc::Sender<Result<StreamChunk, ProviderError>>,
    ) -> Result<(), ProviderError> {
        let full = self.complete(request).await?;
        let _ = tx
            .send(Ok(StreamChunk {
                delta: full.content.clone(),
                done: false,
            }))
            .await;
        let _ = tx
            .send(Ok(StreamChunk {
                delta: String::new(),
                done: true,
            }))
            .await;
        Ok(())
    }
}
