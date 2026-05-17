<img width="392" height="584" alt="Nova Logo" src="https://github.com/user-attachments/assets/82f4226e-2dbb-45f2-b4f5-e0f7912655af" />

# Nova

**Nova** is a privacy-oriented desktop AI companion: multi-thread chat, long-term **Memory Anchor** storage, optional **agent tools**, customizable companion personalities, **Pulse** scheduled check-ins, and **vision** image attachments—all in a local-first **Tauri 2** application.

[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](./LICENSE)

**Repository:** [github.com/g00siferdev-py/project-nova](https://github.com/g00siferdev-py/project-nova)

---

## Documentation

| Guide | Description |
|-------|-------------|
| **[docs/INSTALL.md](./docs/INSTALL.md)** | **Fresh install** — prerequisites, clone, build, first-run setup |
| **[docs/USER-GUIDE.md](./docs/USER-GUIDE.md)** | Day-to-day usage — chat, memory, settings, Pulse, images |
| **[docs/DATA-AND-PRIVACY.md](./docs/DATA-AND-PRIVACY.md)** | What is stored locally; **API keys encrypted**, **database not encrypted** |
| **[docs/ARCHITECTURE.md](./docs/ARCHITECTURE.md)** | Technical overview for developers |
| **[docs/DEVELOPMENT.md](./docs/DEVELOPMENT.md)** | Dev workflow and pre-push checklist |
| [CHANGELOG.md](./CHANGELOG.md) | Release notes |
| [CONTRIBUTING.md](./CONTRIBUTING.md) | How to contribute |

---

## Privacy at a glance

| Data | Where | Encrypted? |
|------|-------|------------|
| Chats, anchors, memory | `nova_memory.sqlite` on your disk | **No** (local file) |
| API keys | `settings.json` + `.nova_crypto/` | **Yes** |
| Personalities | `personality.json` | No |

After you build and run Nova, **nothing is stored on a Nova-operated cloud**. Messages go only to the **LLM provider you configure** (and optional tool URLs if you enable agent tools). See **[docs/DATA-AND-PRIVACY.md](./docs/DATA-AND-PRIVACY.md)** for the full picture.

---

## Key features

- **Memory Anchor** — SQLite conversations, messages, anchors, projects, and preferences; hybrid FTS recall and startup briefings.
- **Companion profiles** — Multiple personalities with live system-prompt preview; per-profile thread isolation.
- **Providers** — OpenAI, Ollama (local), Ollama Cloud, Anthropic, or offline placeholder.
- **Agent tools** (opt-in) — Web search, URL fetch, HTTPS `http_request`, sandboxed workspace files, optional database query.
- **Pulse** — Timer-driven check-ins that run as **normal chat turns** in your selected sidebar thread.
- **Vision** — Attach images in the composer; multimodal payloads for supported models.
- **Portable layouts** — `NOVA_DATA_DIR` and `NOVA_PORTABLE` for custom or USB data locations.

<img width="1920" height="1053" alt="Nova screenshot" src="https://github.com/user-attachments/assets/c6b01618-6ee5-4b0f-9b24-cc34518e274" />

---

## Quick start (experienced developers)

```bash
git clone https://github.com/g00siferdev-py/project-nova.git
cd project-nova
npm install
npm run tauri dev
```

First launch creates local data under your OS app directory (or `NOVA_DATA_DIR` if set). Configure **Settings → Provider**, then start a chat.

**New to the stack?** Follow the step-by-step guide in **[docs/INSTALL.md](./docs/INSTALL.md)**.

---

## Environment variables

| Variable | Purpose |
|----------|---------|
| `NOVA_DATA_DIR` | Absolute path for `nova_memory.sqlite`, settings, personalities, workspace, attachments |
| `NOVA_PORTABLE=1` | Store data in `{executable}/data/` |
| *(unset)* | OS default application data location |

```bash
export NOVA_DATA_DIR="$HOME/NovaData"
mkdir -p "$NOVA_DATA_DIR"
npm run tauri dev
```

---

## npm scripts

| Command | Description |
|---------|-------------|
| `npm install` | Install dependencies |
| `npm run tauri dev` | **Run Nova** (desktop + Rust backend) |
| `npm run tauri build` | Release build and installers |
| `npm run build` | Frontend typecheck and Vite production build |
| `npm run dev` | Vite only — **not** sufficient for full Nova |

---

## Tech stack

| Layer | Technologies |
|-------|----------------|
| Desktop | [Tauri 2](https://v2.tauri.app/) |
| UI | React 19, TypeScript, Vite 7, Tailwind CSS v4 |
| Backend | Rust 1.77+, rusqlite, reqwest, encrypted settings |

---

## Troubleshooting

| Issue | Solution |
|-------|----------|
| Chat does nothing | Use `npm run tauri dev`, not `npm run dev` |
| Placeholder replies | Settings → Provider → live backend + API key |
| Model ignores images | Use a vision model; on Ollama, tools are off for image turns |
| Linux build errors | [Tauri prerequisites](https://v2.tauri.app/start/prerequisites/) |

More: **[docs/INSTALL.md § Troubleshooting](./docs/INSTALL.md#10-troubleshooting)**

---

## Project status

Nova is **early alpha** (0.1.0). Core flows work; security hardening, tests, and polish continue. See [NOVA-STATUS.md](./NOVA-STATUS.md) and [CHANGELOG.md](./CHANGELOG.md).

---

## License

[MIT License](./LICENSE)
