//! Nova — portable, privacy-first AI companion (Rust + Tauri).
//!
//! **Local-first**: conversation memory defaults to SQLite on disk; model
//! traffic goes only through pluggable [`provider::LLMProviderEngine`] backends
//! the user configures (no cloud storage in core). **Portable runs**: set
//! `NOVA_DATA_DIR` or `NOVA_PORTABLE=1` so data stays with the app (e.g. USB).
//!
//! Application entry for mobile builds is [`run`]. Desktop [`main`] in
//! `main.rs` delegates here so the same setup runs everywhere.

mod agent_tools;
mod attachments;
mod chat;
mod database_query;
mod memory;
mod pulse;
mod personality;
mod settings;
mod provider;

use std::path::PathBuf;
use std::sync::Arc;

use memory::{
    AnchorType, ConversationMemory, MemoryAnchor, MemoryRecallBundle, MessageRole, SqliteProfile,
    StoredAnchor, StoredConversation, StoredMessage, StoredProject, DEFAULT_PERSONALITY_ID,
};
use provider::{
    build_engine, fetch_anthropic_model_ids, fetch_ollama_cloud_model_tags,
    fetch_ollama_local_model_tags, fetch_openai_model_ids, list_provider_descriptors,
    LLMProviderEngine, PlaceholderEngine, ProviderDescriptor, ProviderError,
};
use personality::{PersonalityFile, PersonalityManager, PersonalitySnapshot};
use settings::{SettingsManager, SettingsUpdatePayload, SettingsView};
use serde::Serialize;
use tauri::State;

// --- App state ----------------------------------------------------------------

pub struct NovaState {
    pub(crate) http: reqwest::Client,
    pub(crate) llm: tokio::sync::RwLock<Arc<dyn LLMProviderEngine + Send + Sync>>,
    pub(crate) memory: Arc<dyn ConversationMemory + Send + Sync>,
    pub(crate) settings: Arc<SettingsManager>,
    pub(crate) personality: Arc<PersonalityManager>,
    /// Canonical agent workspace (`{data_dir}/workspace`). Created at startup; tools only touch paths inside it.
    pub(crate) workspace_root: PathBuf,
    /// Canonical Nova data directory (same resolution as MemoryAnchor: `NOVA_DATA_DIR`, portable `data/`, or OS app data).
    pub(crate) data_directory: PathBuf,
}

impl NovaState {
    #[must_use]
    pub fn new(
        memory: Arc<dyn ConversationMemory + Send + Sync>,
        settings: Arc<SettingsManager>,
        personality: Arc<PersonalityManager>,
        workspace_root: PathBuf,
        data_directory: PathBuf,
    ) -> Self {
        let http = reqwest::Client::builder()
            .user_agent(format!("Nova/{}", env!("CARGO_PKG_VERSION")))
            .build()
            .expect("reqwest Client");

        let llm: Arc<dyn LLMProviderEngine + Send + Sync> = match build_engine(&http, &settings) {
            Ok(e) => e,
            Err(e) => {
                eprintln!("nova: provider init failed ({e}), using placeholder");
                Arc::new(PlaceholderEngine::new())
            }
        };

        Self {
            http,
            llm: tokio::sync::RwLock::new(llm),
            memory,
            settings,
            personality,
            workspace_root,
            data_directory,
        }
    }
}

fn parse_anchor_type(s: &str) -> Result<AnchorType, String> {
    match s.to_lowercase().as_str() {
        "raw" => Ok(AnchorType::Raw),
        "curated" => Ok(AnchorType::Curated),
        "fact" => Ok(AnchorType::Fact),
        "insight" => Ok(AnchorType::Insight),
        _ => Err(format!(
            "unknown anchor type '{s}' (use raw, curated, fact, insight)"
        )),
    }
}

// --- Tauri commands -----------------------------------------------------------

#[tauri::command]
fn app_version() -> String {
    format!("{} {}", env!("CARGO_PKG_NAME"), env!("CARGO_PKG_VERSION"))
}

