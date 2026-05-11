# Nova

<<<<<<< HEAD
Nova is a **portable AI companion platform**: a cross-platform desktop shell built with **Tauri 2**, **React 19**, **TypeScript**, and **Vite**. The Rust backend lives in `src-tauri/`; the web UI lives in `src/`. Ship one codebase to Windows, macOS, and Linux, and eventually mobile.
=======
**Nova** is a privacy-oriented, portable desktop companion for working with large language models—chat, memory, and companion personality in one local-first application.
>>>>>>> 9bfdbdd (feat: security hardening, improved provider UX (Anthropic + Ollama Cloud + dynamic models), public README)

---

## Key features

- **Memory Anchor** — Conversations, messages, anchors, projects, and preferences live in a local database. Startup briefings and recall help the model stay grounded in what matters for each thread.
- **Customizable personalities** — Companion profiles shape tone and behavior; a live system-prompt preview reflects changes before they affect the next reply.
- **Local-first** — Chat history and memory stay on the device. API traffic goes only to the providers configured in settings (for example OpenAI or Ollama); there is no Nova-operated cloud layer in the core product.
- **Privacy-first** — API keys are stored with strong encryption; the design assumes sensitive threads and anchors should never leave the machine unless sent explicitly to a chosen provider.
- **Portable / USB-friendly** — Data directory layouts support carrying the app and its data on removable media via documented environment variables (`NOVA_DATA_DIR`, `NOVA_PORTABLE`).
- **Model-agnostic** — Multiple backends can be wired behind a shared engine interface; the UI focuses on provider selection, models, and generation parameters.

---

## Privacy and portability

All conversation content, anchors, projects, and companion configuration are stored **locally** (SQLite plus JSON alongside the chosen data directory). Nothing is uploaded to a central Nova service by default.

For a fixed data location (including portable drives), set **`NOVA_DATA_DIR`** to the folder that should hold the database, settings, and personality files. Alternatively, **`NOVA_PORTABLE=1`** uses a `data/` directory next to the executable. When neither is set, the app uses the operating system’s standard application data location.

---

## Quick start

**Prerequisites:** [Rust](https://www.rust-lang.org/tools/install) (stable toolchain), [Node.js](https://nodejs.org/) (LTS recommended), and the [Tauri desktop prerequisites](https://v2.tauri.app/start/prerequisites/) for the target platform.

From the project root after obtaining a copy of the source tree:

```bash
npm install
```

**Run the full desktop app** (required for chat, streaming, and memory—the Vite-only preview has no Rust backend):

```bash
npm run tauri dev
```

**Production build** (bundles the frontend and compiles the Tauri shell):

```bash
npm run tauri build
```

---

## How to use

- **Chat** — Select or create a conversation in the sidebar, compose a message, and send. Replies stream into the thread; history is persisted automatically.
- **Personalities** — Open the settings rail, use the **Companion** area to edit profiles and companion details, and save. The active profile can be switched from the chat header so threads stay aligned with the chosen companion.
- **Memory Anchor** — Each thread has a briefing area, anchor list, and recall tools in the sidebar. Anchors and recall augment the model context without replacing normal chat history.
- **Settings** — The **General** section covers providers, API keys, models, temperature, and token limits. **Data controls** distinguish wiping stored memories from a full factory reset (settings, personalities, and database).

---

## Tech stack

| Layer | Technologies |
|--------|----------------|
| Desktop shell | [Tauri 2](https://v2.tauri.app/) |
| UI | [React 19](https://react.dev/), [TypeScript](https://www.typescriptlang.org/), [Vite 7](https://vitejs.dev/) |
| Styling | [Tailwind CSS v4](https://tailwindcss.com/) |
| Backend | Rust (SQLite via `rusqlite`, async HTTP, structured settings and personality modules) |

---

## Roadmap and current status

Nova is in **early alpha**: core flows—streaming chat, memory, personalities, and settings—are functional, but polish, additional providers, automated tests, and further security review remain in progress.

Near-term themes include real integrations beyond the current provider set, clearer portable-data workflows in the UI, expanded automated testing, and ongoing hardening of the desktop surface.

For a deeper engineering snapshot and task backlog, see [`NOVA-STATUS.md`](./NOVA-STATUS.md).

---

## License

Nova is released under the **MIT License**. See [`LICENSE`](./LICENSE).
