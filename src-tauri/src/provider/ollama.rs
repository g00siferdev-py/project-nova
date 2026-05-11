//! Ollama native `/api/chat` — local-first, streaming-friendly.

use std::time::Duration;

use async_trait::async_trait;
use futures_util::StreamExt;
use serde_json::{json, Value};

use super::engine::LLMProviderEngine;
use super::error::ProviderError;
use super::types::{
    CompletionRequest, CompletionResponse, ModelInfo, StreamChunk, ToolCall,
};
use crate::settings::SettingsManager;

/// Typical context for recent Ollama models (conservative default).
const DEFAULT_OLLAMA_CTX: u32 = 128_000;

/// `num_predict` upper bound — avoids absurd values from shared presets.
const OLLAMA_NUM_PREDICT_CAP: u32 = 131_072;

pub struct OllamaProvider {
    client: reqwest::Client,
    base_url: String,
    model: String,
    /// When set, sends `Authorization: Bearer …` (Ollama Cloud).
    bearer_token: Option<String>,
}

impl OllamaProvider {
    pub fn from_settings(settings: &SettingsManager, http: &reqwest::Client) -> Self {
        let base_url = settings.ollama_base_url().trim_end_matches('/').to_string();
        let model = settings.ollama_model();
        Self {
            client: http.clone(),
            base_url,
            model,
            bearer_token: None,
        }
    }

    /// Remote Ollama host at `https://ollama.com` with API key from encrypted settings (`ollama` slot).
    pub fn from_cloud_settings(settings: &SettingsManager, http: &reqwest::Client) -> Result<Self, ProviderError> {
        let token = settings
            .decrypt_api_key("ollama")?
            .filter(|s| !s.trim().is_empty())
            .ok_or(ProviderError::MissingApiKey("ollama"))?;
        let model = settings.ollama_model();
        Ok(Self {
            client: http.clone(),
            base_url: "https://ollama.com".to_string(),
            model,
            bearer_token: Some(token),
        })
    }

    fn chat_url(&self) -> String {
        format!("{}/api/chat", self.base_url)
    }

    fn authorized(&self, req: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
        match &self.bearer_token {
            Some(t) => req.header("Authorization", format!("Bearer {t}")),
            None => req,
        }
    }

    fn build_messages(request: &CompletionRequest) -> Vec<Value> {
        request
            .messages
            .iter()
            .map(|m| json!({"role": m.role, "content": m.content}))
            .collect()
    }

    fn build_options(request: &CompletionRequest) -> Value {
        let mut o = json!({});
        if let Some(t) = request.temperature {
            o.as_object_mut().unwrap().insert("temperature".into(), json!(t));
        }
        if let Some(mt) = request.max_tokens {
            let capped = mt.min(OLLAMA_NUM_PREDICT_CAP).max(1);
            o.as_object_mut()
                .unwrap()
                .insert("num_predict".into(), json!(capped));
        }
        o
    }
}

