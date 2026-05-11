//! Anthropic Messages API (`/v1/messages`) with SSE streaming and tool support.

use std::time::Duration;

use async_trait::async_trait;
use futures_util::StreamExt;
use serde_json::{json, Value};

use super::engine::LLMProviderEngine;
use super::error::ProviderError;
use super::types::{
    CompletionRequest, CompletionResponse, ModelInfo, StreamChunk, TokenUsage, ToolCall,
    ToolDefinition,
};
use crate::settings::SettingsManager;

const ANTHROPIC_API: &str = "https://api.anthropic.com/v1/messages";
const ANTHROPIC_VERSION: &str = "2023-06-01";
const DEFAULT_MAX_TOKENS: u32 = 8192;
const CLAUDE_CTX: u32 = 200_000;

pub struct AnthropicProvider {
    client: reqwest::Client,
    api_key: String,
    model: String,
}

impl AnthropicProvider {
    pub fn from_settings(settings: &SettingsManager, http: &reqwest::Client) -> Result<Self, ProviderError> {
        let api_key = settings
            .decrypt_api_key("anthropic")?
            .filter(|s| !s.trim().is_empty())
            .ok_or(ProviderError::MissingApiKey("anthropic"))?;
        let model = settings.anthropic_model();
        Ok(Self {
            client: http.clone(),
            api_key,
            model,
        })
    }

    fn max_out_tokens(request: &CompletionRequest) -> u32 {
        request
            .max_tokens
            .unwrap_or(DEFAULT_MAX_TOKENS)
            .clamp(1, 128_000)
    }

    /// Split OpenAI-shaped transcript into Anthropic `system` + `messages` (no system role in messages).
    fn split_system_and_messages(request: &CompletionRequest) -> (Option<String>, Vec<Value>) {
        let mut system_parts: Vec<String> = Vec::new();
        let mut messages: Vec<Value> = Vec::new();
        for m in &request.messages {
            let role = m.role.to_lowercase();
            if role == "system" {
                if !m.content.trim().is_empty() {
                    system_parts.push(m.content.clone());
                }
                continue;
            }
            if role != "user" && role != "assistant" {
                continue;
            }
            messages.push(json!({ "role": role, "content": m.content }));
        }
        let system = if system_parts.is_empty() {
            None
        } else {
            Some(system_parts.join("\n\n"))
        };
        (system, messages)
    }

    fn anthropic_tools(tools: &[ToolDefinition]) -> Vec<Value> {
        tools
            .iter()
            .map(|t| {
                json!({
                    "name": t.name,
                    "description": t.description,
                    "input_schema": t.parameters,
                })
            })
            .collect()
    }

    fn build_body(&self, request: &CompletionRequest, stream: bool) -> Value {
        let (system, messages) = Self::split_system_and_messages(request);
        let max_tokens = Self::max_out_tokens(request);
        let mut body = json!({
            "model": self.model,
            "max_tokens": max_tokens,
            "messages": messages,
            "stream": stream,
        });
        let obj = body.as_object_mut().unwrap();
        if let Some(s) = system {
            if !s.trim().is_empty() {
                obj.insert("system".into(), json!(s));
            }
        }
        if let Some(t) = request.temperature {
            obj.insert("temperature".into(), json!(t));
        }
        if let Some(ref tools) = request.tools {
            if !tools.is_empty() {
                obj.insert("tools".into(), json!(Self::anthropic_tools(tools)));
            }
        }
        body
    }

    fn parse_content_blocks(content: &Value) -> Result<(String, Vec<ToolCall>), ProviderError> {
        let mut text_out = String::new();
        let mut tool_calls = Vec::new();
        let Some(arr) = content.as_array() else {
            return Ok((String::new(), tool_calls));
        };
        for block in arr {
            let ty = block["type"].as_str().unwrap_or("");
            if ty == "text" {
                let t = block["text"].as_str().unwrap_or("");
                if !t.is_empty() {
                    if !text_out.is_empty() {
                        text_out.push('\n');
                    }
                    text_out.push_str(t);
                }
            } else if ty == "tool_use" {
                let id = block["id"].as_str().unwrap_or("").to_string();
                let name = block["name"].as_str().unwrap_or("").to_string();
                let input = block["input"].clone();
                let arguments_json = serde_json::to_string(&input).unwrap_or_else(|_| "{}".to_string());
                tool_calls.push(ToolCall {
                    id,
                    name,
                    arguments_json,
                });
            }
        }
        Ok((text_out, tool_calls))
    }

