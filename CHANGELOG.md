# Changelog

All notable changes to this project are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).

## [Unreleased]

### Added

#### Chat vision (image attachments)

- **Composer** — Attach JPEG, PNG, WebP, or GIF from the chat input; preview before send.
- **Storage** — Images saved under `{data_dir}/attachments/{conversationId}/`; paths stored in SQLite (`image_attachment`, `image_mime`).
- **Providers** — Multimodal payloads for OpenAI (`image_url`), Anthropic (image blocks), Ollama (`images` array).
- **`chat_vision_supported`** IPC — UI disables attach when the active model is not vision-capable.
- **Asset protocol** — Tauri config enables local attachment display via `convertFileSrc`.

#### Pulse (scheduled companion check-ins)

- **In-thread execution** — Pulse runs `execute_chat_turn` on the **sidebar-selected** conversation (same SQLite transcript, briefing, streaming as manual chat).
- **Settings** — `pulseEnabled`, `pulseIntervalMinutes`, `pulseInstructions`, `pulseConversationId` in `settings.json`.
- **Events** — `pulse:tick` emitted to the UI after each run.

#### Documentation

- **`docs/`** suite — [INSTALL](./docs/INSTALL.md), [USER-GUIDE](./docs/USER-GUIDE.md), [DATA-AND-PRIVACY](./docs/DATA-AND-PRIVACY.md), [ARCHITECTURE](./docs/ARCHITECTURE.md), [DEVELOPMENT](./docs/DEVELOPMENT.md).
- **[CONTRIBUTING.md](./CONTRIBUTING.md)** — Contribution expectations.
- **README** — Links to docs; correct repository URL; privacy summary (DB not encrypted, local after build).

### Changed

- **Settings panel** — Tabs reorganized: **Companion**, **Provider**, **Tools**, **General** (Pulse under General).
- **Ollama + images** — Agent tools disabled for requests that include images (Ollama ignores `images` when `tools` are set).
- **`model_supports_vision`** — Expanded heuristics (e.g. `kimi`, `qwen`, `-vl` models).
- **`loadActiveThread`** — Loads messages first; briefing/anchor failures no longer wipe the transcript.
- **Memory migrations** — Image columns migrate on every app open (fixes v6 databases missing columns).

### Fixed

- **`get_recent`** failing on existing databases at schema v6 without image columns.
- **Pulse** calling `execute_chat_turn` with updated signature.
- **`chat_vision_supported`** command visibility for Tauri handler registration.

### Files touched (summary)

| Area | Files |
|------|-------|
| Vision | `src-tauri/src/attachments.rs`, `chat.rs`, `memory.rs`, `ChatMain.tsx`, `useChat.ts` |
| Pulse | `src-tauri/src/pulse.rs`, `settings.rs`, `SettingsPanel.tsx` |
| Docs | `docs/*`, `README.md`, `USER-GUIDE.md`, `CONTRIBUTING.md` |

---

## [0.1.0] — prior releases on main

### Added — Agent workspace and HTTPS tools

- Sandboxed `workspace/` tools (`workspace_read_file`, `workspace_write_file`, `workspace_list_directory`).
- **`http_request`** — HTTPS-only agent tool with custom headers and body.
- Settings: `agentWorkspaceEnabled`, `agentWebToolsEnabled`, database query toggles.

### Added — Web agent tools

- `web_search`, `fetch_url` with SSRF guards.

### Added — Core platform

- Tauri 2 + React 19 chat UI with streaming.
- Memory Anchor SQLite schema (v6 personality isolation).
- OpenAI, Ollama, Anthropic providers; encrypted API keys.
- Companion personality profiles.

---

## Pre-push checklist

1. `cd src-tauri && cargo check && cargo test`
2. `npm run build`
3. Smoke-test `npm run tauri dev` (chat, optional image, settings)
4. Do not commit secrets or `nova_memory.sqlite`
