# Data storage and privacy

Nova is designed as a **local-first** desktop companion. This document states clearly what is stored where, what is encrypted, and what is not—so you can make informed decisions before deploying Nova on a shared or portable machine.

---

## Summary

| Data | Location | Encrypted at rest? | Leaves your machine? |
|------|----------|----------------------|----------------------|
| Chat messages & memory (SQLite) | `nova_memory.sqlite` | **No** | Only if you configure a cloud LLM provider |
| Companion personalities | `personality.json` | **No** | No (local file) |
| App settings (non-secret) | `settings.json` | **No** | No |
| API keys | `settings.json` (ciphertext) + `.nova_crypto/` | **Yes** (AES-256-GCM) | Yes — sent to **your chosen** provider when chatting |
| Image attachments | `attachments/{conversationId}/` | **No** | Yes — embedded in vision API requests when you send photos |
| Agent workspace files | `workspace/` | **No** | Only via tools you enable |
| Pulse / chat traffic | In-memory + provider API | N/A | Yes — to configured LLM endpoint |

**There is no Nova-operated cloud** that stores your conversations. After the app is built and run, persistence is **entirely on your device** unless you explicitly enable tools that contact third-party URLs.

---

## Default data directory

When `NOVA_DATA_DIR` and `NOVA_PORTABLE` are unset, Nova uses the OS application data path (via the `directories` crate), typically:

| OS | Example path |
|----|----------------|
| Linux | `~/.local/share/nova/` |
| macOS | `~/Library/Application Support/Nova/` |
| Windows | `%APPDATA%\Nova\` |

### Files in the data directory

| File / folder | Purpose |
|---------------|---------|
| `nova_memory.sqlite` | SQLite database: conversations, messages, anchors, projects, preferences |
| `settings.json` | Provider, models, feature toggles, encrypted API key blobs |
| `personality.json` | Companion profiles and active profile id |
| `.nova_crypto/` | Input keying material for settings encryption |
| `workspace/` | Sandboxed directory for optional agent file tools |
| `attachments/` | Saved images from the chat composer (vision) |

The git repository **does not** contain your database or settings. Each machine starts empty until you chat or restore a backup copy of the data folder.

---

## SQLite database (`nova_memory.sqlite`)

### What it contains

- **Conversations** — thread titles, timestamps, `personality_id` scope
- **Messages** — user and assistant text; optional `image_attachment` path and `image_mime`
- **Anchors** — long-term memory snippets (raw, curated, fact, insight)
- **Projects** and **preferences** — structured context for briefings

### Encryption status

**The database file is not encrypted.** Anyone with filesystem access to your user account (or a copy of the file) can read conversation content with standard SQLite tools.

Nova does **not** currently offer SQLCipher or OS-level full-disk encryption. Mitigations you may use:

- Full-disk encryption (LUKS, FileVault, BitLocker)
- Restrictive file permissions on the data directory
- `NOVA_DATA_DIR` on an encrypted volume or removable drive you control
- Regular backups stored in encrypted archives

### Schema migrations

Nova runs idempotent migrations on startup (`PRAGMA user_version`). New columns (for example image attachments) are added automatically when you upgrade the app. **Back up** `nova_memory.sqlite` before major upgrades or manual maintenance.

### Personality isolation

Schema version **6** scopes conversations and anchors by `personality_id`. Switching the active companion in the UI filters which threads appear; data for other profiles remains in the same database file.

---

## Settings and API keys

### `settings.json`

Stores non-secret configuration: selected provider, model names, base URLs, temperature, max tokens, Pulse interval, agent tool toggles, etc.

### Encrypted API keys

Provider API keys are stored as **AES-256-GCM** ciphertext in `settings.json`, with key material derived using **Argon2id** and persisted salt, integrated with the OS **keyring** where available, and canonical input from `.nova_crypto/ikm`.

**Keys are not stored in plaintext** in the repository or in SQLite preference rows.

When you send a chat message, Nova decrypts the key in process memory and passes it to the configured provider over HTTPS.

---

## Network exposure

| Action | Network destination |
|--------|---------------------|
| Chat (OpenAI / Anthropic / Ollama Cloud) | Provider API you configure |
| Ollama local | `http://127.0.0.1:11434` (default) or your base URL |
| `web_search` tool | DuckDuckGo (when enabled) |
| `fetch_url` / `http_request` | URLs chosen by the model (SSRF-filtered; HTTPS-only for `http_request`) |
| Pulse | Same as normal chat — uses the open sidebar thread |

Disable agent tools in **Settings → Tools** if you want chat limited to the LLM provider only.

---

## Image attachments

When you attach a photo in chat:

1. The image is saved under `{data_dir}/attachments/{conversationId}/`.
2. A relative path is recorded in SQLite.
3. On send, the file is read and encoded for the vision-capable model API.

Images remain on disk in **unencrypted** files. The webview displays them via Tauri’s asset protocol (`convertFileSrc`).

---

## Portable and custom layouts

| Mode | Behavior |
|------|----------|
| `NOVA_DATA_DIR=/path` | All files under `/path` |
| `NOVA_PORTABLE=1` | `{exe_dir}/data/` — suitable for USB workflows |
| Default | OS app data directory |

Portable mode uses SQLite `DELETE` journal and `synchronous=FULL` for durability on removable media.

---

## Data wipe controls

In **Settings → General**:

| Control | Effect |
|---------|--------|
| **Wipe all memories** | Clears SQLite user tables; re-seeds default thread; keeps settings and personalities |
| **Factory reset** | Wipes database **and** resets `settings.json` / `personality.json` to defaults |

Always **quit Nova** before copying or restoring `nova_memory.sqlite` manually.

---

## Compliance-oriented notes

Nova is **early alpha** software. It does not implement:

- Database encryption
- Multi-user access control
- Audit logging
- Data retention policies beyond manual delete/wipe

Evaluate whether your threat model requires additional controls before storing sensitive content in Nova.

---

## Related documents

- [INSTALL.md](./INSTALL.md) — Fresh install and data directory setup
- [USER-GUIDE.md](./USER-GUIDE.md) — Feature-level usage
- [ARCHITECTURE.md](./ARCHITECTURE.md) — Implementation map
