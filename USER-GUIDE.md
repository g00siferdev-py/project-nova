# Nova — Comprehensive User Guide

This document describes **Nova as it exists today** (desktop app): layout, chat, Memory Anchor, settings, companion personality, data storage, and known limitations. Share it with design and product so recommendations align with the current implementation.

**Audience:** designers, PMs, and developers reviewing the live feature set.  
**Runtime:** Nova is a **Tauri 2** desktop app; full functionality requires the native shell (`npm run tauri dev` or an installed build). A browser-only Vite preview **does not** run Rust IPC, so chat, memory, and settings backends will not work there.

---

## 1. Product overview

- **What Nova is:** A **local-first AI companion** shell: chat UI + **SQLite** conversation memory (**Memory Anchor**) + configurable **LLM providers** (OpenAI, Ollama) + **Companion personality** layered into every reply.
- **Privacy stance (current):** Conversation data and memory live in **your** SQLite database on disk. API keys for cloud providers are stored **encrypted** (see §8). Core Nova does not ship your chats to Nova’s servers (there is no Nova cloud in core).
- **Visual design:** **Dark-first** UI (`dark` on `<html>`), slate/indigo palette, **Lucide** icons, **Tailwind CSS v4**.

---

## 2. Application layout (three panels)

| Region | Component | Purpose |
|--------|-----------|---------|
| **Left** | `ConversationSidebar` | Conversation list, Memory Anchor panel (briefing, anchors, search). |
| **Center** | `ChatMain` | Active thread title, subtitle/context line, message stream, composer, **Settings** toggle. |
| **Right** | `SettingsPanel` | Collapsible rail: **General** and **Companion** tabs. |

- **Settings** opens/closes via the header button (“Settings” / “Hide”). When closed, the rail animates to zero width.
- **Chat header subtitle:** Truncated **Memory Anchor startup briefing** text for the active thread (or a fallback line about local SQLite).

---

## 3. Conversations (left sidebar — top)

### 3.1 Conversation list

- Threads are stored in SQLite (`conversations` table).
- Each row shows **title** and **last updated** timestamp.
- **Active** thread is visually highlighted.

### 3.2 Actions

| Action | Behavior |
|--------|----------|
| **New chat** | Creates a new conversation (default title like “New chat”), selects it, loads empty history. |
| **Select thread** | Switches active conversation; loads messages + Memory Anchor context for that thread. |
| **Rename** | Inline edit (pen icon): commit on Enter or blur; updates title in DB. |
| **Delete** | Removes the conversation and its messages (cascade). Anchors tied only to that thread may be cleared or orphaned per schema; design should assume **destructive** delete. |

### 3.3 Loading states

- **List loading:** Spinner while conversations load.
- **Thread loading:** Center chat shows overlay “Loading history & context…” while the active thread’s messages and briefing load.

---

## 4. Chat (center)

### 4.1 Message list

- **User** messages: right-aligned style, label “You”.
- **Assistant** messages: left-aligned style, label “Nova” (companion display name in copy may follow personality settings).
- Empty state explains messages are stored in **local SQLite**.

### 4.2 Sending messages

- Composer clears after send; **duplicate sends** blocked while sending or thread loading.
- **Optimistic UI:** User bubble appears immediately with a temporary id; then `chat_send_message` runs.
- **Streaming:** Server emits Tauri events; UI shows “Thinking…” then streams tokens into a live assistant bubble until `done`.

### 4.3 Errors

- Global error strip (amber) for IPC / network / provider failures.
- **Important:** If the app is not run under Tauri, users see errors about invoking `chat_send_message` / memory commands.

### 4.4 What happens on send (backend, for design context)

