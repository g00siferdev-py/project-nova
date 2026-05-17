//! Chat send pipeline: MemoryAnchor context, settings-backed LLM, streamed assistant reply.

use serde::Serialize;
use serde_json::json;
use tauri::{AppHandle, Emitter, State};

use std::path::Path;

use crate::attachments::{self, model_supports_vision};
use crate::memory::{ConversationMemory, MemoryRecallBundle, MessageRole, DEFAULT_PERSONALITY_ID};
use crate::provider::{
    build_engine, ChatSendResult, ChatTurn, CompletionRequest, CompletionResponse, LLMProviderEngine,
    ProviderError, StreamChunk, ToolCall, ToolDefinition,
};
use crate::NovaState;

fn should_auto_memory_recall(user_text: &str) -> bool {
    let t = user_text.trim();
    if t.len() >= 140 {
        return true;
    }
    if t.split_whitespace().count() >= 14 {
        return true;
    }
    if t.contains('?') {
        return true;
    }
    let lower = t.to_lowercase();
    const KEYS: &[&str] = &[
        "remember",
        "last time",
        "earlier",
        "previously",
        "before we",
        "mentioned",
        "you said",
        "what did",
        "what is my",
        "what are my",
        "who is",
        "when did",
        "project",
        "recall",
        "context",
        "conversation about",
        "told me",
        "favorite",
        "preference",
        "previously said",
    ];
    KEYS.iter().any(|k| lower.contains(k))
}

fn format_recall_for_prompt(bundle: &MemoryRecallBundle, max_chars: usize) -> String {
    let mut out = String::new();
    if !bundle.anchors.is_empty() {
        out.push_str("**Anchors**\n");
        for a in &bundle.anchors {
            out.push_str(&format!(
                "- [{}] (importance {}): {}\n",
                a.anchor_type, a.importance, a.content
            ));
        }
        out.push('\n');
    }
    if !bundle.messages.is_empty() {
        out.push_str("**Related past messages**\n");
        for m in &bundle.messages {
            let label = match m.role {
                MessageRole::User => "User",
                MessageRole::Assistant => "Assistant",
            };
            let snippet: String = m.content.chars().take(200).collect();
            let thread = match (&m.conversation_title, &m.conversation_id) {
                (Some(title), _) if !title.trim().is_empty() => format!(" [thread: {title}]"),
                (_, Some(id)) if !id.trim().is_empty() => format!(" [thread id: {id}]"),
                _ => String::new(),
            };
            out.push_str(&format!("- **{label}**{thread}: {snippet}\n"));
        }
    }
    if out.chars().count() > max_chars {
        out.chars().take(max_chars.saturating_sub(1)).collect::<String>() + "…"
    } else {
        out
    }
}

fn emit_synthetic_stream_deltas(app: &AppHandle, conversation_id: &str, text: &str) {
    const CHUNK_CHARS: usize = 72;
    let mut buf = String::new();
    let mut n = 0usize;
    for ch in text.chars() {
        buf.push(ch);
        n += 1;
        if n >= CHUNK_CHARS {
            let _ = app.emit(
                "chat:stream",
                ChatStreamEvent {
                    conversation_id: conversation_id.to_string(),
                    delta: std::mem::take(&mut buf),
                    done: false,
                },
            );
            n = 0;
        }
    }
    if !buf.is_empty() {
        let _ = app.emit(
            "chat:stream",
            ChatStreamEvent {
                conversation_id: conversation_id.to_string(),
                delta: buf,
                done: false,
            },
        );
    }
    let _ = app.emit(
        "chat:stream",
        ChatStreamEvent {
            conversation_id: conversation_id.to_string(),
            delta: String::new(),
            done: true,
        },
    );
}

fn assistant_openai_message_with_tool_calls(resp: &CompletionResponse) -> serde_json::Value {
    let content_val = if resp.content.trim().is_empty() {
        serde_json::Value::Null
    } else {
        json!(resp.content)
    };
    let tool_calls_json: Vec<serde_json::Value> = resp
        .tool_calls
        .iter()
        .map(|tc| {
            json!({
                "id": &tc.id,
                "type": "function",
                "function": {
                    "name": &tc.name,
                    "arguments": &tc.arguments_json
                }
            })
        })
        .collect();
    json!({
        "role": "assistant",
        "content": content_val,
        "tool_calls": tool_calls_json,
    })
}

