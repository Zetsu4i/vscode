# VSTauri Roadmap

> Living document — updated as phases progress (see AGENTS.md, "Process rules").
> Product: the **real** Visual Studio Code web workbench, unmodified, served by a
> **Rust backbone** inside a **Tauri 2** shell. No Electron, no Node runtime in
> the shipped app, no lookalike UI.

## Architecture (locked)

```
┌────────────────────────────────────────────────────────────┐
│ Tauri 2 shell (native window, NSIS/deb/AppImage packaging) │
│  └─ WebView2 / WebKitGTK                                   │
│      └─ http://127.0.0.1:<port>/ served by the backbone    │
│          ├─ pristine vscode-web client (upstream build)    │
│          │    + patches/ (2 surgical registration seams)   │
│          │    + bridge/ (upstream-interface impls in TS)   │
│          └─ Rust backbone                                  │
│              ├─ fs: stat/read/write/mkdir/rename/copy/     │
│              │      delete + notify watcher                │
│              ├─ pty: portable-pty (ConPTY) terminals       │
│              ├─ dialog: native open file/folder (rfd)      │
│              └─ bridge websocket event bus                 │
└────────────────────────────────────────────────────────────┘
```

- Upstream pinned at `UPSTREAM_SHA` (currently `1.136.1`,
  `a44adf7f53e00964ab890f9f8758a334f1fc15bc`).
- `bridge/` implements **upstream interfaces only**:
  `IFileSystemProvider` (file scheme), `ITerminalBackend` (+ `BasePty`), and a
  `FileDialogService` override for native dialogs.
- `patches/` (applied by `scripts/prepare-client.sh`):
  - `0010-tauri-bridge-registration.patch` — web.main.ts: register the bridge
    fs provider (fallback: upstream FSA provider), override `IFileDialogService`.
- Old approach (hand-built React workbench, Monaco wiring, git/rg/lsp via
  subprocesses) is **deleted**; its history remains in git.

## Phase 0 — Foundation, governance, CI ✅ (this revision)

- [x] Pin pristine upstream (`scripts/fetch-upstream.sh`, `UPSTREAM_SHA`)
- [x] Bridge: fs provider, terminal backend + pty, dialogs, RPC/WS transport
- [x] Minimal patch set through real git diffs (roundtrip-verified)
- [x] Rust backbone: axum server, workbench template rendering (mirrors
      `webClientServer.ts`: nonce, CSP, product/config injection), static
      client serving, fs + watcher, PTY, dialogs, token auth
- [x] **Headless everywhere** — no console popups: no `std::process::Command`
      anywhere in the backbone (libraries only); ConPTY for shells
- [x] AGENTS.md governance for agents
- [x] CI: build real client from pinned upstream → Tauri bundle
      (Linux: deb+AppImage, Windows: **NSIS only**) → auto-release on green
- [x] Remove the hallucinated lookalike app (src/, vite, old backend)

## Phase 1 — Workbench boots and feels like VS Code (in progress)

Goal: window opens into the genuine VS Code workbench with our services wired.

- [x] Serve pristine workbench with product/config injection
- [x] `file` scheme provider so folders/files resolve on disk
- [x] Native Open File / Open Folder (rfd) through the upstream dialog flow
- [x] CI green end-to-end on both platforms (client build + bundle) — run 33926758651:
      tsc full-tree gate + gulp vscode-web-ci + NSIS/deb/AppImage + auto-release (dev-5)
- [ ] Manual smoke checklist recorded here: open folder → explorer tree,
      open/edit/save file, create/delete/rename via explorer, settings editor,
      theme switch, keybindings, command palette, quick open
- [ ] Verify workspace trust flow with the bridge provider
- [ ] Empty-window UX: "Open Folder" entry points all reachable

## Phase 2 — File system depth

- [ ] Search: replace the FSA worker fallback path with a backbone-backed
      search provider (ripgrep engine as a library: `grep-searcher` +
      `grep-regex` — headless, no subprocess)
- [ ] `IFileSystemProvider.open/read/write/close` handle-based streaming for
      huge files (currently whole-file read/write)
