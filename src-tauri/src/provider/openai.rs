//! OpenAI Chat Completions (`/v1/chat/completions`) with SSE streaming.

use std::time::Duration;

use async_trait::async_trait;
use futures_util::StreamExt;
use serde_json::{json, Value};

use super::engine::LLMProviderEngine;
use super::error::ProviderError;
use super::types::{
    CompletionRequest, CompletionResponse, ModelInfo, StreamChunk, TokenUsage, ToolCall,
};
use crate::settings::SettingsManager;

/// Default context sizes for UI / budgeting (approximate).
const GPT4O_MINI_CTX: u32 = 128_000;
const GPT4O_CTX: u32 = 128_000;

pub struct OpenAIProvider {
    client: reqwest::Client,
    api_key: String,
    model: String,
    base_url: String,
}

impl OpenAIProvider {
    pub fn from_settings(settings: &SettingsManager, http: &reqwest::Client) -> Result<Self, ProviderError> {
        let api_key = settings
            .decrypt_api_key("openai")?
            .filter(|s| !s.trim().is_empty())
            .ok_or(ProviderError::MissingApiKey("openai"))?;
        let model = settings.openai_model();
        let base_url = settings.openai_base_url().trim_end_matches('/').to_string();
        Ok(Self {
            client: http.clone(),
            api_key,
            model,
            base_url,
        })
    }

    fn context_for_model(model: &str) -> Option<u32> {
        let m = model.to_lowercase();
        if m.contains("gpt-4o") {
            Some(GPT4O_CTX)
        } else if m.contains("gpt-4") || m.contains("gpt-3.5") {
            Some(GPT4O_MINI_CTX)
        } else {
            Some(128_000)
        }
    }

    /// Chat Completions `max_tokens` is an output cap; never send values the API rejects.
    fn clamp_completion_tokens_for_model(model: &str, requested: u32) -> u32 {
        let m = model.to_lowercase();
        let cap = if m.contains("gpt-3.5") && !m.contains("16k") && !m.contains("1106") {
            4096u32
        } else if m.contains("gpt-4") || m.contains("gpt-3.5") || m.contains("gpt-4o") {
            16_384
        } else {
            16_384
        };
        requested.min(cap).max(1)
    }

    fn build_body(&self, request: &CompletionRequest, stream: bool) -> Value {
        let mut body = json!({
            "model": self.model,
            "messages": request.messages.iter().map(|m| json!({"role": m.role, "content": m.content})).collect::<Vec<_>>(),
            "stream": stream,
        });
        if let Some(t) = request.temperature {
            body.as_object_mut().unwrap().insert("temperature".into(), json!(t));
        }
        if let Some(mt) = request.max_tokens {
            let capped = Self::clamp_completion_tokens_for_model(&self.model, mt);
            body.as_object_mut()
                .unwrap()
                .insert("max_tokens".into(), json!(capped));
        }
        if let Some(ref tools) = request.tools {
            if !tools.is_empty() {
                let tjson: Vec<Value> = tools
                    .iter()
                    .map(|t| {
                        json!({
                            "type": "function",
                            "function": {
                                "name": t.name,
                                "description": t.description,
                                "parameters": t.parameters,
                            }
                        })
                    })
                    .collect();
                body.as_object_mut()
                    .unwrap()
                    .insert("tools".into(), json!(tjson));
            }
        }
        body
    }

    fn parse_message(v: &Value) -> Result<(String, Vec<ToolCall>, Option<String>), ProviderError> {
        let choice = &v["choices"][0];
        let msg = &choice["message"];
        let content = msg["content"].as_str().unwrap_or("").to_string();
        let finish = choice["finish_reason"].as_str().map(String::from);

        let mut tool_calls = Vec::new();
        if let Some(arr) = msg["tool_calls"].as_array() {
            for tc in arr {
                let id = tc["id"].as_str().unwrap_or("").to_string();
                let name = tc["function"]["name"].as_str().unwrap_or("").to_string();
                let args = tc["function"]["arguments"].as_str().unwrap_or("{}").to_string();
                tool_calls.push(ToolCall {
                    id,
                    name,
                    arguments_json: args,
                });
            }
        }
        Ok((content, tool_calls, finish))
    }

    fn parse_usage(v: &Value) -> Option<TokenUsage> {
        let u = &v["usage"];
        if u.is_null() {
            return None;
        }
        Some(TokenUsage {
            prompt_tokens: u["prompt_tokens"].as_u64().map(|x| x as u32),
            completion_tokens: u["completion_tokens"].as_u64().map(|x| x as u32),
        })
    }
}

