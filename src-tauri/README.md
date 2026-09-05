# vscode-tauri (Phase 1 prototype)

Tauri v2 shell that loads the **original, unmodified** VS Code workbench build output.
See [`../ROADMAP.md`](../ROADMAP.md) Phase 1 for goals and acceptance criteria, and
[`../AGENTS.md`](../AGENTS.md) for the rules this work follows.

## How it works

```
npm run compile            →  out/   (original VS Code build output, untouched)
        │
src-tauri (cargo run)      →  loopback static server over out/  (src/server.rs)
        │
WebView2 window            →  http://127.0.0.1:<port>/vs/code/electron-browser/workbench/workbench.html
        │
initialization script      →  src/shim.js  (plays the role of Electron's preload)
```

The shim reproduces the globals the renderer expects from Electron's preload
(`src/vs/base/parts/sandbox/electron-browser/preload.ts`):

- `window.vscode` with `ipcRenderer` (validated `vscode:*` channels), `ipcMessagePort`,
  `webFrame`, `webUtils`, `process`, `context`
- `window.process` (platform/arch/env/argv/versions subset), `setImmediate`,
  `clearImmediate`, `Buffer` (minimal), `global`
- forwards IPC to Rust via Tauri's bridge and keeps a diagnostics ring buffer in
  `window.__VSCODE_TAURI_DIAGNOSTICS__` (boot / errors / ipc) for devtools inspection

Implemented Rust-side (`src/ipc.rs`):

- window-configuration channel (`vscode:*window-config*`) → builds the
  `INativeWindowConfiguration` payload Electron main normally produces
  (product.json, userEnv, NLS, splash defaults)
- `vscode:fetchShellEnv`
- `native_set_zoom_level` (webFrame zoom)
- `renderer_log` (renderer errors land in the shell log)

Every other channel currently answers `null` and is logged once — the log is the
input for Phase 2 (IPC contract extraction). Nothing in the Electron/Node tree is
modified; the Electron app keeps building in parallel.

## Running

```bash
# from the repo root (produces out/)
npm ci
npm run compile

# from src-tauri/
cargo run
```

Useful environment variables:

- `VSCODE_TAURI_OUT_DIR=<path>` — override where `out/` is discovered
- `RUST_LOG=debug` — verbose shell logging

## Known Phase 1 gaps (intentional, tracked in ROADMAP.md)

- Workbench boot will proceed only as far as unimplemented IPC allows; unimplemented
  channels are logged, not silently swallowed.
- `webUtils.getPathForFile` is a stub (drag & drop of folders will not resolve paths
  until the Phase 4 file service exists).
- Rust → renderer IPC events (`ipcRenderer.on`) are registered but nothing emits yet.
- Module loading uses the dev-mode relative-import branch
  (`VSCODE_DEV=1` + `_VSCODE_USE_RELATIVE_IMPORTS`, the same switch
  `build/vite/setup-dev.ts` sets) because the stock workbench otherwise computes
  `vscode-file://` module URLs only Electron can load.
- The extension host still runs under Electron; the Node sidecar is Phase 7.

## Bundling (Windows)

`tauri.conf.json` targets **NSIS only** (per AGENTS.md). Icons are generated from the
repository's own `resources/win32/` assets. Do not add MSI/Wix/macOS/Linux targets.
