# ADR-0003 — Rust crates for native services

Status: accepted · Date: 2026-09-04

Prefer small, maintained crates. Upstream-compatible *behavior* always beats crate convenience when the two conflict. Any consequential re-choice gets a new ADR.

| Concern | Crate / approach | Upstream behavior to match |
|---|---|---|
| App shell | `tauri` 2 (wry; system webviews) | Electron `BrowserWindow` lifecycle & options |
| File system | `tokio` + `std.fs` behind our `files` service; `trash` crate | `src/vs/platform/files`: encodings, atomic saves, streaming |
| Search | `grep-searcher`, `grep-regex`, `ignore` (ripgrep libraries) | ripgrep flag parity for `ISearchService` |
| Watching | `notify` + debounce/coalescing layer | recursive watches, excludes, event coalescing |
| PTY | `portable-pty` (ConPTY / unix pty) | node-pty observable behavior incl. shell integration |
| Storage | `rusqlite` (SQLite) | Electron storage schema/keys as used by `stateService` |
| Secrets | `keyring` | OS keychain, safeStorage-equivalent |
| Process exec | `tokio::process` | child_process flags, exit codes, signals |
| Async runtime | `tokio` | — |
| Serialization | `serde` / `serde_json` | wire shapes identical to upstream service events |
| Tunnel / remote | reuse upstream `cli/` crate | — |

Rules:

- No `unsafe` without a note in an ADR.
- New crates get `cgmanifest.json` entries per the repo's OSS policy.
- Crates are pinned/reviewed like any other dependency in this repo.