#[async_trait]
impl LLMProviderEngine for OllamaProvider {
    fn provider_id(&self) -> &'static str {
        if self.bearer_token.is_some() {
            "ollama_cloud"
        } else {
            "ollama"
        }
    }

    fn model_info(&self) -> ModelInfo {
        ModelInfo {
            provider_id: self.provider_id().to_string(),
            model_id: self.model.clone(),
            context_window_tokens: Some(DEFAULT_OLLAMA_CTX),
        }
    }

    async fn complete(&self, request: &CompletionRequest) -> Result<CompletionResponse, ProviderError> {
        let body = json!({
            "model": self.model,
            "messages": Self::build_messages(request),
            "stream": false,
            "options": Self::build_options(request),
        });
        let res = self
            .authorized(self.client.post(self.chat_url()).json(&body))
            .timeout(Duration::from_secs(300))
            .send()
            .await?
            .error_for_status()?;

        let v: Value = res.json().await?;
        if let Some(err) = v["error"].as_str() {
            return Err(ProviderError::Api(err.to_string()));
        }
        let content = v["message"]["content"]
            .as_str()
            .unwrap_or("")
            .to_string();

        let mut tool_calls = Vec::new();
        if let Some(tc) = v["message"]["tool_calls"].as_array() {
            for t in tc {
                let id = t["id"].as_str().unwrap_or("").to_string();
                let name = t["function"]["name"].as_str().unwrap_or("").to_string();
                let args = t["function"]["arguments"]
                    .as_str()
                    .map(String::from)
                    .unwrap_or_else(|| t["function"]["arguments"].to_string());
                tool_calls.push(ToolCall {
                    id,
                    name,
                    arguments_json: args,
                });
            }
        }

        Ok(CompletionResponse {
            content,
            tool_calls,
            finish_reason: v["done"].as_bool().and_then(|d| d.then_some("stop".to_string())),
            usage: None,
        })
    }

    async fn stream(
        &self,
        request: &CompletionRequest,
        tx: tokio::sync::mpsc::Sender<Result<StreamChunk, ProviderError>>,
    ) -> Result<(), ProviderError> {
        let body = json!({
            "model": self.model,
            "messages": Self::build_messages(request),
            "stream": true,
            "options": Self::build_options(request),
        });
        let res = self
            .authorized(self.client.post(self.chat_url()).json(&body))
            .timeout(Duration::from_secs(300))
            .send()
            .await?
            .error_for_status()?;

        let mut stream = res.bytes_stream();
        let mut line_buf = String::new();

        while let Some(chunk) = stream.next().await {
            let chunk = chunk.map_err(ProviderError::Http)?;
            line_buf.push_str(&String::from_utf8_lossy(&chunk));

            while let Some(pos) = line_buf.find('\n') {
                let line = line_buf[..pos].trim().to_string();
                line_buf = line_buf[pos + 1..].to_string();
                if line.is_empty() {
                    continue;
                }
                let v: Value = match serde_json::from_str(&line) {
                    Ok(v) => v,
                    Err(_) => continue,
                };
                if let Some(err) = v["error"].as_str() {
                    let _ = tx.send(Err(ProviderError::Api(err.to_string()))).await;
                    return Ok(());
                }
                let piece = v["message"]["content"].as_str().unwrap_or("");
                if !piece.is_empty() {
                    let _ = tx
                        .send(Ok(StreamChunk {
                            delta: piece.to_string(),
                            done: false,
                        }))
                        .await;
                }
                if v["done"].as_bool() == Some(true) {
                    let _ = tx
                        .send(Ok(StreamChunk {
                            delta: String::new(),
                            done: true,
                        }))
                        .await;
                    return Ok(());
                }
            }
        }

        let _ = tx
            .send(Ok(StreamChunk {
                delta: String::new(),
                done: true,
            }))
            .await;
        Ok(())
    }
}

/// Lists model names from Ollama Cloud (`GET https://ollama.com/api/tags`) using the encrypted `ollama` API key.
pub async fn fetch_ollama_cloud_model_tags(
    http: &reqwest::Client,
    settings: &SettingsManager,
) -> Result<Vec<String>, ProviderError> {
    let token = settings
        .decrypt_api_key("ollama")?
        .filter(|s| !s.trim().is_empty())
        .ok_or(ProviderError::MissingApiKey("ollama"))?;
    let res = http
        .get("https://ollama.com/api/tags")
        .header("Authorization", format!("Bearer {}", token.trim()))
        .timeout(Duration::from_secs(45))
        .send()
        .await?
        .error_for_status()?;
    let v: Value = res.json().await?;
    if let Some(msg) = v["error"].as_str() {
        return Err(ProviderError::Api(msg.to_string()));
    }
    let mut names = Vec::new();
    if let Some(models) = v["models"].as_array() {
        for m in models {
            if let Some(n) = m["name"].as_str() {
                names.push(n.to_string());
            }
        }
    }
    names.sort();
    names.dedup();
    Ok(names)
}

/// Lists tags from local Ollama `GET {base}/api/tags` (no API key).
pub async fn fetch_ollama_local_model_tags(
    http: &reqwest::Client,
    settings: &SettingsManager,
) -> Result<Vec<String>, ProviderError> {
    let base = settings.ollama_base_url();
    let base = base.trim_end_matches('/');
    let url = format!("{}/api/tags", base);
    let res = http
        .get(&url)
        .timeout(Duration::from_secs(45))
        .send()
        .await?
        .error_for_status()?;
    let v: Value = res.json().await?;
    if let Some(err) = v["error"].as_str() {
        return Err(ProviderError::Api(err.to_string()));
    }
    let mut names = Vec::new();
    if let Some(models) = v["models"].as_array() {
        for m in models {
            if let Some(n) = m["name"].as_str() {
                names.push(n.to_string());
            }
        }
    }
    names.sort();
    names.dedup();
    Ok(names)
}
