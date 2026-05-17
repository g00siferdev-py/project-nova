# Nova documentation

Nova is a **local-first desktop AI companion** (Tauri 2 + React + Rust). Everything in this folder is written for operators, contributors, and anyone performing a **fresh install** from source.

## Start here

| Document | Audience | Contents |
|----------|----------|----------|
| [**INSTALL.md**](./INSTALL.md) | Everyone installing Nova | Prerequisites, clone, build, first-run configuration, environment variables |
| [**DATA-AND-PRIVACY.md**](./DATA-AND-PRIVACY.md) | Security-conscious users | What stays on disk, what is encrypted, what is **not** encrypted |
| [**USER-GUIDE.md**](./USER-GUIDE.md) | Daily users | UI layout, chat, memory, settings, Pulse, images |
| [**ARCHITECTURE.md**](./ARCHITECTURE.md) | Developers | Stack, data flow, key modules, IPC surface |
| [**DEVELOPMENT.md**](./DEVELOPMENT.md) | Contributors | Dev workflow, tests, pre-push checklist |

## Repository root files

| File | Purpose |
|------|---------|
| [README.md](../README.md) | Project overview and quick start |
| [CHANGELOG.md](../CHANGELOG.md) | Notable changes by release |
| [NOVA-STATUS.md](../NOVA-STATUS.md) | Engineering status and backlog |
| [LICENSE](../LICENSE) | MIT License |

## Important privacy note

After you build and run Nova, **all conversation data lives on your machine** under the application data directory. **API keys** are encrypted at rest. The **SQLite database** (`nova_memory.sqlite`) that stores chats, anchors, and metadata is **not encrypted**—see [DATA-AND-PRIVACY.md](./DATA-AND-PRIVACY.md) for details and mitigations.

## Support matrix (current alpha)

| Requirement | Version / notes |
|-------------|-----------------|
| Rust | **1.77+** (`rust-version` in `src-tauri/Cargo.toml`) |
| Node.js | **LTS** (18 or 20 recommended) |
| Desktop OS | Linux, macOS, Windows (see [Tauri prerequisites](https://v2.tauri.app/start/prerequisites/)) |
| Runtime | **`npm run tauri dev`** or a release bundle—not `npm run dev` alone |

---

*Documentation version aligns with app **0.1.0** (early alpha). Update these files when user-visible behavior changes.*