fn ollama_assistant_with_tool_calls(resp: &CompletionResponse) -> serde_json::Value {
    let tool_calls: Vec<serde_json::Value> = resp
        .tool_calls
        .iter()
        .enumerate()
        .map(|(i, tc)| {
            let args: serde_json::Value =
                serde_json::from_str(&tc.arguments_json).unwrap_or(json!({}));
            json!({
                "type": "function",
                "function": {
                    "index": i,
                    "name": &tc.name,
                    "arguments": args
                }
            })
        })
        .collect();
    json!({
        "role": "assistant",
        "content": resp.content.clone(),
        "tool_calls": tool_calls,
    })
}

fn anthropic_assistant_with_tool_calls(resp: &CompletionResponse) -> serde_json::Value {
    let mut blocks = Vec::new();
    if !resp.content.trim().is_empty() {
        blocks.push(json!({"type": "text", "text": resp.content}));
    }
    for tc in &resp.tool_calls {
        let input: serde_json::Value =
            serde_json::from_str(&tc.arguments_json).unwrap_or(json!({}));
        blocks.push(json!({
            "type": "tool_use",
            "id": &tc.id,
            "name": &tc.name,
            "input": input
        }));
    }
    json!({ "role": "assistant", "content": blocks })
}

fn anthropic_user_tool_results(tool_calls: &[ToolCall], bodies: &[String]) -> serde_json::Value {
    let blocks: Vec<serde_json::Value> = tool_calls
        .iter()
        .zip(bodies.iter())
        .map(|(tc, body)| {
            json!({
                "type": "tool_result",
                "tool_use_id": &tc.id,
                "content": body
            })
        })
        .collect();
    json!({ "role": "user", "content": blocks })
}

#[derive(Clone, Copy)]
enum AgentWebToolBackend {
    OpenAI,
    Ollama,
    Anthropic,
}

fn web_tool_backend_for_provider(provider_id: &str) -> Option<AgentWebToolBackend> {
    match provider_id {
        "openai" => Some(AgentWebToolBackend::OpenAI),
        "ollama" | "ollama_cloud" => Some(AgentWebToolBackend::Ollama),
        "anthropic" => Some(AgentWebToolBackend::Anthropic),
        _ => None,
    }
}