/// Where SQLite and `settings.json` live (per machine). Helps debug “works on my other computer”.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppDataPaths {
    pub data_directory: String,
    pub database_file: String,
    /// Agent read/write sandbox (`{dataDirectory}/workspace`).
    pub workspace_directory: String,
    /// `desktop` (WAL) vs `portable` (from `NOVA_DATA_DIR` / `NOVA_PORTABLE`).
    pub sqlite_profile: String,
    pub nova_data_dir_env: bool,
    pub nova_portable_env: bool,
}

#[tauri::command]
fn app_data_paths() -> Result<AppDataPaths, String> {
    let database_file = memory::default_db_path().map_err(|e| e.to_string())?;
    let data_directory = memory::default_data_dir().map_err(|e| e.to_string())?;
    let sqlite_profile = match memory::sqlite_profile_from_env() {
        SqliteProfile::Desktop => "desktop",
        SqliteProfile::Portable => "portable",
    };
    let nova_data_dir_env = std::env::var("NOVA_DATA_DIR")
        .map(|s| !s.trim().is_empty())
        .unwrap_or(false);
    let nova_portable_env = std::env::var("NOVA_PORTABLE")
        .map(|s| s == "1" || s.eq_ignore_ascii_case("true"))
        .unwrap_or(false);
    let workspace_directory = data_directory.join("workspace");
    Ok(AppDataPaths {
        data_directory: data_directory.to_string_lossy().into_owned(),
        database_file: database_file.to_string_lossy().into_owned(),
        workspace_directory: workspace_directory.to_string_lossy().into_owned(),
        sqlite_profile: sqlite_profile.into(),
        nova_data_dir_env,
        nova_portable_env,
    })
}

/// Opens the resolved data directory in the system file manager (Finder, Explorer, Nautilus, …).
#[tauri::command]
fn reveal_data_directory() -> Result<(), String> {
    let dir = memory::default_data_dir().map_err(|e| e.to_string())?;
    opener::open(&dir).map_err(|e| format!("open data folder: {e}"))
}

#[tauri::command]
async fn provider_info(state: State<'_, NovaState>) -> Result<String, String> {
    let engine = state.llm.read().await.clone();
    let m = engine.model_info();
    let ctx = m
        .context_window_tokens
        .map(|n| n.to_string())
        .unwrap_or_else(|| "unknown".into());
    Ok(format!(
        "{} — model `{}`, context ~{} tokens",
        m.provider_id, m.model_id, ctx
    ))
}

#[tauri::command]
fn provider_list_available() -> Vec<ProviderDescriptor> {
    list_provider_descriptors()
}

