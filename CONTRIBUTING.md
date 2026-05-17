# Contributing to Nova

Thank you for your interest in Nova. This project is in **early alpha**; contributions are welcome with the understanding that APIs and UX may change.

## Before you start

1. Read [docs/INSTALL.md](./docs/INSTALL.md) and get `npm run tauri dev` running.
2. Read [docs/DEVELOPMENT.md](./docs/DEVELOPMENT.md) for the pre-push checklist.
3. Read [docs/DATA-AND-PRIVACY.md](./docs/DATA-AND-PRIVACY.md) — do not commit user databases, settings, or API keys.

## Pull request expectations

- Focused changes with a clear description
- `cargo check` and `cargo test` pass in `src-tauri/`
- `npm run build` passes
- User-visible changes noted in `CHANGELOG.md` under `[Unreleased]`
- Documentation updated in `docs/` when behavior changes

## Code style

- Rust: `cargo fmt` before commit
- TypeScript: match existing patterns in `src/`
- New IPC commands: register in `lib.rs` and `permissions/nova-invoke-allowlist.toml`

## Questions

Open a GitHub issue on [g00siferdev-py/project-nova](https://github.com/g00siferdev-py/project-nova) for bugs and feature discussion.

## License

By contributing, you agree that your contributions will be licensed under the [MIT License](./LICENSE).
