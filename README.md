<img width="392" height="584" alt="Nova Logo" src="https://github.com/user-attachments/assets/82f4226e-2dbb-45f2-b4f5-e0f7912655af" />

# Nova

**Nova** is a privacy-oriented, portable desktop companion for large language models: chat, long-term memory, optional **web search and URL fetch** tools for the assistant (when enabled), and customizable companion personalities in one local-first application.

---

## Key features

- **Memory Anchor** — Conversations, messages, anchors, projects, and preferences stay in a local SQLite database. Startup briefings and hybrid recall help each thread stay grounded in what matters.
- **Optional agent tools** — When turned on in Settings, supported providers can call built-in **`web_search`** (DuckDuckGo) and **`fetch_url`** (size-capped, SSRF-guarded HTTP(S)) from the chat pipeline. Off by default; traffic goes only to the endpoints those tools use plus your configured LLM provider.
- **Customizable personalities** — Companion profiles shape tone and behavior, with a live system-prompt preview before the next reply.
- **Local-first** — Chat history and memory remain on the device. Nova does not operate a central cloud service for conversations or memory.
- **Privacy-first** — API keys are stored encrypted on disk. Messages and anchors are not sent anywhere except to the provider you configure (and to DuckDuckGo / target URLs only if agent web tools are enabled).
- **Portable layouts** — Optional environment variables (`NOVA_DATA_DIR`, `NOVA_PORTABLE`) support fixed data locations, including removable drives.
- **Model-agnostic** — OpenAI, Ollama (local and cloud), Anthropic, or an offline placeholder; shared engine interface for chat and tools.

<img width="1920" height="1053" alt="NovaTest01" src="https://github.com/user-attachments/assets/c6b01618-6ee5-4b0f-9b24-6cc34518e274" />

---

## Install from source (copy-paste)

These steps assume a fresh machine with **Git** installed. Run everything from a terminal.

### 1. Install prerequisites

**Rust (stable)** — required to build the Tauri backend:

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
source "$HOME/.cargo/env"
rustc --version
```

**Node.js (LTS)** — use your preferred method; example with **nvm**:

```bash
curl -o- https://raw.githubusercontent.com/nvm-sh/nvm/v0.40.1/install.sh | bash
source "$HOME/.nvm/nvm.sh"
nvm install --lts
node --version
npm --version
```

**Tauri / OS packages** — follow the official checklist for your OS, then install the packages it lists:

- **Docs:** [https://v2.tauri.app/start/prerequisites/](https://v2.tauri.app/start/prerequisites/)

**Debian / Ubuntu (common dependencies)** — adjust if your distro uses different package names:

```bash
sudo apt update
sudo apt install -y \
  build-essential curl wget file pkg-config libssl-dev \
  libgtk-3-dev libwebkit2gtk-4.1-dev libayatana-appindicator3-dev \
  librsvg2-dev patchelf
