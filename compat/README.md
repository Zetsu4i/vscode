# `compat/` — Electron ⇄ Tauri compatibility contracts

Per `AGENTS.md` ("Before Replacing Any Electron/Node Code"), every native
service must have its IPC surface documented and pinned by a contract test
*before* the Rust replacement lands.

## Layout

| Path | Purpose |
| --- | --- |
| `ipc-contract.json` | Machine-readable catalogue of every `vscode:` channel, its direction, owning service, and porting phase. |
| `tests/` | Contract tests. They assert the Rust shell answers each channel with the same shape Electron did. |

## Rules

1. A channel may not be implemented in Rust until it appears in
   `ipc-contract.json` with a documented payload.
2. A channel may not be marked `implemented` until its contract test passes.
3. The legacy Electron implementation stays in the tree until then
   (hard constraint 5).

## Running

```bash
# Rust-side contract assertions
cd src-tauri && cargo test

# Shell self-check against a real workbench build
cargo run -- --self-check --workbench-dir ../out-vscode-min
```
