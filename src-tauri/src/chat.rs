//! Chat send pipeline: MemoryAnchor context, settings-backed LLM, streamed assistant reply.

use serde::Serialize;
use tauri::{AppHandle, Emitter, State};

use crate::memory::{ConversationMemory, MemoryRecallBundle, MessageRole, DEFAULT_PERSONALITY_ID};
use crate::provider::{
    build_engine, ChatSendResult, ChatTurn, CompletionRequest, LLMProviderEngine, ProviderError,
    StreamChunk, ToolCall,
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
    messages.push(ChatTurn {
        role: "system".into(),
        content: system_content,
    });
    for m in recent {
        let role = match m.role {
            MessageRole::User => "user",
            MessageRole::Assistant => "assistant",
        };
        messages.push(ChatTurn {
            role: role.into(),
            content: m.content,
        });
    }

    let configured = state.settings.max_tokens();
    let max_tokens = match configured {
        Some(n) => Some(n),
        None => engine.model_info().context_window_tokens,
    };

    let temperature = state.settings.temperature();
    eprintln!("nova: chat_send_message temperature={temperature}");
    let req = CompletionRequest {
        messages,
        tools: None,
        max_tokens,
        temperature: Some(temperature),
    };

    let (tx, mut rx) = tokio::sync::mpsc::channel::<Result<StreamChunk, ProviderError>>(64);
    let engine_clone = engine.clone();
    let send_task = tokio::spawn(async move { engine_clone.stream(&req, tx).await });

    let mut full = String::new();
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
