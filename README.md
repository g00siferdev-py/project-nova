<img width="392" height="584" alt="Nova Logo" src="https://github.com/user-attachments/assets/82f4226e-2dbb-45f2-b4f5-e0f7912655af" />

# Nova

**Nova** is a privacy-oriented, portable desktop companion for large language models: chat, long-term memory, and customizable companion personalities in one local-first application.

---

## Key features

- **Memory Anchor** — Conversations, messages, anchors, projects, and preferences stay in a local database. Startup briefings and recall help each thread stay grounded in what matters.
- **Customizable personalities** — Companion profiles shape tone and behavior, with a live system-prompt preview before the next reply.
- **Local-first** — Chat history and memory remain on the device. Network traffic goes only to the AI providers enabled in settings (for example OpenAI or Ollama). Nova does not operate a central cloud service for conversations or memory.
- **Privacy-first** — API keys are protected with strong encryption. Messages and anchors are not sent anywhere except to the provider the installation is configured to use.
- **Portable layouts** — Optional environment variables (`NOVA_DATA_DIR`, `NOVA_PORTABLE`) support fixed data locations, including removable drives.
- **Model-agnostic** — Multiple provider backends share a common engine; the interface focuses on choosing providers, models, and generation parameters.
<img width="1920" height="1053" alt="NovaTest01" src="https://github.com/user-attachments/assets/c6b01618-6ee5-4b0f-9b24-6cc34518e274" />


---

## Privacy and portability
<img width="392" height="584" alt="Nova" src="https://github.com/user-attachments/assets/79f68c6e-d067-4736-acce-5b5c779285fa" />

Conversation content, anchors, projects, and companion configuration are stored **locally** (SQLite and JSON under the active data directory). By default, nothing is uploaded to a Nova-operated service.

- **`NOVA_DATA_DIR`** — Set to a folder where the database, settings, and personality files should live (ideal for a dedicated disk or USB layout).
- **`NOVA_PORTABLE=1`** — Uses a `data/` directory next to the application executable.
- **Default** — If neither option is set, data follows the operating system’s usual application data location for desktop apps.

---

## Quick start / how to run

**Prerequisites**

- [Rust](https://www.rust-lang.org/tools/install) (stable toolchain) and `cargo`
- [Node.js](https://nodejs.org/) (LTS recommended) and npm
- [Tauri prerequisites](https://v2.tauri.app/start/prerequisites/) for the target platform (Windows, macOS, or Linux)

**Install dependencies** (from the project root):

```bash
npm install
```

**Run the desktop app** — Full chat, streaming, and memory require the Tauri shell:

```bash
npm run tauri dev
```

A Vite-only web preview does not include the Rust backend; chat and persistence need the command above.

**Production build** — Produces installable artifacts with the bundled frontend:

```bash
npm run tauri build
```

### First steps in the app

- Open or create a conversation from the sidebar, send a message, and watch the assistant reply stream into the thread.
- Use **Settings → Companion** to adjust companion profiles and save; switch the active profile from the chat header when needed.
- Use the sidebar briefing, anchors, and recall tools to enrich context for the active thread.
- Under **Settings → General**, configure providers, keys, models, temperature, and limits; data controls support wiping memories only or a full local reset.

---

## Tech stack

| Layer | Technologies |
|--------|----------------|
| Desktop shell | [Tauri 2](https://v2.tauri.app/) |
| UI | [React 19](https://react.dev/), [TypeScript](https://www.typescriptlang.org/), [Vite 7](https://vitejs.dev/) |
| Styling | [Tailwind CSS v4](https://tailwindcss.com/) |
| Backend | Rust (SQLite, async HTTP, settings and personality modules) |

---

## Project status

Nova is in **early alpha**: core flows are usable, while polish, more providers, automated tests, and continued security review remain on the roadmap. Feedback and contributions are welcome as the project moves toward a stable release.

---

## License

Distributed under the **MIT License**. See [`LICENSE`](./LICENSE).
