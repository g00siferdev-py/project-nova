# Installing Nova from source

This guide walks through a **complete fresh install** on a new machine. Nova is distributed as source; you build the desktop app locally with **Rust**, **Node.js**, and **Tauri 2** tooling.

---

## 1. Overview

| Step | What you get |
|------|----------------|
| Install toolchain | `rustc`, `npm`, OS libraries for WebKit/GTK |
| Clone repository | Nova source tree |
| `npm install` | Frontend dependencies |
| `npm run tauri dev` | Runnable desktop app with hot reload |
| `npm run tauri build` *(optional)* | Release installer under `src-tauri/target/release/bundle/` |

**First launch** creates a data directory on disk (see [DATA-AND-PRIVACY.md](./DATA-AND-PRIVACY.md)). Nothing is uploaded to a Nova-operated cloud service.

---

## 2. Prerequisites

### 2.1 Rust (stable)

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
source "$HOME/.cargo/env"
rustc --version   # must satisfy src-tauri/Cargo.toml rust-version
```

Update periodically:

```bash
rustup update stable
```

### 2.2 Node.js (LTS)

Use your preferred installer. Example with **nvm**:

```bash
curl -o- https://raw.githubusercontent.com/nvm-sh/nvm/v0.40.1/install.sh | bash
source "$HOME/.nvm/nvm.sh"
nvm install --lts
node --version
npm --version
```

### 2.3 Tauri / OS packages

Follow the official checklist for your platform:

**[https://v2.tauri.app/start/prerequisites/](https://v2.tauri.app/start/prerequisites/)**

#### Debian / Ubuntu (example)

```bash
sudo apt update
sudo apt install -y \
  build-essential curl wget file pkg-config libssl-dev \
  libgtk-3-dev libwebkit2gtk-4.1-dev libayatana-appindicator3-dev \
  librsvg2-dev patchelf
```

#### Fedora (example)

```bash
sudo dnf install \
  webkitgtk4.1-devel gtk3-devel libappindicator-gtk3-devel \
  librsvg2-devel openssl-devel curl wget file
```

#### macOS

Install **Xcode Command Line Tools** (see Tauri prerequisites page).

#### Windows

Install **Microsoft C++ Build Tools** and **WebView2** (see Tauri prerequisites page).

---

## 3. Clone the repository

```bash
git clone https://github.com/g00siferdev-py/project-nova.git
cd project-nova
```

If you received a ZIP archive, extract it and `cd` into the folder that contains `package.json` and `src-tauri/`.

---

## 4. Install JavaScript dependencies

From the repository root:

```bash
npm install
```

This installs React, Vite, Tauri CLI, and Tailwind tooling declared in `package.json`.

---

## 5. Run Nova (development)

**Chat, memory, settings, and providers require the Tauri shell.** The Vite-only dev server does not load the Rust backend.

```bash
npm run tauri dev
```

On first run, Nova creates:

| Path (typical Linux) | Contents |
|----------------------|----------|
| `~/.local/share/nova/nova_memory.sqlite` | Conversations, messages, anchors (SQLite) |
| `~/.local/share/nova/settings.json` | Provider choice, models, toggles |
| `~/.local/share/nova/personality.json` | Companion profiles |
| `~/.local/share/nova/.nova_crypto/` | Key-encryption material |
| `~/.local/share/nova/workspace/` | Sandboxed agent file tools (optional) |
| `~/.local/share/nova/attachments/` | Image files for vision chat (when used) |

The first compile may take several minutes while Rust dependencies build.

---

## 6. Production build (optional)

```bash
npm run tauri build
```

Artifacts appear under:

```text
src-tauri/target/release/bundle/
```

Formats depend on OS (`.deb`, `.AppImage`, `.msi`, `.dmg`, etc.).

---

## 7. First-time configuration

Open **Settings** from the chat header (right side). Tabs:

| Tab | Purpose |
|-----|---------|
| **Companion** | Personality profiles, tone, system-prompt preview |
| **Provider** | Active backend, API keys, model IDs, base URLs |
| **Tools** | Web tools, workspace files, database query toggles |
| **General** | Temperature, max tokens, **Pulse**, data paths, factory reset |

### Minimum steps for live chat

1. **Settings → Provider** — Choose **OpenAI**, **Ollama (local)**, **Ollama Cloud**, or **Anthropic** (not *Placeholder*).
2. Enter and **save** the API key if required.
3. Pick a **model** appropriate for your provider.
4. **Settings → General** or sidebar — **New chat**, then send a message.

### Optional features

| Feature | Where to enable |
|---------|-----------------|
| Web search / URL fetch / HTTPS `http_request` | **Settings → Tools** |
| Workspace file read/write | **Settings → Tools** |
| Scheduled check-ins in the open thread | **Settings → General → Pulse** |
| Photo prompts (vision models) | Attach button in composer (vision-capable model required) |

---

## 8. Data directory overrides

| Variable | Effect |
|----------|--------|
| `NOVA_DATA_DIR` | Absolute path; all app data files live in this folder |
| `NOVA_PORTABLE=1` | Data under `{executable_dir}/data/` (USB-style layout) |
| *(unset)* | OS default application data location |

**Linux / macOS example:**

```bash
export NOVA_DATA_DIR="$HOME/NovaData"
mkdir -p "$NOVA_DATA_DIR"
npm run tauri dev
```

**One-shot:**

```bash
NOVA_DATA_DIR="$HOME/NovaData" npm run tauri dev
```

**Windows (cmd):**

```cmd
set NOVA_DATA_DIR=C:\Users\YourUser\NovaData
npm run tauri dev
```

Portable mode uses stricter SQLite durability settings (`DELETE` journal, `synchronous=FULL`).

---

## 9. npm scripts reference

| Command | Description |
|---------|-------------|
| `npm install` | Install Node dependencies |
| `npm run dev` | Vite only — **no** Rust IPC |
| `npm run build` | Typecheck + production web assets |
| `npm run tauri dev` | **Full desktop app** (recommended for development) |
| `npm run tauri build` | Release binaries and installers |

---

## 10. Troubleshooting

| Symptom | Fix |
|---------|-----|
| Chat does nothing; invoke errors | Use `npm run tauri dev`, not `npm run dev` |
| Linux linker / WebKit errors | Reinstall Tauri prerequisite packages for your distro |
| Rust compile failures | `rustup update stable`; verify `rustc --version` |
| Placeholder replies only | **Settings → Provider** — select a live backend and API key |
| Images ignored on Ollama | Use a vision model (e.g. llava, kimi); agent tools are disabled for image turns |
| Empty conversation list after restore | Quit Nova before copying `nova_memory.sqlite`; restart app |
| `no such column: image_attachment` | Update to latest `main` and restart (migration runs on open) |

---

## 11. Verify the install

```bash
cd src-tauri && cargo check && cargo test --quiet
cd .. && npm run build
```

Then run `npm run tauri dev`, send a test message, and confirm the sidebar shows your thread.

---

## 12. Next steps

- [USER-GUIDE.md](./USER-GUIDE.md) — Day-to-day usage
- [DATA-AND-PRIVACY.md](./DATA-AND-PRIVACY.md) — Encryption and local storage
- [DEVELOPMENT.md](./DEVELOPMENT.md) — Contributing and pre-push checks
