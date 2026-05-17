# Nova — project status

**Version:** 0.1.0 (early alpha)  
**Repository:** [g00siferdev-py/project-nova](https://github.com/g00siferdev-py/project-nova)

---

## Executive summary

Nova is a **local-first desktop AI companion** (Tauri 2 + React + Rust). Conversations and memory live in **SQLite on your machine**. **API keys are encrypted**; the **database file is not encrypted**. There is no Nova cloud for chat storage.

**Documentation:** See **[docs/README.md](./docs/README.md)** for the full guide index, including **[fresh install instructions](./docs/INSTALL.md)**.

---

## What works today

| Area | Status |
|------|--------|
| Streaming chat | Per-thread history, rename/delete, optimistic UI |
| Memory Anchor | Anchors, briefings, hybrid recall, extract, personality scoping |
| Providers | OpenAI, Ollama local, Ollama Cloud, Anthropic, placeholder |
| Companion | Multi-profile `personality.json`, header dropdown sync |
| Agent tools | Web, workspace, optional `database_query` (opt-in) |
| Pulse | Scheduled ticks in **open sidebar thread** |
| Vision | Image attach + multimodal provider payloads |
| Settings | Four tabs: Companion, Provider, Tools, General |
| Data controls | Memory wipe, factory reset, `NOVA_DATA_DIR` / portable |
| Docs | `docs/` install, privacy, user guide, architecture, development |

---

## Privacy and storage (explicit)

| Asset | Encrypted at rest? | Location |
|-------|-------------------|----------|
| `nova_memory.sqlite` | **No** | App data directory |
| Chat image files | **No** | `{data_dir}/attachments/` |
| `personality.json` | **No** | App data directory |
| API keys | **Yes** (AES-256-GCM) | `settings.json` + `.nova_crypto/` |

Details: **[docs/DATA-AND-PRIVACY.md](./docs/DATA-AND-PRIVACY.md)**

---

## Technical snapshot

- **`NovaState`** — memory, settings, personality, LLM engine, HTTP client, data paths
- **`chat_send_message`** → `execute_chat_turn` → briefing + recall + `run_chat_completion`
- **`attachments.rs`** — save images, build provider-specific `ChatTurn` JSON
- **`pulse.rs`** — background loop; same chat path as manual send
- **Schema v6** + idempotent image column migration on every open

---

## Recent improvements (unreleased on branch)

- Pulse runs in the active conversation (not isolated ghost API calls)
- Vision attachments end-to-end with Ollama tool bypass for image turns
- Memory migration fix for v6 databases without image columns
- Comprehensive `docs/` and README refresh

---

## Backlog (high level)

1. **Database encryption** — SQLCipher or OS-level guidance (not shipped)
2. **Tauri capability tightening** — audit allowlists as surface grows
3. **Automated CI** — `cargo test`, `npm run build`, smoke tests
4. **Projects UI** — data exists; no dedicated screen yet
5. **Semantic embeddings** — column reserved; recall is FTS + keyword today
6. **In-app data directory picker** — portable/USB UX

---

## Build verification

```bash
cd src-tauri && cargo check && cargo test
npm run build
npm run tauri dev   # manual smoke test
```

---

*Last updated with documentation suite, Pulse in-thread, and vision attachments.*
