//! Multi-backend LLM layer: OpenAI, Ollama, placeholders; async completion + streaming.

mod anthropic;
mod engine;
mod error;
mod ollama;
mod openai;
mod placeholder;
mod types;

pub use anthropic::{fetch_anthropic_model_ids, AnthropicProvider};
pub use engine::LLMProviderEngine;
pub use error::ProviderError;
pub use ollama::{
    fetch_ollama_cloud_model_tags, fetch_ollama_local_model_tags, OllamaProvider,
};
pub use openai::{fetch_openai_model_ids, OpenAIProvider};
pub use placeholder::PlaceholderEngine;
pub use types::{
    ChatSendResult, ChatTurn, CompletionRequest, CompletionResponse, ModelInfo, ProviderDescriptor,
    StreamChunk, TokenUsage, ToolCall, ToolDefinition,
};

use std::sync::Arc;

use crate::settings::SettingsManager;

/// Static catalog for the settings UI (`provider_list_available`).
#[must_use]
pub fn list_provider_descriptors() -> Vec<ProviderDescriptor> {
    vec![
        ProviderDescriptor {
            id: "placeholder".into(),
            label: "Placeholder (offline)".into(),
            local_first: true,
            requires_api_key: false,
        },
        ProviderDescriptor {
            id: "openai".into(),
            label: "OpenAI".into(),
            local_first: false,
            requires_api_key: true,
        },
        ProviderDescriptor {
            id: "ollama".into(),
            label: "Ollama · Local — runs on your computer".into(),
            local_first: true,
            requires_api_key: false,
        },
        ProviderDescriptor {
            id: "ollama_cloud".into(),
            label: "Ollama · Cloud — models on ollama.com".into(),
            local_first: false,
            requires_api_key: true,
        },
        ProviderDescriptor {
            id: "anthropic".into(),
            label: "Anthropic (Claude)".into(),
            local_first: false,
            requires_api_key: true,
        },
    ]
}

/// Build the active engine from encrypted [`SettingsManager`] (and mirrored public prefs).
pub fn build_engine(
    http: &reqwest::Client,
    settings: &SettingsManager,
) -> Result<Arc<dyn LLMProviderEngine + Send + Sync>, ProviderError> {
    let active = settings.selected_provider();
    let engine: Arc<dyn LLMProviderEngine + Send + Sync> = match active.trim() {
        "openai" => Arc::new(OpenAIProvider::from_settings(settings, http)?),
        "ollama" => Arc::new(OllamaProvider::from_settings(settings, http)),
        "ollama_cloud" => Arc::new(OllamaProvider::from_cloud_settings(settings, http)?),
        "anthropic" => Arc::new(AnthropicProvider::from_settings(settings, http)?),
        _ => Arc::new(PlaceholderEngine::new()),
    };
    Ok(engine)
}