```

**Fedora (example):**

```bash
sudo dnf install webkitgtk4.1-devel gtk3-devel libappindicator-gtk3-devel librsvg2-devel openssl-devel curl wget file
```

**macOS:** Xcode Command Line Tools (the Tauri prerequisites page explains how to install them).

**Windows:** Microsoft C++ Build Tools and WebView2 (see the same Tauri prerequisites page).

### 2. Clone the repository

```bash
git clone https://github.com/YOUR_GITHUB_USERNAME/Nova.git
cd Nova
```

Replace `YOUR_GITHUB_USERNAME` with the account or organization that hosts your fork (or upstream, if you have clone access).

If you already have the repo as a ZIP, unpack it and `cd` into the folder that contains `package.json` and `src-tauri/`.

### 3. Install JavaScript dependencies

```bash
npm install
```

### 4. Run the desktop app (development)

Chat, streaming, SQLite memory, and settings **require** the Tauri shell (not the Vite-only dev server):

```bash
npm run tauri dev
```

The first launch creates local data under the default app data directory (or under `NOVA_DATA_DIR` / portable layout if you set those — see below).

### 5. Production build (optional)

```bash
npm run tauri build
```

Installable artifacts appear under `src-tauri/target/release/bundle/` (format depends on the OS: `.deb`, `.AppImage`, `.msi`, `.dmg`, etc.).

### 6. First-time configuration in the app

1. Open **Settings → General**, choose a **provider** (e.g. OpenAI or Ollama), pick a **model**, and save your **API key** if that provider requires one.
2. Optional: enable **Allow web tools for the assistant** if you want the model to use **`web_search`** and **`fetch_url`** (OpenAI, Ollama, and Anthropic when that path is active). Requests are made from your machine; local/private URLs are blocked for `fetch_url`.
3. Open or create a conversation from the sidebar, send a message, and confirm replies stream into the thread.
4. Use **Settings → Companion** to edit personality profiles; switch the active profile from the chat UI when needed.
5. Use the sidebar **Memory Anchor** briefing, **Extract raw anchors**, and **Hybrid recall** to manage long-term snippets for the active thread.

---

## Environment variables (data directory)

| Variable | Purpose |
|----------|---------|
| `NOVA_DATA_DIR` | Absolute path to the folder where `nova_memory.sqlite`, `settings.json`, and `personality.json` should live. Useful for a dedicated disk or synced folder. |
| `NOVA_PORTABLE=1` | Store data in a `data/` directory next to the application executable (portable/USB layout). |
| *(unset)* | Use the OS default application data location. |

**Example: pin data to a folder in your home directory (Linux/macOS):**

```bash
export NOVA_DATA_DIR="$HOME/NovaData"
mkdir -p "$NOVA_DATA_DIR"
npm run tauri dev
```

**Example: one-shot run with a custom data dir:**

```bash
NOVA_DATA_DIR="$HOME/NovaData" npm run tauri dev
```

On **Windows (cmd):**

```cmd
set NOVA_DATA_DIR=C:\Users\YourUser\NovaData
npm run tauri dev
```

---

## npm scripts

| Command | Description |
|---------|-------------|
| `npm install` | Install frontend and toolchain dependencies declared in `package.json`. |
| `npm run dev` | Vite dev server only (no Rust IPC; **not** sufficient for full Nova). |
| `npm run build` | Typecheck and production-build the web assets (`tsc && vite build`). |
| `npm run tauri dev` | Run Nova as a desktop app with hot reload. |
| `npm run tauri build` | Build release binaries and installers. |

---

## Privacy and portability

Conversation content, anchors, projects, and companion configuration are stored **locally** (SQLite and JSON under the active data directory). By default, nothing is uploaded to a Nova-operated service.

<img width="392" height="584" alt="Nova" src="https://github.com/user-attachments/assets/79f68c6e-d067-4736-acce-5b5c779285fa" />

---

## Tech stack

| Layer | Technologies |
|--------|----------------|
| Desktop shell | [Tauri 2](https://v2.tauri.app/) |
| UI | [React 19](https://react.dev/), [TypeScript](https://www.typescriptlang.org/), [Vite 7](https://vitejs.dev/) |
| Styling | [Tailwind CSS v4](https://tailwindcss.com/) |
| Backend | Rust **1.77+** (SQLite via rusqlite, async HTTP, encrypted settings, MemoryAnchor, optional agent tools) |

---

## Troubleshooting

- **`npm run dev` works but chat does nothing** — Use `npm run tauri dev`. The web-only server does not load the Rust backend or SQLite.
- **Linux: linker or WebKit errors** — Re-check [Tauri v2 prerequisites](https://v2.tauri.app/start/prerequisites/) and install the listed `-dev` / `-devel` packages for your distro.
- **Rust compile errors** — Run `rustup update stable` and ensure `rustc --version` meets the `rust-version` in `src-tauri/Cargo.toml`.
- **Empty DuckDuckGo search snippets** — Broad news-style queries may return little from the instant-answer API; the implementation falls back to HTML result titles/links when needed. For full pages, the assistant can use **`fetch_url`** on a specific article URL.

---

## Project status

Nova is in **early alpha**: core flows are usable; polish, more automated tests, and continued security review remain on the roadmap. Feedback and contributions are welcome.

---

## License

Distributed under the **MIT License**. See [`LICENSE`](./LICENSE).
