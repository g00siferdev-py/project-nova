# Nova user guide

Complete guide to the Nova desktop application as shipped in **version 0.1.0** (early alpha).

**Runtime requirement:** `npm run tauri dev` or an installed release build. Browser-only Vite preview cannot access chat, memory, or settings backends.

---

## 1. What Nova does

Nova is a **local-first AI companion**:

- Multi-thread **chat** with streaming replies
- **Memory Anchor** — SQLite-backed long-term memory (anchors, briefings, hybrid search)
- **Companion personalities** — per-profile tone and system instructions
- **Optional agent tools** — web search, URL fetch, HTTPS requests, workspace files, database query
- **Pulse** — scheduled check-ins in your **currently selected** conversation
- **Image attachments** — send photos to vision-capable models from the composer

Your data stays on your machine. See [DATA-AND-PRIVACY.md](./DATA-AND-PRIVACY.md): the **database is not encrypted**, but it is **local** after install.

---

## 2. Application layout

| Region | Component | Purpose |
|--------|-----------|---------|
| **Left** | Conversation sidebar | Thread list, Memory Anchor panel |
| **Center** | Chat | Messages, composer, companion picker, Settings toggle |
| **Right** | Settings panel | Companion · Provider · Tools · General |

Settings slides in from the right; toggle **Settings** / **Hide** in the chat header.

---

## 3. Conversations

### List and actions

- **New chat** — creates a thread for the active companion profile
- **Select** — loads history and memory context
- **Rename** — inline edit (pen icon)
- **Delete** — removes thread and messages (destructive)

### Companion header dropdown

Choose which **companion profile** receives new chats and memory scoping. Switching profiles filters the sidebar to that profile’s threads.

---

## 4. Chat

### Sending messages

- Type in the composer; **Enter** sends, **Shift+Enter** newline
- **Attach image** (camera icon) — pick JPEG, PNG, WebP, or GIF when your model supports vision
- Preview appears above the composer; **X** removes it before send
- User bubble shows immediately (optimistic UI); assistant reply streams in

### Streaming

Nova emits `chat:stream-start`, token deltas on `chat:stream`, and `done`. A “Thinking…” state shows before the first token.

### Errors

Amber banner at the top for IPC, provider, or validation errors (for example non-vision model with an image attached).

### What happens on send (backend)

1. User message saved to SQLite (text + optional image path)
2. Startup briefing built (transcript + anchors + projects + preferences)
3. Optional automatic memory recall appended for qualifying questions
4. Companion system prompt merged with briefing
5. Recent turns sent to the model (images encoded for vision APIs)
6. Assistant reply saved and streamed to the UI

---

## 5. Memory Anchor

### Startup briefing

Read-only panel in the sidebar: context Nova injects into the model (recent transcript excerpts, anchors, projects, preferences).

### Recent anchors

Anchors for the current thread plus global anchors (`conversation_id` null).

### Extract raw anchors

Heuristic extraction from recent **user** messages in the active thread.

### Hybrid recall search

Keyword + FTS search across anchors and messages; may include cross-thread hits with conversation titles.

---

## 6. Settings

Open **Settings** from the chat header. Four tabs:

### 6.1 Companion

- Switch, create, or delete personality **profiles**
- Edit companion name, tone, values, special instructions
- **Live system prompt preview**
- **Save changes** / **Save as new profile**

File on disk: `personality.json`

### 6.2 Provider

| Backend | Notes |
|---------|-------|
| **Placeholder** | Offline; no network |
| **OpenAI** | API key, base URL, model (e.g. `gpt-4o`, `gpt-4o-mini`) |
| **Ollama (local)** | Base URL (default `http://127.0.0.1:11434`), model name |
| **Ollama Cloud** | API key, cloud model (e.g. `kimi-k2.5:cloud`) |
| **Anthropic** | API key, Claude model id |

### 6.3 Tools

| Toggle | Tools enabled |
|--------|----------------|
| **Web tools** | `web_search`, `fetch_url`, `http_request` (HTTPS-only) |
| **Workspace tools** | Read/write/list under `{data_dir}/workspace` |
| **App data databases** | `database_query` on `.sqlite` in data folder |
| **Allow database writes** | INSERT/UPDATE/DELETE via `database_query` (dangerous) |

**Note:** When you send an **image** on Ollama, web/workspace tools are **disabled for that request** so the model can receive the image payload.

### 6.4 General

| Section | Purpose |
|---------|---------|
| **Generation** | Temperature, max output tokens |
| **Pulse** | Enable timer, interval (minutes), instructions; runs in **sidebar-selected** thread |
| **Data** | Reveal data folder, wipe memories, factory reset |
| **About** | Backend version |

Pulse emits `pulse:tick` events; the chat UI reloads the thread after each tick.

---

## 7. Vision (image attachments)

### Requirements

- Vision-capable model (e.g. OpenAI `gpt-4o*`, Claude 3+, Ollama llava/kimi/vision models)
- Attach button is **disabled** with a tooltip when the active model is not supported

### Storage

Images save to `{data_dir}/attachments/{conversationId}/`. Paths are stored in SQLite. **Files are not encrypted.**

### Tips

- Add a short caption (“What is in this photo?”) with the image
- For Ollama Cloud **kimi** and similar models, ensure Provider tab shows the correct model id
- If the model acts blind, check terminal logs for `nova: chat completion includes image(s)`

---

## 8. Data and privacy (essentials)

| Item | Encrypted? |
|------|------------|
| `nova_memory.sqlite` | **No** — local only |
| `personality.json`, `settings.json` (non-key fields) | **No** |
| API keys in settings | **Yes** (AES-GCM) |
| Image files in `attachments/` | **No** |

Full detail: [DATA-AND-PRIVACY.md](./DATA-AND-PRIVACY.md)

### Environment variables

| Variable | Purpose |
|----------|---------|
| `NOVA_DATA_DIR` | Custom data folder |
| `NOVA_PORTABLE=1` | Portable `data/` next to executable |

---

## 9. Known limitations

| Topic | Status |
|-------|--------|
| Database encryption | Not implemented |
| Light theme | Not implemented |
| Browser-only `npm run dev` | No backend |
| Semantic vector search | Schema ready; recall is FTS + keyword today |
| Dedicated projects UI | Projects in briefing only |
| Pulse + tools | Pulse uses normal chat path; tools follow same rules as manual send |

---

## 10. Quick reference checklist

- [x] Multi-conversation chat with streaming
- [x] Memory Anchor briefing, anchors, extract, recall
- [x] Four settings tabs (Companion, Provider, Tools, General)
- [x] Encrypted API keys; **unencrypted** local SQLite
- [x] Pulse in open thread
- [x] Image attach for vision models
- [x] Agent tools (optional)
- [x] Portable / custom data directory

---

*For installation: [INSTALL.md](./INSTALL.md). For developers: [ARCHITECTURE.md](./ARCHITECTURE.md).*