    fn parse_message_response(v: &Value) -> Result<CompletionResponse, ProviderError> {
        if let Some(err) = v["error"]["message"].as_str() {
            return Err(ProviderError::Api(err.to_string()));
        }
        let (content, tool_calls) = Self::parse_content_blocks(&v["content"])?;
        let stop = v["stop_reason"].as_str().map(String::from);
        let usage = v["usage"].as_object().and_then(|u| {
            Some(TokenUsage {
                prompt_tokens: u["input_tokens"].as_u64().map(|x| x as u32),
                completion_tokens: u["output_tokens"].as_u64().map(|x| x as u32),
            })
        });
        Ok(CompletionResponse {
            content,
            tool_calls,
            finish_reason: stop,
            usage,
        })
    }

    fn apply_auth(&self, req: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
        req.header("x-api-key", &self.api_key)
            .header("anthropic-version", ANTHROPIC_VERSION)
    }
}

#[async_trait]
impl LLMProviderEngine for AnthropicProvider {
    fn provider_id(&self) -> &'static str {
        "anthropic"
    }

    fn model_info(&self) -> ModelInfo {
        ModelInfo {
            provider_id: "anthropic".to_string(),
            model_id: self.model.clone(),
            context_window_tokens: Some(CLAUDE_CTX),
        }
    }

    async fn complete(&self, request: &CompletionRequest) -> Result<CompletionResponse, ProviderError> {
        let body = self.build_body(request, false);
        let res = self
            .apply_auth(self.client.post(ANTHROPIC_API).json(&body))
            .timeout(Duration::from_secs(180))
            .send()
            .await?
            .error_for_status()?;

        let v: Value = res.json().await?;
        Self::parse_message_response(&v)
    }

    async fn stream(
        &self,
        request: &CompletionRequest,
        tx: tokio::sync::mpsc::Sender<Result<StreamChunk, ProviderError>>,
    ) -> Result<(), ProviderError> {
        let body = self.build_body(request, true);
        let res = self
            .apply_auth(self.client.post(ANTHROPIC_API).json(&body))
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
                if line.is_empty() || line.starts_with(':') {
                    continue;
                }
                let Some(data) = line.strip_prefix("data:").map(str::trim) else {
                    continue;
                };
                let v: Value = match serde_json::from_str(data) {
                    Ok(v) => v,
                    Err(_) => continue,
                };
                if let Some(err) = v["error"]["message"].as_str() {
                    let _ = tx.send(Err(ProviderError::Api(err.to_string()))).await;
                    return Ok(());
                }
                let ty = v["type"].as_str().unwrap_or("");
                if ty == "error" {
                    let msg = v["error"]["message"]
                        .as_str()
                        .or_else(|| v["error"].as_str())
                        .unwrap_or("Anthropic stream error");
                    let _ = tx.send(Err(ProviderError::Api(msg.to_string()))).await;
                    return Ok(());
                }
                if ty == "content_block_delta" {
                    let delta = &v["delta"];
                    let dty = delta["type"].as_str().unwrap_or("");
                    if dty == "text_delta" {
                        let piece = delta["text"].as_str().unwrap_or("");
                        if !piece.is_empty() {
                            let _ = tx
                                .send(Ok(StreamChunk {
                                    delta: piece.to_string(),
                                    done: false,
                                }))
                                .await;
                        }
                    }
                } else if ty == "message_stop" {
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

const ANTHROPIC_MODELS_URL: &str = "https://api.anthropic.com/v1/models";

/// Lists Claude model ids from `GET /v1/models` (requires saved Anthropic API key).
pub async fn fetch_anthropic_model_ids(
    http: &reqwest::Client,
    settings: &SettingsManager,
) -> Result<Vec<String>, ProviderError> {
    let api_key = settings
        .decrypt_api_key("anthropic")?
        .filter(|s| !s.trim().is_empty())
        .ok_or(ProviderError::MissingApiKey("anthropic"))?;
    let res = http
        .get(ANTHROPIC_MODELS_URL)
        .header("x-api-key", api_key.trim())
        .header("anthropic-version", ANTHROPIC_VERSION)
        .timeout(Duration::from_secs(45))
        .send()
        .await?
        .error_for_status()?;
    let v: Value = res.json().await?;
    if let Some(msg) = v["error"]["message"].as_str() {
        return Err(ProviderError::Api(msg.to_string()));
    }
    let mut names = Vec::new();
    if let Some(arr) = v["data"].as_array().or_else(|| v["models"].as_array()) {
        for m in arr {
            if let Some(id) = m["id"].as_str() {
                names.push(id.to_string());
            }
        }
    }
    names.sort();
    names.dedup();
    Ok(names)
}
