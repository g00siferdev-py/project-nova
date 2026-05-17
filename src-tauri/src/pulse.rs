//! Scheduled Pulse: injects settings instructions as a normal user message on the bound conversation.

use std::time::Duration;

use chrono::{SecondsFormat, Utc};
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};

use crate::chat;
use crate::NovaState;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PulseTickEvent {
    pub ok: bool,
    pub at: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub conversation_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

async fn run_pulse_tick(app: &AppHandle, state: &NovaState) {
    let at = Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true);

    let view = match state.settings.view() {
        Ok(v) => v,
        Err(e) => {
            eprintln!("nova: pulse skipped — settings: {e}");
            return;
        }
    };

    if !view.pulse_enabled {
        return;
    }

    if view.selected_provider.trim().eq_ignore_ascii_case("placeholder") {
        let _ = app.emit(
            "pulse:tick",
            PulseTickEvent {
                ok: false,
                at,
                conversation_id: None,
                summary: None,
                error: Some(
                    "Configure a live provider in Settings before using Pulse.".into(),
                ),
            },
        );
        return;
    }

    let Some(cid) = view
        .pulse_conversation_id
        .filter(|s| !s.trim().is_empty())
    else {
        let _ = app.emit(
            "pulse:tick",
            PulseTickEvent {
                ok: false,
                at,
                conversation_id: None,
                summary: None,
                error: Some(
                    "No conversation selected — open a chat thread (Pulse runs in that session)."
                        .into(),
                ),
            },
        );
        return;
    };

    let instructions = view.pulse_instructions.trim();
    let message = if instructions.is_empty() {
        format!("(scheduled reminder · {at})")
    } else {
        format!("{instructions}\n\n(scheduled · {at})")
    };

    let pid = state.personality.active_profile_id();

    match chat::execute_chat_turn(app, state, &cid, &message, &pid, None).await {
        Ok(reply) => {
            let summary = reply.trim().to_string();
            let ok = !summary.is_empty();
            let _ = app.emit(
                "pulse:tick",
                PulseTickEvent {
                    ok,
                    at,
                    conversation_id: Some(cid.clone()),
                    summary: if ok { Some(summary) } else { None },
                    error: if ok {
                        None
                    } else {
                        Some("Empty assistant reply.".into())
                    },
                },
            );
        }
        Err(e) => {
            let _ = app.emit(
                "pulse:tick",
                PulseTickEvent {
                    ok: false,
                    at,
                    conversation_id: Some(cid),
                    summary: None,
                    error: Some(e),
                },
            );
        }
    }
}

pub fn spawn_pulse_loop(app_handle: AppHandle) {
    tauri::async_runtime::spawn(async move {
        loop {
            let sleep_secs = match app_handle.try_state::<NovaState>() {
                None => 60u64,
                Some(state) => match state.settings.view() {
                    Ok(v) if v.pulse_enabled => {
                        (v.pulse_interval_minutes.max(1).min(24 * 60) as u64)
                            .saturating_mul(60)
                            .max(60)
                    }
                    Ok(_) => 30,
                    Err(_) => 60,
                },
            };

            tokio::time::sleep(Duration::from_secs(sleep_secs.max(1))).await;

            if let Some(state) = app_handle.try_state::<NovaState>() {
                if let Ok(v) = state.settings.view() {
                    if !v.pulse_enabled {
                        continue;
                    }
                    if v.selected_provider.trim().eq_ignore_ascii_case("placeholder") {
                        continue;
                    }
                    run_pulse_tick(&app_handle, &state).await;
                }
            }
        }
    });
}
