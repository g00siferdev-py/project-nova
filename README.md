# Nova

Nova is a **portable AI companion platform**: a cross-platform desktop shell built with **Tauri 2**, **React 19**, **TypeScript**, and **Vite**. The Rust backend lives in `src-tauri/`; the web UI lives in `src/`. Ship one codebase to Windows, macOS, and Linux, and eventually mobile.

## Repository layout

| Path | Role |
|------|------|
| `src/` | React + TypeScript UI (Vite entry at `main.tsx`) |
| `src/components/` | Reusable UI (e.g. `layout/AppShell`) |
| `src/features/` | Vertical slices (chat, memory, tools, etc.) |
| `src/hooks/` | Shared React hooks |
| `src/lib/` | Thin clients and helpers (e.g. Tauri `invoke` re-exports) |
| `src/styles/` | Global styles |
| `src/types/` | Shared TypeScript types |
| `src-tauri/` | Rust crate, Tauri config, capabilities, icons |
| `src-tauri/src/commands/` | `#[tauri::command]` handlers registered in `lib.rs` |
| `src-tauri/capabilities/` | [Capability](https://v2.tauri.app/security/capabilities/) JSON for IPC and APIs |
| `public/` | Static assets served as-is (favicon, `nova-icon.svg`, etc.) |

Path alias **`@/*`** maps to `./src/*` (see `tsconfig.json` and `vite.config.ts`).

## Prerequisites

- [Rust](https://www.rust-lang.org/tools/install) (stable), `cargo`
- [Node.js](https://nodejs.org/) (LTS recommended) and npm
- Platform libraries for Tauri on Linux: see [Tauri prerequisites](https://v2.tauri.app/start/prerequisites/)

## Scripts

```bash
npm install          # install frontend + CLI
npm run dev          # Vite dev server (port 1420)
npm run tauri dev    # desktop app + hot reload
npm run build        # production frontend → dist/
npm run tauri build  # bundle installers with embedded dist/
```

Regenerate tray and installer icons from the square source in `public/nova-icon.svg`:

```bash
npx tauri icon public/nova-icon.svg
```

## Configuration

- **`src-tauri/tauri.conf.json`** — app id `app.nova.desktop`, window `main`, dev URL, bundle icons.
- **`src-tauri/capabilities/default.json`** — permissions for the `main` window (start with `core:default`; tighten as you add plugins).
- **`package.json` / `Cargo.toml`** — aligned naming: npm package `nova`, Rust crate `nova`, library `nova_lib`.

## Security note

Capabilities gate what the webview can invoke. Add explicit permissions when you introduce plugins (filesystem, HTTP, shell, etc.); avoid widening `core:default` more than you need.

## License

Add a license file when you choose one for Nova.