- [ ] Watcher excludes/includes filters honored server-side
- [ ] Trash support verification on Windows (Recycle Bin) and Linux
- [ ] Recent workspaces persistence (server-side, token-scoped)

## Phase 3 — Terminal depth

- [ ] Shell integration injection server-side (env + script, mirrors
      `terminalEnvironment.ts`) → command decorations, cwd tracking
- [ ] Scrollback replay + persistence (reconnect on reload, `BasePty` replay
      path already exists)
- [ ] Flow control (acknowledgeDataEvent → pause/resume pty reads)
- [ ] Signals support (portable-pty child signals) + Windows ConPTY resize
      pixel dimensions
- [ ] Profile sources parity with upstream detection (WSL, cloud shells later)

## Phase 4 — SCM & tasks

- [ ] Git: libgit2 (or `gix`) backbone service; register an SCM provider from
      the bridge implementing upstream `ISCMProvider` (status, stage, unstage,
      commit, diff views, branch switch) — no `git` subprocess on Windows
- [ ] .gitignore-aware explorer decorations
- [ ] Tasks: process execution through the pty host path (headless)
- [ ] Debug: DAP bridge (first: Node via bundled runtime decision, see Phase 5)

## Phase 5 — Extension host

Current: web-worker extension host (upstream `WebWorkerExtensionHost`) — web
extensions work; Node extensions cannot run in the webview.

- [ ] Decision doc: Node sidecar runtime (VSCode's own ext host, full
      compatibility, adds ~80MB) vs Rust WASM host (small, incompatible).
      Default plan: **Node sidecar** for compatibility, speaking the upstream
      extension host protocol over the backbone.
- [ ] Marketplace: product configuration for Open VSX (upstream-compatible
      gallery), install/uninstall/update from the extensions view
- [ ] Node-side services behind the bridge (LSP servers spawn, debug adapters)

## Phase 6 — Prune upstream (only after 100% translation)

Per AGENTS.md: upstream code leaves the build only when its replacement is
verified. Each deletion is its own patch, applied step by step.

- [ ] Strip Electron main/utility process code from the client build
      (`src/vs/platform/electron-*`, `src/vs/code/electron-main/...`) once no
      client path references it
- [ ] Audit `remote/` (server) — decide what remains relevant for the bridge
- [ ] Shrink bundle: drop unused native module builds, trim locales policy

## Phase 7 — Product polish

- [ ] Window icon/title/menu parity, document.title sync to the native window
- [ ] Auto-update channel (Tauri updater) behind a product flag
- [ ] Installer parity: NSIS (primary Windows), deb/AppImage (Linux);
      signing story
- [ ] Startup time & memory budget vs Electron baseline, recorded here
- [ ] Parity checklist: run upstream's own smoke tests where feasible

## Build & release

- CI (`.github/workflows/ci.yml`):
  1. `build-client` (Linux): fetch pinned upstream → prepare (bridge+patches)
     → `npm ci` → `gulp vscode-web` → artifact
  2. `bundle` (matrix ubuntu/windows): download client → `tauri build`
     (Linux: deb+AppImage; Windows: **NSIS only**) → artifacts
  3. `release`: on every green build, publish a prerelease
     `dev-<run_number>` with all installers attached
- Local dev: `scripts/fetch-upstream.sh` → `scripts/prepare-client.sh` →
  `scripts/build-client.sh` → `cargo tauri dev` (or `cargo run` in src-tauri).

## Status log

- 2026-09-05: Architecture pivot — upstream-first approach replaces the
  discarded lookalike app. Phase 0 complete, Phase 1 in progress.
- 2026-09-05: First fully green pipeline (run 33926758651, release dev-5 with
  NSIS installer; Linux glob fix lands in dev-6). Note: the non-ci gulp task's
  "angler" mangler fails on upstream's own code at 1.136.1 (vs/sessions
  protected-field accesses) — we build with upstream's product-build task
  (`vscode-web-ci`) and keep a full-tree `tsc --noEmit` gate instead.
  Follow-up: evaluate `vscode-web-min-ci` to shrink the Linux AppImage
  (~180MB unminified artifact).