/// Non-streaming completion with tool rounds (OpenAI, Ollama, Anthropic).
async fn agent_complete_with_tools(
    engine: &(dyn LLMProviderEngine + Send + Sync),
    http: &reqwest::Client,
    workspace_root: Option<&Path>,
    data_directory: &Path,
    database_app_data_enabled: bool,
    database_allow_write: bool,
    mut messages: Vec<ChatTurn>,
    max_tokens: Option<u32>,
    temperature: f32,
    backend: AgentWebToolBackend,
    tools: Vec<ToolDefinition>,
) -> Result<String, ProviderError> {
    const MAX_ROUNDS: usize = 8;
    if tools.is_empty() {
        return Err(ProviderError::Api("internal: no tools configured".into()));
    }
    for _ in 0..MAX_ROUNDS {
        let req = CompletionRequest {
            messages: messages.clone(),
            tools: Some(tools.clone()),
            max_tokens,
            temperature: Some(temperature),
        };
        let resp = engine.complete(&req).await?;
        if resp.tool_calls.is_empty() {
            return Ok(resp.content);
        }

        match backend {
            AgentWebToolBackend::OpenAI => {
                messages.push(ChatTurn {
                    role: "assistant".into(),
                    content: resp.content.clone(),
                    openai_message: Some(assistant_openai_message_with_tool_calls(&resp)),
                    ollama_message: None,
                    anthropic_message: None,
                });
                for tc in &resp.tool_calls {
                    let body = crate::agent_tools::run_builtin_tool(
                        http,
                        workspace_root,
                        data_directory,
                        database_app_data_enabled,
                        database_allow_write,
                        &tc.name,
                        &tc.arguments_json,
                    )
                    .await
                    .unwrap_or_else(|e| format!("Tool error: {e}"));
                    messages.push(ChatTurn {
                        role: "tool".into(),
                        content: body.clone(),
                        openai_message: Some(json!({
                            "role": "tool",
                            "tool_call_id": &tc.id,
                            "content": body,
                        })),
                        ollama_message: None,
                        anthropic_message: None,
                    });
                }
            }
            AgentWebToolBackend::Ollama => {
                messages.push(ChatTurn {
                    role: "assistant".into(),
                    content: resp.content.clone(),
                    openai_message: None,
                    ollama_message: Some(ollama_assistant_with_tool_calls(&resp)),
                    anthropic_message: None,
                });
                for tc in &resp.tool_calls {
                    let body = crate::agent_tools::run_builtin_tool(
                        http,
                        workspace_root,
                        data_directory,
                        database_app_data_enabled,
                        database_allow_write,
                        &tc.name,
                        &tc.arguments_json,
                    )
                    .await
                    .unwrap_or_else(|e| format!("Tool error: {e}"));
                    messages.push(ChatTurn {
                        role: "tool".into(),
                        content: body.clone(),
                        openai_message: None,
                        ollama_message: Some(json!({
                            "role": "tool",
                            "tool_name": &tc.name,
                            "content": body,
                        })),
                        anthropic_message: None,
                    });
                }
            }
            AgentWebToolBackend::Anthropic => {
                messages.push(ChatTurn {
                    role: "assistant".into(),
                    content: resp.content.clone(),
                    openai_message: None,
                    ollama_message: None,
                    anthropic_message: Some(anthropic_assistant_with_tool_calls(&resp)),
                });
                let mut bodies: Vec<String> = Vec::with_capacity(resp.tool_calls.len());
                for tc in &resp.tool_calls {
                    let body = crate::agent_tools::run_builtin_tool(
                        http,
                        workspace_root,
                        data_directory,
                        database_app_data_enabled,
                        database_allow_write,
                        &tc.name,
                        &tc.arguments_json,
                    )
                    .await
                    .unwrap_or_else(|e| format!("Tool error: {e}"));
                    bodies.push(body);
                }
                messages.push(ChatTurn {
                    role: "user".into(),
                    content: bodies.join("\n---\n"),
                    openai_message: None,
                    ollama_message: None,
                    anthropic_message: Some(anthropic_user_tool_results(&resp.tool_calls, &bodies)),
                });
            }
        }
    }
    Err(ProviderError::Api(
        "Agent stopped after maximum tool rounds — try a narrower question.".into(),
    ))
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ChatStreamStart {
    pub conversation_id: String,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ChatStreamEvent {
    pub conversation_id: String,
    pub delta: String,
    pub done: bool,
}

/// Shared LLM path after `messages` (system + transcript) is built: tools or stream, then persist assistant.
async fn run_chat_completion(
    app: &AppHandle,
    state: &NovaState,
    conversation_id: &str,
    engine: &std::sync::Arc<dyn LLMProviderEngine + Send + Sync>,
    messages: Vec<ChatTurn>,
) -> Result<String, String> {
    let configured = state.settings.max_tokens();
    let max_tokens = match configured {
        Some(n) => Some(n),
        None => engine.model_info().context_window_tokens,
    };

    let temperature = state.settings.temperature();

    let mut tool_definitions: Vec<ToolDefinition> = Vec::new();
    if state.settings.agent_web_tools_enabled() {
        tool_definitions.extend(crate::agent_tools::builtin_tool_definitions());
    }
    if state.settings.agent_workspace_enabled() {
        tool_definitions.extend(crate::agent_tools::workspace_tool_definitions());
    }
    let database_tools_enabled =
        state.settings.agent_workspace_enabled() || state.settings.database_app_data_enabled();
    if database_tools_enabled {
        tool_definitions.extend(crate::database_query::tool_definitions());
    }
    let workspace_root_for_tools = state
        .settings
        .agent_workspace_enabled()
        .then_some(state.workspace_root.as_path());
    let database_app_data_enabled = state.settings.database_app_data_enabled();
    let database_allow_write = state.settings.database_allow_write();

    let has_images = attachments::messages_include_images(&messages);
    let provider_id = engine.provider_id();
    // Ollama often ignores `images` when `tools` are present — prefer vision over tools for that turn.
    let agent_tool_backend = (!tool_definitions.is_empty())
        .then(|| web_tool_backend_for_provider(provider_id))
        .flatten()
        .filter(|_| {
            !(has_images && matches!(provider_id, "ollama" | "ollama_cloud"))
        });

    if has_images {
        eprintln!(
            "nova: chat completion includes image(s) for provider={provider_id} tools={}",
            agent_tool_backend.is_some()
        );
    }

    let mut full = String::new();

    if let Some(backend) = agent_tool_backend {
        match agent_complete_with_tools(
            engine.as_ref(),
            &state.http,
            workspace_root_for_tools,
            state.data_directory.as_path(),
            database_app_data_enabled,
            database_allow_write,
            messages,
            max_tokens,
            temperature,
            backend,
            tool_definitions,
        )
        .await
        {
            Ok(text) => {
                full = text;
                emit_synthetic_stream_deltas(app, conversation_id, &full);
            }
            Err(e) => {
                let msg = e.to_string();
                let _ = app.emit("chat:stream-error", msg.clone());
                return Err(msg);
            }
        }
    } else {
        let req = CompletionRequest {
            messages,
            tools: None,
            max_tokens,
            temperature: Some(temperature),
        };

        let (tx, mut rx) = tokio::sync::mpsc::channel::<Result<StreamChunk, ProviderError>>(64);
        let engine_clone = engine.clone();
        let send_task = tokio::spawn(async move { engine_clone.stream(&req, tx).await });

        let mut saw_done = false;
        while let Some(item) = rx.recv().await {
            match item {
                Ok(chunk) => {
                    if !chunk.delta.is_empty() {
                        full.push_str(&chunk.delta);
                        let _ = app.emit(
                            "chat:stream",
                            ChatStreamEvent {
                                conversation_id: conversation_id.to_string(),
                                delta: chunk.delta,
                                done: false,
                            },
                        );
                    }
                    if chunk.done {
                        saw_done = true;
                        let _ = app.emit(
                            "chat:stream",
                            ChatStreamEvent {
                                conversation_id: conversation_id.to_string(),
                                delta: String::new(),
                                done: true,
                            },
                        );
                        break;
                    }
                }
                Err(e) => {
                    let msg = e.to_string();
                    let _ = app.emit("chat:stream-error", msg.clone());
                    send_task.abort();
                    return Err(msg);
                }
            }
        }

        match send_task.await {
            Ok(Ok(())) => {}
            Ok(Err(e)) => {
                let msg = e.to_string();
                let _ = app.emit("chat:stream-error", msg.clone());
                return Err(msg);
            }
            Err(j) => {
                let msg = j.to_string();
                let _ = app.emit("chat:stream-error", msg.clone());
                return Err(msg);
            }
        }

        if !saw_done {
            let _ = app.emit(
                "chat:stream",
                ChatStreamEvent {
                    conversation_id: conversation_id.to_string(),
                    delta: String::new(),
                    done: true,
                },
            );
        }
    }

    let mut reply = full;
    if reply.trim().is_empty() {
        reply = "(no text in model response)".into();
    }

    state
        .memory
        .store_message(conversation_id, MessageRole::Assistant, &reply, None, None)
        .map_err(|e| e.to_string())?;

    Ok(reply)
}

/// Saved image for the user turn about to be stored.
pub struct PendingImage {
    pub rel_path: String,
    pub mime: String,
}

/// One user turn on an existing conversation — same path for manual send and scheduled Pulse.
pub async fn execute_chat_turn(
    app: &AppHandle,
    state: &NovaState,
    conversation_id: &str,
    message: &str,
    personality_id: &str,
    pending_image: Option<PendingImage>,
) -> Result<String, String> {
    let text = message.trim();
    if text.is_empty() && pending_image.is_none() {
        return Err("message content is empty".into());
    }

    let pid = personality_id.trim();
    let pid = if pid.is_empty() {
        DEFAULT_PERSONALITY_ID
    } else {
        pid
    };
    state
        .personality
        .set_active_profile_id(pid)
        .map_err(|e| format!("companion persona sync: {e}"))?;
    ConversationMemory::set_active_personality(&*state.memory, pid);

    let engine: std::sync::Arc<dyn LLMProviderEngine + Send + Sync> =
        match build_engine(&state.http, &state.settings) {
            Ok(e) => {
                *state.llm.write().await = e.clone();
                e
            }
            Err(e) => return Err(e.to_string()),
        };

    if pending_image.is_some() && !model_supports_vision(engine.provider_id(), &engine.model_info().model_id) {
        return Err(format!(
            "The active model ({}) does not support image input. Switch to a vision-capable model (e.g. gpt-4o, Claude 3+, or a llava/vision Ollama model).",
            engine.model_info().model_id
        ));
    }

    let (img_rel, img_mime) = match &pending_image {
        Some(p) => (Some(p.rel_path.as_str()), Some(p.mime.as_str())),
        None => (None, None),
    };

    state
        .memory
        .store_message(conversation_id, MessageRole::User, text, img_rel, img_mime)
        .map_err(|e| e.to_string())?;

    let _ = app.emit(
        "chat:stream-start",
        ChatStreamStart {
            conversation_id: conversation_id.to_string(),
        },
    );

    let mut briefing = state
        .memory
        .get_startup_briefing(conversation_id)
        .map_err(|e| e.to_string())?;

    let recall_source = if !text.is_empty() {
        text
    } else {
        "image attachment"
    };
    if should_auto_memory_recall(recall_source) {
        let recall_q: String = recall_source.chars().take(520).collect();
        match state.memory.memory_recall(&recall_q, None, 12, 14) {
            Ok(bundle) if !bundle.anchors.is_empty() || !bundle.messages.is_empty() => {
                let block = format_recall_for_prompt(&bundle, 1_800);
                briefing.push_str("\n\n## Retrieved memory (auto)\n\n");
                briefing.push_str(&block);
            }
            Ok(_) => {}
            Err(e) => eprintln!("nova: memory auto-recall failed: {e}"),
        }
    }

    let recent = state
        .memory
        .get_recent(conversation_id, 48)
        .map_err(|e| e.to_string())?;

    let persona = state.personality.system_prompt_prefix();
    let system_content = {
        let p = persona.trim();
        if p.is_empty() {
            briefing.clone()
        } else {
            format!("{p}\n\n---\n\n# Memory & session context\n\n{briefing}")
        }
    };

    let provider_id = engine.provider_id().to_string();
    let data_dir = state.data_directory.as_path();

    let mut messages: Vec<ChatTurn> = Vec::with_capacity(recent.len() + 1);
    messages.push(ChatTurn::text("system", system_content));
    for m in recent {
        let turn = attachments::chat_turn_from_stored(&provider_id, data_dir, &m)?;
        messages.push(turn);
    }

    run_chat_completion(app, state, conversation_id, &engine, messages).await
}

#[tauri::command]
pub async fn chat_send_message(
    app: AppHandle,
    state: State<'_, NovaState>,
    conversation_id: String,
    message: String,
    personality_id: Option<String>,
    image_base64: Option<String>,
    image_mime: Option<String>,
) -> Result<ChatSendResult, String> {
    let pid = personality_id
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or(DEFAULT_PERSONALITY_ID);

    let engine_probe = build_engine(&state.http, &state.settings).map_err(|e| e.to_string())?;
    let pending_image = match (image_base64, image_mime) {
        (Some(b64), Some(mime)) if !b64.trim().is_empty() => {
            let info = engine_probe.model_info();
            if !model_supports_vision(engine_probe.provider_id(), &info.model_id) {
                return Err(format!(
                    "The active model ({}) does not support image input. Switch to a vision-capable model (e.g. gpt-4o, Claude 3+, or a llava/vision Ollama model).",
                    info.model_id
                ));
            }
            let (rel, mime) = attachments::save_image_attachment(
                &state.data_directory,
                conversation_id.trim(),
                &mime,
                &b64,
            )?;
            Some(PendingImage { rel_path: rel, mime })
        }
        (Some(_), None) => return Err("imageMime is required when sending an image".into()),
        (None, Some(_)) => return Err("image data missing".into()),
        _ => None,
    };

    let reply = execute_chat_turn(
        &app,
        &state,
        conversation_id.trim(),
        &message,
        pid,
        pending_image,
    )
    .await?;

    let engine = state.llm.read().await.clone();
    let info = engine.model_info();
    Ok(ChatSendResult {
        reply,
        tool_calls: Vec::<ToolCall>::new(),
        provider_id: info.provider_id,
        model_id: info.model_id,
    })
}

#[tauri::command]
pub async fn chat_vision_supported(state: State<'_, NovaState>) -> Result<bool, String> {
    let engine = state.llm.read().await.clone();
    let info = engine.model_info();
    Ok(model_supports_vision(&info.provider_id, &info.model_id))
}
