//! Companion personality presets — `personality.json` in the app data directory.

use std::path::{Path, PathBuf};
use std::sync::RwLock;

use serde::{Deserialize, Serialize};
use thiserror::Error;

const FILE_VERSION: u32 = 1;

#[derive(Debug, Error)]
pub enum PersonalityError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("personality: {0}")]
    Invalid(String),
}

/// One saved personality profile (preset).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PersonalityProfile {
    pub id: String,
    /// Preset label in the UI (e.g. "Default", "Work mentor").
    pub profile_name: String,
    pub companion_name: String,
    pub core_personality: String,
    pub tone_of_voice: String,
    pub background_story: String,
    pub core_values: String,
    pub relationship_style: String,
    pub special_instructions: String,
    #[serde(default)]
    pub avatar_description: Option<String>,
}

impl Default for PersonalityProfile {
    fn default() -> Self {
        Self {
            id: "default".into(),
            profile_name: "Default".into(),
            companion_name: "Nova".into(),
            core_personality: String::new(),
            tone_of_voice: String::new(),
            background_story: String::new(),
            core_values: String::new(),
            relationship_style: String::new(),
            special_instructions: String::new(),
            avatar_description: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PersonalityFile {
    #[serde(default = "default_file_version")]
    pub version: u32,
    #[serde(default)]
    pub profiles: Vec<PersonalityProfile>,
    #[serde(default = "default_active_id")]
    pub active_profile_id: String,
}

fn default_file_version() -> u32 {
    FILE_VERSION
}

fn default_active_id() -> String {
    "default".into()
}

impl Default for PersonalityFile {
    fn default() -> Self {
        Self {
            version: FILE_VERSION,
            profiles: vec![PersonalityProfile::default()],
            active_profile_id: "default".into(),
        }
    }
}

/// Returned to the frontend (`personality_get`).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PersonalitySnapshot {
    pub file: PersonalityFile,
    pub generated_system_prompt: String,
}

pub fn build_system_prompt(p: &PersonalityProfile) -> String {
    let name = p.companion_name.trim();
    let display = if name.is_empty() { "Nova" } else { name };

    let mut out = String::from("# Companion persona\n\n");
    out.push_str(&format!(
        "You are **{display}**, the user’s AI companion. Stay in character consistently across the conversation.\n\n"
    ));

    let push_section = |buf: &mut String, title: &str, body: &str| {
        let t = body.trim();
        if t.is_empty() {
            return;
        }
        buf.push_str("## ");
        buf.push_str(title);
        buf.push('\n');
        buf.push_str(t);
        buf.push_str("\n\n");
    };

    push_section(&mut out, "Core personality", &p.core_personality);
    push_section(&mut out, "Tone of voice", &p.tone_of_voice);
    push_section(&mut out, "Background & role", &p.background_story);
    push_section(&mut out, "Core values & principles", &p.core_values);
    push_section(&mut out, "Relationship style", &p.relationship_style);
    push_section(&mut out, "Special instructions & quirks", &p.special_instructions);

    if let Some(ref av) = p.avatar_description {
        let t = av.trim();
        if !t.is_empty() {
            push_section(&mut out, "Visual / avatar note (for future use)", t);
        }
    }

    out.push_str(
        "Respect user privacy, follow their lead, and use the session context below when relevant.\n",
    );
    out.trim_end().to_string()
}

fn active_profile(file: &PersonalityFile) -> &PersonalityProfile {
    file.profiles
        .iter()
        .find(|p| p.id == file.active_profile_id)
        .or_else(|| file.profiles.first())
        .expect("personality.profiles must be non-empty")
}

pub struct PersonalityManager {
    path: PathBuf,
    inner: RwLock<PersonalityFile>,
}

impl PersonalityManager {
    pub fn load(data_dir: impl AsRef<Path>) -> Result<Self, PersonalityError> {
        let data_dir = data_dir.as_ref();
        std::fs::create_dir_all(data_dir)?;
        let path = data_dir.join("personality.json");
        let file = if path.exists() {
            let raw = std::fs::read_to_string(&path)?;
            let mut f: PersonalityFile = serde_json::from_str(&raw)?;
            if f.profiles.is_empty() {
                f.profiles.push(PersonalityProfile::default());
            }
            if !f.profiles.iter().any(|p| p.id == f.active_profile_id) {
                f.active_profile_id = f.profiles[0].id.clone();
            }
            f.version = FILE_VERSION;
            f
        } else {
            PersonalityFile::default()
        };

        let mgr = Self {
            path: path.clone(),
            inner: RwLock::new(file),
        };
        if !path.exists() {
            mgr.persist_unlocked()?;
        }
        Ok(mgr)
    }

    fn persist_unlocked(&self) -> Result<(), PersonalityError> {
        let inner = self
            .inner
            .read()
            .map_err(|_| PersonalityError::Invalid("lock poisoned".into()))?;
        let json = serde_json::to_string_pretty(&*inner)?;
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&self.path, json)?;
        Ok(())
    }

    pub fn snapshot(&self) -> Result<PersonalitySnapshot, PersonalityError> {
        let inner = self
            .inner
            .read()
            .map_err(|_| PersonalityError::Invalid("lock poisoned".into()))?;
        let generated = build_system_prompt(active_profile(&inner));
        Ok(PersonalitySnapshot {
            file: inner.clone(),
            generated_system_prompt: generated,
        })
    }

    /// Rich persona block prepended before MemoryAnchor briefing in chat.
    pub fn system_prompt_prefix(&self) -> String {
        self.inner
            .read()
            .ok()
            .map(|f| build_system_prompt(active_profile(&f)))
            .unwrap_or_default()
    }

    /// Set which profile supplies the chat persona (`system_prompt_prefix`), persist to disk.
    /// Must stay in sync with MemoryAnchor active personality for the same `profile_id`.
    pub fn set_active_profile_id(&self, profile_id: &str) -> Result<(), PersonalityError> {
        let mut id = profile_id.trim().to_string();
        if id.is_empty() {
            id = "default".into();
        }
        {
            let mut inner = self
                .inner
                .write()
                .map_err(|_| PersonalityError::Invalid("lock poisoned".into()))?;
            if !inner.profiles.iter().any(|p| p.id == id) {
                return Err(PersonalityError::Invalid(format!(
                    "unknown personality profile id: {id}"
                )));
            }
            if inner.active_profile_id == id {
                return Ok(());
            }
            inner.active_profile_id = id.clone();
        }
        eprintln!("nova: PersonalityManager set_active_profile_id -> {id} (persisting)");
        self.persist_unlocked()
    }

    pub fn replace_all(&self, mut file: PersonalityFile) -> Result<(), PersonalityError> {
        if file.profiles.is_empty() {
            return Err(PersonalityError::Invalid(
                "at least one personality profile is required".into(),
            ));
        }
        if !file
            .profiles
            .iter()
            .any(|p| p.id == file.active_profile_id)
        {
            file.active_profile_id = file.profiles[0].id.clone();
        }
        file.version = FILE_VERSION;
        {
            let mut inner = self
                .inner
                .write()
                .map_err(|_| PersonalityError::Invalid("lock poisoned".into()))?;
            *inner = file;
        }
        self.persist_unlocked()
    }
}
