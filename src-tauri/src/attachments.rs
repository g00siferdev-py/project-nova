//! Chat image attachments: on-disk storage under the Nova data directory and vision API payloads.

use std::path::{Path, PathBuf};

use base64::Engine;
use serde_json::{json, Value};

use crate::memory::StoredMessage;
use crate::provider::ChatTurn;

const MAX_IMAGE_BYTES: usize = 8 * 1024 * 1024;

/// MIME types we accept from the composer.
pub fn normalize_image_mime(mime: &str) -> Option<&'static str> {
    match mime.trim().to_lowercase().as_str() {
        "image/jpeg" | "image/jpg" => Some("image/jpeg"),
        "image/png" => Some("image/png"),
        "image/webp" => Some("image/webp"),
        "image/gif" => Some("image/gif"),
        _ => None,
    }
}

pub fn extension_for_mime(mime: &str) -> &'static str {
    match mime {
        "image/png" => "png",
        "image/webp" => "webp",
        "image/gif" => "gif",
        _ => "jpg",
    }
}

#[cfg(test)]
mod vision_tests {
    use super::model_supports_vision;

    #[test]
    fn kimi_cloud_is_vision_capable() {
        assert!(model_supports_vision("ollama_cloud", "kimi-k2.5:cloud"));
    }
}

/// Whether the active provider + model id likely supports image input.
#[must_use]
pub fn model_supports_vision(provider_id: &str, model_id: &str) -> bool {
    let p = provider_id.trim().to_lowercase();
    let m = model_id.trim().to_lowercase();
    match p.as_str() {
        "placeholder" => false,
        "openai" => {
            m.contains("gpt-4o")
                || m.contains("gpt-4-turbo")
                || m.contains("gpt-4.1")
                || m.contains("o1")
                || m.contains("o3")
                || m.contains("o4")
                || (m.contains("gpt-4") && m.contains("vision"))
        }
        "anthropic" => {
            m.contains("claude-3")
                || m.contains("claude-sonnet-4")
                || m.contains("claude-opus-4")
                || m.contains("claude-haiku-4")
        }
        "ollama" | "ollama_cloud" => {
            m.contains("llava")
                || m.contains("vision")
                || m.contains("bakllava")
                || m.contains("moondream")
                || m.contains("minicpm-v")
                || m.contains("gemma3")
                || m.contains("kimi")
                || m.contains("qwen")
                || m.contains("llama3.2-vision")
                || m.contains("multimodal")
                || m.contains("-vl")
                || m.contains("_vl")
        }
        _ => false,
    }
}

/// Decode base64 (raw or data-URL) and write under `{data_dir}/attachments/{conversation_id}/`.
pub fn save_image_attachment(
    data_dir: &Path,
    conversation_id: &str,
    mime: &str,
    base64_input: &str,
) -> Result<(String, String), String> {
    let mime = normalize_image_mime(mime).ok_or_else(|| {
        format!("unsupported image type (use JPEG, PNG, WebP, or GIF); got {mime}")
    })?;

    let payload = base64_input.trim();
    let b64 = payload
        .strip_prefix("data:")
        .and_then(|rest| rest.split_once(',').map(|(_, data)| data))
        .unwrap_or(payload);

    let bytes = base64::engine::general_purpose::STANDARD
        .decode(b64)
        .map_err(|e| format!("invalid image data: {e}"))?;

    if bytes.is_empty() {
        return Err("image file is empty".into());
    }
    if bytes.len() > MAX_IMAGE_BYTES {
        return Err(format!(
            "image too large ({} MB max)",
            MAX_IMAGE_BYTES / (1024 * 1024)
        ));
    }

    let rel_dir = format!("attachments/{conversation_id}");
    let dir = data_dir.join(&rel_dir);
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;

    let name = format!("{}.{}", uuid::Uuid::new_v4(), extension_for_mime(mime));
    let rel_path = format!("{rel_dir}/{name}");
    let abs = data_dir.join(&rel_path);
    std::fs::write(&abs, &bytes).map_err(|e| e.to_string())?;

    Ok((rel_path, mime.to_string()))
}

pub fn read_image_bytes(data_dir: &Path, rel_path: &str) -> Result<Vec<u8>, String> {
    let rel = rel_path.trim().trim_start_matches('/');
    let abs = data_dir.join(rel);
    if !abs.starts_with(data_dir) {
        return Err("invalid attachment path".into());
    }
    std::fs::read(&abs).map_err(|e| e.to_string())
}

pub fn read_image_base64(data_dir: &Path, rel_path: &str, mime: &str) -> Result<String, String> {
    let bytes = read_image_bytes(data_dir, rel_path)?;
    let b64 = base64::engine::general_purpose::STANDARD.encode(&bytes);
    Ok(format!("data:{mime};base64,{b64}"))
}

