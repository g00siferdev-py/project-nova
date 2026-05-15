//! Chat send pipeline: MemoryAnchor context, settings-backed LLM, streamed assistant reply.

use serde::Serialize;
use serde_json::json;
use tauri::{AppHandle, Emitter, State};

use std::path::Path;

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

#[tauri::command]
pub async fn chat_send_message(
    app: AppHandle,
    state: State<'_, NovaState>,
    conversation_id: String,
    message: String,
    personality_id: Option<String>,
) -> Result<ChatSendResult, String> {
    let content = message.trim().to_string();
    if content.is_empty() {
        return Err("message content is empty".into());
    }

    let pid = personality_id
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or(DEFAULT_PERSONALITY_ID);
    state
        .personality
        .set_active_profile_id(pid)
        .map_err(|e| format!("companion persona sync: {e}"))?;
    ConversationMemory::set_active_personality(&*state.memory, pid);
    eprintln!(
        "nova: chat_send_message personality_id={pid} conversation_id={} (PersonalityManager + MemoryAnchor aligned)",
        conversation_id
    );

    state
        .memory
        .store_message(&conversation_id, MessageRole::User, &content)
        .map_err(|e| e.to_string())?;
    eprintln!(
        "nova: chat persisted user message — thread={} personality_id={pid} chars={}",
        conversation_id,
        content.chars().count()
    );

    let engine: std::sync::Arc<dyn LLMProviderEngine + Send + Sync> =
        match build_engine(&state.http, &state.settings) {
            Ok(e) => {
                *state.llm.write().await = e.clone();
                e
            }
            Err(e) => return Err(e.to_string()),
        };

    let _ = app.emit(
        "chat:stream-start",
        ChatStreamStart {
            conversation_id: conversation_id.clone(),
        },
    );

    let mut briefing = state
        .memory
        .get_startup_briefing(&conversation_id)
        .map_err(|e| e.to_string())?;

    let recall_heuristic = should_auto_memory_recall(&content);
    eprintln!(
        "nova: memory auto-recall heuristic for this send: {} (thread={})",
        recall_heuristic, conversation_id
    );
    if recall_heuristic {
        let recall_q: String = content.chars().take(520).collect();
        eprintln!(
            "nova: memory auto-recall invoking hybrid search — query_chars={} (cross-thread: all conversations)",
            recall_q.chars().count()
        );
        match state.memory.memory_recall(&recall_q, None, 12, 14) {
            Ok(bundle) if !bundle.anchors.is_empty() || !bundle.messages.is_empty() => {
                eprintln!(
                    "nova: memory auto-recall retrieved — anchors={}, messages={}",
                    bundle.anchors.len(),
                    bundle.messages.len()
                );
                let block = format_recall_for_prompt(&bundle, 1_800);
                let preview: String = block.chars().take(240).collect();
                let preview = preview.replace('\n', " ");
                eprintln!("nova: memory auto-recall injecting into system briefing — block_chars={}, preview=\"{preview}…\"", block.chars().count());
                briefing.push_str("\n\n## Retrieved memory (auto)\n\n");
                briefing.push_str(&block);
            }
            Ok(bundle) => eprintln!(
                "nova: memory auto-recall: no hits (anchors={}, messages={})",
                bundle.anchors.len(),
                bundle.messages.len()
            ),
            Err(e) => eprintln!("nova: memory auto-recall failed: {e}"),
        }
    }

    let recent = state
        .memory
        .get_recent(&conversation_id, 48)
        .map_err(|e| e.to_string())?;

    let persona = state.personality.system_prompt_prefix();
    eprintln!(
        "nova: chat_send_message system_prompt_prefix chars={} (persona aligned to personality_id={pid})",
        persona.chars().count()
    );
    let system_content = {
        let p = persona.trim();
        if p.is_empty() {
            briefing.clone()
        } else {
            format!("{p}\n\n---\n\n# Memory & session context\n\n{briefing}")
        }
    };
    eprintln!(
        "nova: chat LLM system layer chars={} (persona + MemoryAnchor briefing + any auto-recall)",
        system_content.chars().count()
    );

    let mut messages: Vec<ChatTurn> = Vec::with_capacity(recent.len() + 1);
    messages.push(ChatTurn::text("system", system_content));
    for m in recent {
        let role = match m.role {
            MessageRole::User => "user",
            MessageRole::Assistant => "assistant",
        };
        messages.push(ChatTurn::text(role, m.content));
    }

    let configured = state.settings.max_tokens();
    let max_tokens = match configured {
        Some(n) => Some(n),
        None => engine.model_info().context_window_tokens,
    };

    let temperature = state.settings.temperature();
    eprintln!("nova: chat_send_message temperature={temperature}");

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

    let agent_tool_backend = (!tool_definitions.is_empty())
        .then(|| web_tool_backend_for_provider(engine.provider_id()))
        .flatten();
    if agent_tool_backend.is_some() {
        eprintln!(
            "nova: chat agent tools enabled (provider={}) — non-streaming tool loop + synthetic stream",
            engine.provider_id()
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
                emit_synthetic_stream_deltas(&app, &conversation_id, &full);
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
                                conversation_id: conversation_id.clone(),
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
                                conversation_id: conversation_id.clone(),
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
                    conversation_id: conversation_id.clone(),
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
        .store_message(&conversation_id, MessageRole::Assistant, &reply)
        .map_err(|e| e.to_string())?;

    let info = engine.model_info();
    Ok(ChatSendResult {
        reply,
        tool_calls: Vec::<ToolCall>::new(),
        provider_id: info.provider_id,
        model_id: info.model_id,
    })
}