#[tauri::command]
async fn ollama_cloud_list_models(state: State<'_, NovaState>) -> Result<Vec<String>, String> {
    fetch_ollama_cloud_model_tags(&state.http, &state.settings)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn openai_list_models(state: State<'_, NovaState>) -> Result<Vec<String>, String> {
    fetch_openai_model_ids(&state.http, &state.settings)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn ollama_list_local_models(state: State<'_, NovaState>) -> Result<Vec<String>, String> {
    fetch_ollama_local_model_tags(&state.http, &state.settings)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn anthropic_list_models(state: State<'_, NovaState>) -> Result<Vec<String>, String> {
    fetch_anthropic_model_ids(&state.http, &state.settings)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn provider_switch(state: State<'_, NovaState>, provider_id: String) -> Result<(), String> {
    let id = provider_id.trim().to_lowercase();
    state
        .settings
        .apply_update(SettingsUpdatePayload {
            selected_provider: Some(id),
            ..Default::default()
        })
        .map_err(|e| e.to_string())?;

    let engine = build_engine(&state.http, &state.settings).map_err(|e: ProviderError| e.to_string())?;
    *state.llm.write().await = engine;
    Ok(())
}

#[tauri::command]
fn settings_get(state: State<NovaState>) -> Result<SettingsView, String> {
    state.settings.view().map_err(|e| e.to_string())
}

#[tauri::command]
async fn settings_update(
    state: State<'_, NovaState>,
    patch: SettingsUpdatePayload,
) -> Result<SettingsView, String> {
    state.settings.apply_update(patch).map_err(|e| e.to_string())?;
    match build_engine(&state.http, &state.settings) {
        Ok(engine) => *state.llm.write().await = engine,
        Err(e) => {
            eprintln!("nova: rebuild LLM after settings failed ({e}), keeping placeholder");
            *state.llm.write().await = Arc::new(PlaceholderEngine::new());
        }
    }
    state.settings.view().map_err(|e| e.to_string())
}

#[tauri::command]
fn personality_get(state: State<NovaState>) -> Result<PersonalitySnapshot, String> {
    state.personality.snapshot().map_err(|e| e.to_string())
}

#[tauri::command]
fn personality_save(state: State<NovaState>, file: PersonalityFile) -> Result<PersonalitySnapshot, String> {
    state
        .personality
        .replace_all(file)
        .map_err(|e| e.to_string())?;
    state.personality.snapshot().map_err(|e| e.to_string())
}

#[tauri::command]
async fn settings_save_api_key(
    state: State<'_, NovaState>,
    provider: String,
    api_key: String,
) -> Result<(), String> {
    state
        .settings
        .save_api_key(&provider, &api_key)
        .map_err(|e| e.to_string())?;
    match build_engine(&state.http, &state.settings) {
        Ok(engine) => *state.llm.write().await = engine,
        Err(e) => {
            eprintln!("nova: rebuild LLM after API key save failed ({e})");
        }
    }
    Ok(())
}

/// Clears only the SQLite memory store (conversations, messages, anchors, projects, preferences).
/// Does not modify `settings.json`, API keys, or `personality.json`.
#[tauri::command]
async fn database_wipe_memories(state: State<'_, NovaState>) -> Result<(), String> {
    eprintln!("nova: ipc database_wipe_memories — SQLite user tables only");
    state
        .memory
        .wipe_all_user_data()
        .map_err(|e| e.to_string())?;
    Ok(())
}

/// Permanently clears SQLite memory data and resets `settings.json` / `personality.json` to defaults.
#[tauri::command]
async fn database_wipe_all(state: State<'_, NovaState>) -> Result<(), String> {
    eprintln!("nova: ipc database_wipe_all — SQLite + settings + personality");
    state
        .memory
        .wipe_all_user_data()
        .map_err(|e| e.to_string())?;
    state
        .settings
        .reset_to_install_defaults()
        .map_err(|e| e.to_string())?;
    state
        .personality
        .replace_all(PersonalityFile::default())
        .map_err(|e| e.to_string())?;
    ConversationMemory::set_active_personality(&*state.memory, DEFAULT_PERSONALITY_ID);
    match build_engine(&state.http, &state.settings) {
        Ok(engine) => *state.llm.write().await = engine,
        Err(e) => {
            eprintln!("nova: database_wipe_all rebuild LLM failed ({e}), using placeholder");
            *state.llm.write().await = Arc::new(PlaceholderEngine::new());
        }
    }
    Ok(())
}

#[tauri::command]
fn memory_set_active_personality(state: State<NovaState>, personality_id: String) -> Result<(), String> {
    let mut tid = personality_id.trim().to_string();
    if tid.is_empty() {
        tid = DEFAULT_PERSONALITY_ID.to_string();
    }
    eprintln!("nova: ipc memory_set_active_personality personality_id={tid} (sync persona + memory)");
    state
        .personality
        .set_active_profile_id(&tid)
        .map_err(|e| e.to_string())?;
    state.memory.set_active_personality(&tid);
    Ok(())
}

#[tauri::command]
fn memory_list_conversations(
    state: State<NovaState>,
) -> Result<Vec<StoredConversation>, String> {
    state.memory.list_conversations().map_err(|e| e.to_string())
}

#[tauri::command]
fn memory_create_conversation(state: State<NovaState>, title: String) -> Result<String, String> {
    state
        .memory
        .create_conversation(&title)
        .map_err(|e| e.to_string())
}

#[tauri::command]
fn memory_get_conversation(
    state: State<NovaState>,
    conversation_id: String,
) -> Result<StoredConversation, String> {
    state
        .memory
        .get_conversation(&conversation_id)
        .map_err(|e| e.to_string())
}

#[tauri::command]
fn memory_rename_conversation(
    state: State<NovaState>,
    conversation_id: String,
    title: String,
) -> Result<(), String> {
    state
        .memory
        .rename_conversation(&conversation_id, &title)
        .map_err(|e| e.to_string())
}

#[tauri::command]
fn delete_conversation(state: State<NovaState>, conversation_id: String) -> Result<(), String> {
    state
        .memory
        .delete_conversation(conversation_id.trim())
        .map_err(|e| e.to_string())
}

#[tauri::command]
fn memory_store_message(
    state: State<NovaState>,
    conversation_id: String,
    role: MessageRole,
    content: String,
) -> Result<(), String> {
    state
        .memory
        .store_message(&conversation_id, role, &content, None, None)
        .map_err(|e| e.to_string())
}

#[tauri::command]
fn memory_get_recent(
    state: State<NovaState>,
    conversation_id: String,
    limit: usize,
) -> Result<Vec<StoredMessage>, String> {
    state
        .memory
        .get_recent(&conversation_id, limit)
        .map_err(|e| e.to_string())
}

/// Rich briefing: transcript + Memory Anchors + projects + preferences.
#[tauri::command]
fn memory_startup_briefing(
    state: State<NovaState>,
    conversation_id: String,
) -> Result<String, String> {
    state
        .memory
        .get_startup_briefing(&conversation_id)
        .map_err(|e| e.to_string())
}

/// Same payload as [`memory_startup_briefing`]; use after bulk anchor edits.
#[tauri::command]
fn memory_update_startup_briefing(
    state: State<NovaState>,
    conversation_id: String,
) -> Result<String, String> {
    state
        .memory
        .update_startup_briefing(&conversation_id)
        .map_err(|e| e.to_string())
}

/// Insert one anchor (`conversation_id` null = global).
#[tauri::command]
fn memory_create_anchor(
    state: State<NovaState>,
    conversation_id: Option<String>,
    anchor_type: String,
    content: String,
    importance: i32,
) -> Result<String, String> {
    let ty = parse_anchor_type(&anchor_type)?;
    state
        .memory
        .create_anchor(conversation_id.as_deref(), ty, &content, importance)
        .map_err(|e| e.to_string())
}

/// Heuristic **raw** anchor extraction from recent user messages.
#[tauri::command]
fn memory_extract_anchors_from_conversation(
    state: State<NovaState>,
    conversation_id: String,
    max_anchors: usize,
) -> Result<Vec<String>, String> {
    state
        .memory
        .create_anchor_from_conversation(&conversation_id, max_anchors.max(1).min(32))
        .map_err(|e| e.to_string())
}

/// Keyword recall (semantic search when `embedding` is populated — future).
#[tauri::command]
fn memory_recall_anchors(
    state: State<NovaState>,
    query: String,
    conversation_id: Option<String>,
    limit: usize,
) -> Result<Vec<StoredAnchor>, String> {
    state
        .memory
        .recall_anchors(&query, conversation_id.as_deref(), limit.max(1).min(100))
        .map_err(|e| e.to_string())
}

/// Hybrid FTS + keyword anchor recall and optional scoped message hits.
#[tauri::command]
fn memory_recall(
    state: State<NovaState>,
    query: String,
    conversation_id: Option<String>,
    anchor_limit: Option<usize>,
    message_limit: Option<usize>,
) -> Result<MemoryRecallBundle, String> {
    let scope = conversation_id.as_deref().filter(|s| !s.trim().is_empty());
    state
        .memory
        .memory_recall(
            &query,
            scope,
            anchor_limit.unwrap_or(12).max(1).min(64),
            message_limit.unwrap_or(6).max(0).min(24),
        )
        .map_err(|e| e.to_string())
}

/// Anchors for this thread plus global (`conversation_id` NULL).
#[tauri::command]
fn memory_list_anchors(
    state: State<NovaState>,
    conversation_id: String,
    limit: usize,
) -> Result<Vec<StoredAnchor>, String> {
    state
        .memory
        .list_anchors_for_thread(&conversation_id, limit.max(1).min(200))
        .map_err(|e| e.to_string())
}

#[tauri::command]
fn memory_list_projects(state: State<NovaState>, limit: usize) -> Result<Vec<StoredProject>, String> {
    state
        .memory
        .list_projects(limit.max(1).min(100))
        .map_err(|e| e.to_string())
}

// --- Lifecycle ----------------------------------------------------------------

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    if let Ok(dir) = memory::default_data_dir() {
        eprintln!("nova: data directory {}", dir.display());
    }
    if let Ok(db) = memory::default_db_path() {
        eprintln!("nova: sqlite database {}", db.display());
    }
    eprintln!(
        "nova: sqlite profile {:?} (NOVA_DATA_DIR set: {}, NOVA_PORTABLE set: {})",
        memory::sqlite_profile_from_env(),
        std::env::var("NOVA_DATA_DIR")
            .map(|s| !s.trim().is_empty())
            .unwrap_or(false),
        std::env::var("NOVA_PORTABLE")
            .map(|s| s == "1" || s.eq_ignore_ascii_case("true"))
            .unwrap_or(false),
    );

    let memory: Arc<dyn ConversationMemory + Send + Sync> =
        Arc::new(MemoryAnchor::open_default().expect("failed to open Nova memory database"));

    let data_dir =
        memory::default_data_dir().expect("failed to resolve Nova data directory");
    let settings = Arc::new(
        SettingsManager::load(data_dir.clone(), memory.clone()).expect("failed to load settings"),
    );
    let personality = Arc::new(
        PersonalityManager::load(&data_dir).expect("failed to load personality store"),
    );

    let mut data_directory = data_dir.clone();
    if let Ok(c) = std::fs::canonicalize(&data_directory) {
        data_directory = c;
    }
    eprintln!("nova: resolved data directory {}", data_directory.display());

    let mut workspace_root = data_dir.join("workspace");
    if let Err(e) = std::fs::create_dir_all(&workspace_root) {
        eprintln!(
            "nova: warning: could not create agent workspace directory {}: {e}",
            workspace_root.display()
        );
    }
    workspace_root = std::fs::canonicalize(&workspace_root).unwrap_or(workspace_root);
    eprintln!("nova: agent workspace directory {}", workspace_root.display());

    tauri::Builder::default()
        .manage(NovaState::new(
            memory,
            settings,
            personality,
            workspace_root,
            data_directory,
        ))
        .setup(|app| {
            pulse::spawn_pulse_loop(app.handle().clone());
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            app_version,
            app_data_paths,
            reveal_data_directory,
            provider_info,
            provider_list_available,
            ollama_cloud_list_models,
            openai_list_models,
            ollama_list_local_models,
            anthropic_list_models,
            provider_switch,
            settings_get,
            settings_update,
            settings_save_api_key,
            database_wipe_memories,
            database_wipe_all,
            personality_get,
            personality_save,
            chat::chat_send_message,
            chat::chat_vision_supported,
            memory_set_active_personality,
            memory_list_conversations,
            memory_get_conversation,
            memory_create_conversation,
            memory_rename_conversation,
            delete_conversation,
            memory_store_message,
            memory_get_recent,
            memory_startup_briefing,
            memory_update_startup_briefing,
            memory_create_anchor,
            memory_extract_anchors_from_conversation,
            memory_recall_anchors,
            memory_recall,
            memory_list_anchors,
            memory_list_projects,
        ])
        .run(tauri::generate_context!())
        .expect("error while running Nova (Tauri application)");
}