/// Absolute path for `convertFileSrc` in the webview.
pub fn absolute_attachment_path(data_dir: &Path, rel_path: &str) -> PathBuf {
    data_dir.join(rel_path.trim().trim_start_matches('/'))
}

fn build_openai_user_message(text: &str, data_dir: &Path, rel_path: &str, mime: &str) -> Result<Value, String> {
    let b64 = read_image_bytes(data_dir, rel_path)?;
    let encoded = base64::engine::general_purpose::STANDARD.encode(&b64);
    let data_url = format!("data:{mime};base64,{encoded}");

    let mut parts: Vec<Value> = Vec::new();
    if !text.trim().is_empty() {
        parts.push(json!({"type": "text", "text": text}));
    } else {
        parts.push(json!({"type": "text", "text": "Describe this image."}));
    }
    parts.push(json!({
        "type": "image_url",
        "image_url": { "url": data_url }
    }));

    Ok(json!({
        "role": "user",
        "content": parts
    }))
}

fn build_anthropic_user_message(text: &str, data_dir: &Path, rel_path: &str, mime: &str) -> Result<Value, String> {
    let b64 = read_image_bytes(data_dir, rel_path)?;
    let encoded = base64::engine::general_purpose::STANDARD.encode(&b64);

    let mut blocks: Vec<Value> = Vec::new();
    if !text.trim().is_empty() {
        blocks.push(json!({"type": "text", "text": text}));
    } else {
        blocks.push(json!({"type": "text", "text": "Describe this image."}));
    }
    blocks.push(json!({
        "type": "image",
        "source": {
            "type": "base64",
            "media_type": mime,
            "data": encoded
        }
    }));

    Ok(json!({
        "role": "user",
        "content": blocks
    }))
}

fn build_ollama_user_message(text: &str, data_dir: &Path, rel_path: &str, _mime: &str) -> Result<Value, String> {
    let b64 = read_image_bytes(data_dir, rel_path)?;
    let encoded = base64::engine::general_purpose::STANDARD.encode(&b64);
    let content = if text.trim().is_empty() {
        "Describe this image.".to_string()
    } else {
        text.to_string()
    };
    Ok(json!({
        "role": "user",
        "content": content,
        "images": [encoded]
    }))
}

/// True when a [`ChatTurn`] carries an image payload for the provider API.
#[must_use]
pub fn chat_turn_includes_image(turn: &ChatTurn) -> bool {
    if let Some(v) = &turn.ollama_message {
        if v.get("images")
            .and_then(|x| x.as_array())
            .is_some_and(|a| !a.is_empty())
        {
            return true;
        }
    }
    if let Some(v) = &turn.openai_message {
        if let Some(parts) = v.get("content").and_then(|c| c.as_array()) {
            return parts.iter().any(|p| {
                p.get("type").and_then(|t| t.as_str()) == Some("image_url")
            });
        }
    }
    if let Some(v) = &turn.anthropic_message {
        if let Some(blocks) = v.get("content").and_then(|c| c.as_array()) {
            return blocks
                .iter()
                .any(|b| b.get("type").and_then(|t| t.as_str()) == Some("image"));
        }
    }
    false
}

#[must_use]
pub fn messages_include_images(messages: &[ChatTurn]) -> bool {
    messages.iter().any(chat_turn_includes_image)
}

/// Build a provider-specific [`ChatTurn`] for a stored row (text and/or image).
pub fn chat_turn_from_stored(
    provider_id: &str,
    data_dir: &Path,
    m: &StoredMessage,
) -> Result<ChatTurn, String> {
    let role = match m.role {
        crate::memory::MessageRole::User => "user",
        crate::memory::MessageRole::Assistant => "assistant",
    };

    if role == "assistant" || m.image_attachment.is_none() {
        return Ok(ChatTurn::text(role, &m.content));
    }

    let rel = m.image_attachment.as_deref().unwrap();
    let mime = m
        .image_mime
        .as_deref()
        .and_then(normalize_image_mime)
        .unwrap_or("image/jpeg");

    let (openai_message, ollama_message, anthropic_message) = match provider_id {
        "openai" => (
            Some(build_openai_user_message(&m.content, data_dir, rel, mime)?),
            None,
            None,
        ),
        "anthropic" => (
            None,
            None,
            Some(build_anthropic_user_message(&m.content, data_dir, rel, mime)?),
        ),
        "ollama" | "ollama_cloud" => (
            None,
            Some(build_ollama_user_message(&m.content, data_dir, rel, mime)?),
            None,
        ),
        _ => (None, None, None),
    };

    Ok(ChatTurn {
        role: role.into(),
        content: m.content.clone(),
        openai_message,
        ollama_message,
        anthropic_message,
    })
}
