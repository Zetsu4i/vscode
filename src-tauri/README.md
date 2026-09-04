# Code - Tauri shell

Rust side of the migration. See `../ROADMAP.md`, `../AGENTS.md`, and `../docs/tauri/`.

- `src/services/<name>.rs` holds one Rust module per upstream platform service, mirroring upstream names — `services/files.rs` (Phase 2 slice A) is the first.
- `tauri.conf.json` serves `../out/vscode-web/` in the webview — produced by `npm run tauri:web` (the genuine VS Code web workbench; a placeholder `index.html` is committed there so bare `cargo check`/CI work without a frontend build).
- Compile checks run in CI (`.github/workflows/tauri.yml`) because the current dev sandbox has no crates.io egress.

Local development on a machine with Rust + webview deps installed:

```bash
npm ci                 # once, at repo root
npm run tauri:dev      # runs Tauri dev server for the shell
```