#[async_trait]
impl LLMProviderEngine for OpenAIProvider {
    fn provider_id(&self) -> &'static str {
        "openai"
    }

    fn model_info(&self) -> ModelInfo {
        ModelInfo {
            provider_id: "openai".to_string(),
            model_id: self.model.clone(),
            context_window_tokens: Self::context_for_model(&self.model),
        }
    }

    async fn complete(&self, request: &CompletionRequest) -> Result<CompletionResponse, ProviderError> {
        let url = format!("{}/chat/completions", self.base_url);
        let body = self.build_body(request, false);
        let res = self
            .client
            .post(&url)
            .bearer_auth(&self.api_key)
            .json(&body)
            .timeout(Duration::from_secs(120))
            .send()
            .await?
            .error_for_status()?;

        let v: Value = res.json().await?;
        if let Some(err) = v["error"]["message"].as_str() {
            return Err(ProviderError::Api(err.to_string()));
        }
        let (content, tool_calls, finish_reason) = Self::parse_message(&v)?;
        Ok(CompletionResponse {
            content,
            tool_calls,
            finish_reason,
            usage: Self::parse_usage(&v),
        })
    }

    async fn stream(
        &self,
        request: &CompletionRequest,
        tx: tokio::sync::mpsc::Sender<Result<StreamChunk, ProviderError>>,
    ) -> Result<(), ProviderError> {
        let url = format!("{}/chat/completions", self.base_url);
        let body = self.build_body(request, true);
        let res = self
            .client
            .post(&url)
            .bearer_auth(&self.api_key)
            .json(&body)
            .timeout(Duration::from_secs(120))
            .send()
            .await?
            .error_for_status()?;

        let mut stream = res.bytes_stream();
        let mut line_buf = String::new();

        while let Some(chunk) = stream.next().await {
            let chunk = chunk.map_err(|e| ProviderError::Http(e))?;
            let s = String::from_utf8_lossy(&chunk);
            line_buf.push_str(&s);

            while let Some(pos) = line_buf.find('\n') {
                let line = line_buf[..pos].trim().to_string();
                line_buf = line_buf[pos + 1..].to_string();
                if line.is_empty() || line.starts_with(':') {
                    continue;
                }
                if let Some(data) = line.strip_prefix("data:").map(str::trim) {
                    if data == "[DONE]" {
                        let _ = tx
                            .send(Ok(StreamChunk {
                                delta: String::new(),
                                done: true,
                            }))
                            .await;
                        return Ok(());
                    }
                    let v: Value = match serde_json::from_str(data) {
                        Ok(v) => v,
                        Err(_) => continue,
                    };
                    if let Some(err) = v["error"]["message"].as_str() {
                        let _ = tx.send(Err(ProviderError::Api(err.to_string()))).await;
                        return Ok(());
                    }
                    let delta = v["choices"][0]["delta"]["content"]
                        .as_str()
                        .unwrap_or("")
                        .to_string();
                    if !delta.is_empty() {
                        let _ = tx
                            .send(Ok(StreamChunk {
                                delta,
                                done: false,
                            }))
                            .await;
                    }
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

/// Lists model ids from OpenAI-compatible `GET {base}/models` (requires saved OpenAI API key).
pub async fn fetch_openai_model_ids(
    http: &reqwest::Client,
    settings: &SettingsManager,
) -> Result<Vec<String>, ProviderError> {
    let api_key = settings
        .decrypt_api_key("openai")?
        .filter(|s| !s.trim().is_empty())
        .ok_or(ProviderError::MissingApiKey("openai"))?;
    let base = settings.openai_base_url();
    let base = base.trim_end_matches('/');
    let url = format!("{}/models", base);
    let res = http
        .get(&url)
        .bearer_auth(api_key.trim())
        .timeout(Duration::from_secs(45))
        .send()
        .await?
        .error_for_status()?;
    let v: Value = res.json().await?;
    if let Some(msg) = v["error"]["message"].as_str() {
        return Err(ProviderError::Api(msg.to_string()));
    }
    let mut names = Vec::new();
    if let Some(data) = v["data"].as_array() {
        for m in data {
            if let Some(id) = m["id"].as_str() {
                names.push(id.to_string());
            }
        }
    }
    names.sort();
    names.dedup();
    Ok(names)
}
