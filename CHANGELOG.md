# Changelog

All notable changes to this project are documented in this file.

The format is inspired by [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).

## [Unreleased]

### Added

#### Agent workspace (sandboxed file access)

- **`workspace_read_file`** — Read UTF-8 text from a path **relative** to the app workspace root. Optional `max_bytes` cap.
- **`workspace_write_file`** — Create or overwrite a UTF-8 text file; parent directories are created as needed.
- **`workspace_list_directory`** — Non-recursive listing of a relative directory (use `"."` for the workspace root).
- **Path rules** — Relative paths only, forward slashes, no `..` segments. Resolved paths are checked with `canonicalize` so symlinks cannot escape the workspace.
- **Settings** — `agentWorkspaceEnabled` (default `false`), exposed in the React settings panel as **Allow workspace file tools for the assistant**. Same provider gate as web tools (OpenAI, Ollama, Anthropic).
- **Runtime directory** — `{data_directory}/workspace` is created on app startup (`create_dir_all` in Tauri `run()`), not at compile time. `NovaState` holds canonical `workspace_root` for tool execution.

#### Agent HTTPS `http_request`

- **`http_request`** — Custom **HTTPS-only** HTTP client for the model: optional `method` (`GET` | `POST` | `PUT` | `DELETE` | `PATCH`, default `GET`), optional `headers` (object of string values), optional string `body` (e.g. stringified JSON for APIs).
- **Response shape** — Tool returns a JSON **string** with `status`, `statusText`, `headers`, and `body` (including 4xx/5xx; no `error_for_status` abort). Large response bodies are truncated for LLM context.
- **Security** — Same host allowlist as `fetch_url` (SSRF guard); **no `http://`**; no userinfo in URLs (tokens go in headers). Header names/values reject CR/LF/NUL; `HeaderName` / `HeaderValue` validation; caps on header count and sizes. Supports **`Authorization: Bearer …`** and other standard headers.
- **Operational** — 30s timeout; clear errors for timeouts, connection failures, and invalid input; extra guidance in `statusText` for **429** and selected 5xx codes; `Retry-After` preserved in response headers when the server sends it.

#### IPC and settings schema

- **`AppDataPaths.workspaceDirectory`** — String path to `{dataDirectory}/workspace` from `app_data_paths` (for UI/debug).
- **Settings file / view / patch** — `agent_workspace_enabled` persisted with serde camelCase as `agentWorkspaceEnabled` in JSON to the frontend.

### Changed

- **`chat.rs`** — Non-streaming tool loop runs when **either** web tools **or** workspace tools are enabled (merged `ToolDefinition` list). `agent_complete_with_tools` passes optional `workspace_root` into `run_builtin_tool` for workspace tool dispatch.
- **`run_builtin_tool`** — Signature extended with `workspace_root: Option<&Path>` for workspace tools; web and HTTP tools ignore it.

### Frontend

- **`SettingsPanel.tsx`** — `agentWorkspaceEnabled` toggle and copy; `AppDataPaths` type includes `workspaceDirectory`; workspace path shown when data paths load.

### Tests (Rust)

- Workspace path resolution rejects `..` and accepts normalized `./` segments.
- `http_request` header parsing rejects CRLF injection; HTTPS validator rejects plain `http`; Bearer-shaped header accepted.

### Files touched (this release batch)

| File | Role |
|------|------|
| `src-tauri/src/agent_tools.rs` | Workspace tools, `http_request`, URL/header validation, `run_builtin_tool` dispatch, unit tests |
| `src-tauri/src/chat.rs` | Merged tools, `agent_complete_with_tools`, `workspace_root` wiring |
| `src-tauri/src/lib.rs` | `NovaState.workspace_root`, startup `create_dir_all` + canonicalize, `AppDataPaths.workspace_directory` |
| `src-tauri/src/settings.rs` | `agent_workspace_enabled` in file, view, patch, getter |
| `src/components/settings/SettingsPanel.tsx` | Workspace toggle, types, workspace path hint |

---

### Suggested commit message (for GitHub)

**Title:** `feat(agent): workspace file tools, https http_request, and settings`

**Body:**

```
- Add sandboxed workspace under {data_dir}/workspace (runtime mkdir).
- Tools: workspace_read_file, workspace_write_file, workspace_list_directory.
- Add http_request (HTTPS only, custom headers/method/body, SSRF guards).
- Settings + UI: agentWorkspaceEnabled; AppDataPaths.workspaceDirectory.
- Merge web + workspace tool definitions in chat tool loop.
```

### Pre-push checklist

1. `cd src-tauri && cargo check` (and `cargo test agent_tools` if you change Rust).
2. `npm run build` for the frontend.
3. Smoke-test: enable web tools and/or workspace tools, send a chat message with a supported provider.
4. **Do not commit secrets** — API keys and Bearer tokens belong in settings or env, not in docs or commits.