1. User message is **persisted** to SQLite for the active conversation.
2. **Startup briefing** is built (recent transcript snippet + anchors + projects + prefs for that thread).
3. **Automatic memory recall** may run for certain questions: hybrid search across **anchors and past messages (all threads)** can append a **“Retrieved memory (auto)”** block to the briefing (bounded size). *(stderr logs in dev builds help verify this.)*
4. **Personality** system prompt (if configured) is **prepended**; then Memory & session context (briefing + optional recall).
5. Recent transcript turns are sent as user/assistant messages; the model streams the reply; the assistant reply is **saved** to SQLite.

---

## 5. Memory Anchor (left sidebar — bottom)

Memory Anchor is Nova’s **structured long-term memory** on top of raw chat rows.

### 5.1 Layers (conceptual)

| Layer | Meaning in UI / data |
|-------|----------------------|
| **Raw anchors** | Heuristic snippets auto-derived from user text (e.g. after **Extract raw anchors**). |
| **Curated / fact / insight** | Types stored in DB for higher-signal notes (future UX may expand editing). |
| **Global vs thread** | Anchors can belong to a **conversation** or be **global** (`conversation_id` null). |

### 5.2 Startup briefing (read-only panel)

- Large text area shows the **composed briefing** used as part of the model’s system context: recent transcript excerpts, anchors, active projects, non-secret preferences.
- Refreshes when you switch threads or after chat actions that refresh sidebar context.

### 5.3 Thread anchors list (“Recent anchors”)

- Shows anchors for the **current thread** (plus global), sorted with **newest first** by creation time (up to a capped count in UI).
- Displays **type** badge, **importance**, and **content**.

### 5.4 Extract raw anchors

- Button runs **heuristic extraction** on recent **user** messages in the active thread and inserts **raw** anchors (deduped).
- Use when users want durable snippets without manual copy.

### 5.5 Hybrid recall search

- Search field + button: runs **`memory_recall`** (FTS + keyword-style retrieval) with optional **thread scope** from the UI.
- Results can include **anchors** and **matching messages** (cross-thread hits may show thread title when returned from backend).
- Errors display under the search instead of failing silently.

---

## 6. Settings — General tab

Open **Settings** from the chat header, then **General**.

### 6.1 Appearance

- **Dark mode** card: informational (default dark theme); not a live light/dark toggle in current builds.

### 6.2 Provider

- **Active backend** dropdown lists: **Placeholder (offline)**, **OpenAI**, **Ollama (local)**, **Anthropic (planned)**.
- Changing provider calls **`provider_switch`** and refreshes settings.
- **Placeholder:** deterministic local message; no network.
- **Anthropic:** still a **placeholder** in backend (planned); UI may expose model field + key for future use.

### 6.3 OpenAI

| Control | Purpose |
|---------|---------|
| **Base URL** | OpenAI-compatible API root (debounced save). |
| **Model** | Model id string (debounced). |
| **API key** | Password field + **Save OpenAI API key**; stored encrypted; indicator shows saved vs not set. |

### 6.4 Ollama

| Control | Purpose |
|---------|---------|
| **Base URL** | Default `http://127.0.0.1:11434` style host (debounced). |
| **Model** | Model name as Ollama expects (debounced). |

### 6.5 Anthropic (planned)

- **Model id** field (debounced).
- **API key** save button (same pattern as OpenAI) for when the backend is implemented.

### 6.6 Generation

| Control | Purpose |
|---------|---------|
| **Temperature** | Slider `0.0`–`2.0` (debounced) with live numeric readout. |
| **Max input tokens** | **Dropdown presets:** Use model default (recommended), 4k–200k steps. Controls **generation budget** / reply cap; backend may clamp per provider. **Immediate save** on change (not debounced like other fields). Legacy saved values appear as a one-off option until user picks a preset. |

### 6.7 About

- Short text on **local storage** and **AES-GCM** key handling.
- **Read backend version** button loads `app_version` from Rust.

---

## 7. Settings — Companion tab

File on disk: **`personality.json`** (alongside other app data).

### 7.1 Current profile header

- Prominent **“Current profile: …”** line + **Active** badge + companion name used in chat.

### 7.2 Profile management

- **Switch profile** dropdown: changing selection **immediately** loads that profile’s fields into the form (options may mark “editing” on active).
- **New blank profile** / **Delete this profile** (delete disabled when only one profile remains).

### 7.3 Form fields (per profile)

| Field | Role |
|-------|------|
| **Profile name** | Preset label in lists. |
| **Companion name** | In-character name used in generated system prompt. |
| **Core personality** | Long-form persona. |
| **Tone of voice** | Short style line. |
| **Background story / role** | Setting / roleplay context. |
| **Core values / principles** | Guiding principles. |
| **Relationship style** | e.g. friend, mentor. |
| **Special instructions / quirks** | Boundaries, habits. |
| **Avatar description** | Optional; reserved for future visuals. |

### 7.4 Live system prompt preview

- Read-only preview of the **exact** companion block built from fields (updates live as you type).

### 7.5 Saving (sticky footer)

| Button | Behavior |
|--------|----------|
| **Save changes** | Persists edits to the **currently active** profile on disk. |
| **Save as new profile** | Prompts for a new name; clones current form into a **new** profile and saves (avoids overwriting others). |

---

## 8. Data, paths, and security (reference)

### 8.1 Where data lives

| Variable / mode | Effect |
|-----------------|--------|
| **`NOVA_DATA_DIR`** set | SQLite at `{dir}/nova_memory.sqlite`; `settings.json`, `personality.json`, `.nova_crypto/` alongside. |
| **`NOVA_PORTABLE=1`** | Data under `{exe_dir}/data/` (portable / USB style layout). |
| **Default** | OS app data directory for **Nova** (see `directories` crate layout). |

### 8.2 SQLite profiles

- **Desktop:** WAL journal, `synchronous=NORMAL`.
- **Portable / custom dir:** `DELETE` journal, `synchronous=FULL`.

### 8.3 Secrets

- **API keys:** Encrypted (AES-256-GCM) with key material derived from **Argon2id** + persisted salt + **OS keyring** where available, with on-disk **`.nova_crypto/ikm`** as canonical input keying material (see technical docs / `NOVA-STATUS.md`).
- **Preferences** table may mirror non-secret provider prefs; secrets are not stored in SQLite prefs after migration.

---

## 9. Known limitations & honest gaps

| Topic | Status |
|-------|--------|
| **Anthropic** | UI + key slot exist; backend is still a **placeholder** (errors if selected for real chat). |
| **Light theme** | Not implemented; dark is default. |
| **Browser-only dev** | No Tauri IPC — chat/memory/settings fail by design. |
| **Semantic embeddings** | Schema supports `embedding` blobs; hybrid recall is **keyword + FTS**-driven today; true vector similarity is a future step. |
| **Projects UI** | Projects exist in DB and briefing; **no dedicated projects screen** yet. |
| **Mobile** | Desktop-first; mobile entry exists in Rust but UX is not the focus of this guide. |

---

## 10. Quick reference — “What exists for UX review?”

- [x] Three-panel layout (sidebar / chat / settings)
- [x] Multi-conversation chat with rename & delete
- [x] Streaming assistant replies + error handling
- [x] Memory Anchor briefing + anchors list + extract + hybrid search
- [x] Automatic cross-thread memory recall (heuristic) injected into model context
- [x] General settings: providers, keys, models, URLs, temperature, max tokens, about/version
- [x] Companion settings: multi-profile personality + live preview + dual save actions (sticky footer)
- [x] Local SQLite + encrypted settings keys + portable env vars

---

## 11. Document maintenance

- When you add or remove a user-visible control, update this guide and consider a short **changelog** section or link to `NOVA-STATUS.md` for engineering depth.
- **Design team:** Use §2–§7 for screen inventory; use §8–§9 for constraints and honesty about roadmap vs shipped.

---

*Generated for the Nova repository. For build commands and repo layout, see `README.md`; for implementation status, see `NOVA-STATUS.md`.*
