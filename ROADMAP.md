# VS Code Tauri Rewrite Roadmap

> Living document — updated as phases progress (see AGENTS.md, "Status
> tracking"). Status legend: ⬜ Not started · 🟦 In progress · ✅ Done ·
> ⛔ Blocked. A phase is not Done until all acceptance tests pass.

## Target

A native Windows application that is behaviorally identical to VS Code, but
uses Tauri v2 instead of Electron.

## Architecture (locked)

```
+--------------------------------------------------------+
| Renderer: Original VS Code Workbench UI / Monaco       |
| Runs in WebView2, served by the local backbone         |
+----------------------+---------------------------------+
                       | token-authed JSON-RPC (/bridge/rpc)
                       | + WebSocket event bus (/bridge/ws)
+----------------------+---------------------------------+
| Rust/Tauri Backend (src-tauri/)                        |
| - Window shell, backbone HTTP server                   |
| - File system service + notify watcher                 |
| - Terminal service (portable-pty / ConPTY)             |
| - Dialog service (rfd)                                 |
| - Search service (planned)                             |
| - Process/task service (planned)                       |
| - Extension manager (planned)                          |
| - Update service (planned)                             |
+----------------------+---------------------------------+
                       | extension host protocol
+----------------------+---------------------------------+
| Extension Host: web-worker host today;                 |
| Node.js sidecar planned for full extension support     |
+--------------------------------------------------------+
```

Node is temporarily kept (sidecar, Phase 7) for extension compatibility. It
is not Electron and will only leave when a proven Rust replacement exists.

- Upstream pinned at `UPSTREAM_SHA` (currently `1.136.1`,
  `a44adf7f53e00964ab890f9f8758a334f1fc15bc`), pristine, modified only via
  `patches/` (see `patches/README.md`).
- `bridge/` implements **upstream interfaces only** (`IFileSystemProvider`,
  `ITerminalBackend` + `BasePty`, `IFileDialogService` override).
- Branches: **`main` is the only development branch** (AGENTS.md constraint
  1). `tauri-rebuild` is frozen history; stray `tari-rebuild` was removed.

---

## Phase 0 — Repository Baseline and Guardrails

### Status: ✅ Done (baseline metrics pending → moved into Phase 10)

### Goal

Establish a safe main-only baseline, agent rules, roadmap, and CI.

- [x] Clone `Zetsu4i/vscode` and develop on `main` only
- [x] Add `AGENTS.md` (instructions + restrictions for agents)
- [x] Add `ROADMAP.md` (this living document)
- [x] Add `.github/workflows/windows-nsis-release.yml`
      (NSIS-only Windows + auto-release on every successful build)
- [ ] Configure branch protection for `main` (needs repo admin — owner task)
- [x] CI builds the real client from pinned upstream on Windows runners'
      sibling (ubuntu) and bundles on windows-latest
- [ ] Baseline metrics (cold start, installed size, memory, terminal smoke,
      extension smoke) — recorded in Phase 10 alongside parity runs

### Acceptance

- [x] `main` builds successfully
- [x] CI produces a Windows artifact and publishes a Release (dev-5+)

---

## Phase 1 — Tauri Shell Prototype (workbench boots in the shell)

### Status: 🟦 In progress — boot blocker found & fixed; awaiting Windows smoke test

### Goal

Open the existing VS Code workbench UI inside a Tauri window without
replacing features.

- [x] `src-tauri/` with `tauri.conf.json` loading the existing workbench
      build output (backbone serves it on `127.0.0.1:<random port>`)
- [x] Configuration injection mirroring `webClientServer.ts`
      (`vscode-workbench-web-configuration`, nonce/CSP, NLS fallback) —
      no Electron preload needed because the client is served over HTTP
- [x] Workbench renders without fatal errors — **verified in a headless
      browser against the exact dev-5 artifact** after the fix below
- [x] **Fix (patches/0011):** the `web` esbuild target did not bundle the
      browser shell bootstrap — `out/vs/code/browser/workbench/workbench.js`
      and `workbench.css` 404'd and the installed exe showed a blank page.
      The `web` target now includes the `server-web` shell entry (verified
      locally: genuine workbench renders, extension host starts)
- [x] **Fix (CI):** ship `product.json` with the client — the gulp packaging
      task omits it, so the workbench received a fallback product config
      (missing marketplace endpoints, real version). The workflow now
      injects version/commit/date exactly like the client build expects
- [x] Boot-file assertions in CI (workbench.js/css, nls.messages.js,
      product.json) so the "blank window" bug class fails the build instead
      of reaching users

### Acceptance

- [x] VS Code workbench renders in the Tauri-served shell (headless-verified)
- [ ] Editor area renders Monaco with a real file open (Windows smoke test)
- [x] No crash on initial startup
- [ ] Manual smoke checklist recorded: open folder → explorer tree, open/
      edit/save, create/delete/rename, settings editor, theme switch,
      keybindings, command palette, quick open
- [ ] Workspace trust flow verified with the bridge provider

---

## Phase 2 — IPC Contract Extraction

### Status: ⬜ Not started (bridge method list already inventoried inline)

- [ ] Scan upstream source for the web-relevant service surfaces:
      `IFileSystemProvider`, `ITerminalBackend`, `IFileDialogService`,
      `ISCMProvider`, `ISearchService`, `IClipboardService`, storage…
- [ ] Create `compat/ipc-contract.json` documenting each bridge method:
      name, upstream counterpart, args, result, error semantics
- [ ] Group services into: Window, Dialog, Clipboard, Storage, Files,
      Terminal, Search, Process/tasks, Extensions, Update
- [ ] Add contract tests for each group under `compat/tests/`

### Acceptance

- [ ] IPC contract checked into repo
- [ ] Contract tests run in CI

---

## Phase 3 — Window, Dialog, Clipboard, and Storage

### Status: 🟦 In progress (dialogs done; clipboard/storage/window parity pending)

- [x] Native dialog service (`rfd` behind upstream `IFileDialogService`)
- [ ] Window service parity: title sync with active editor, size/fullscreen
      state persistence, window controls
- [ ] Clipboard service parity (WebView2 clipboard vs native — verify
      copy/paste of text, files, images)
- [ ] Storage/settings persistence parity (user data location on Windows,
      profiles, workspace storage)
- [x] Exposed through the bridge with upstream-matching semantics

### Acceptance

- [x] File open/save dialogs work
- [ ] Window title, size, and fullscreen work
- [ ] Clipboard copy/paste works everywhere (editor, terminal, input boxes)
- [ ] Settings persist after restart

---

## Phase 4 — File System Service

### Status: 🟦 In progress (core CRUD + watcher shipped; depth items pending)

- [x] read/write/create/delete/rename/copy through `IFileSystemProvider`
- [x] File watching with Rust `notify`
- [ ] Match VS Code watcher behavior exactly (excludes/includes, atomic
      writes, rename vs delete+create distinction)
- [ ] Handle-based streaming (`open/read/write/close`) for huge files
      (currently whole-file reads/writes)
- [x] Atomic write support server-side
- [ ] Encoding and BOM handling verification matrix
- [x] Workspace folder APIs (folder open through the dialog → workspace URI)
- [ ] Search traversal hooks for the search service (Phase 6)

### Acceptance

- [x] Open/save/edit files works
- [ ] Editor file watchers fire correctly on external edits
- [ ] Large file edits do not block UI (streaming)
- [ ] Encoding edge cases match original

---

## Phase 5 — Terminal Service

### Status: 🟦 In progress (PTY pipeline shipped; parity depth pending)

- [x] PTY service with `portable-pty` (ConPTY on Windows)
- [x] Spawn default shell on Windows
- [x] write/read/resize/kill/exit through the bridge (`pty.*` methods)
- [x] xterm.js renderer via upstream terminal stack
- [ ] Shell integration injection server-side (mirrors
      `terminalEnvironment.ts`) → command decorations, cwd tracking
- [ ] Scrollback replay + reconnect persistence (`BasePty` replay path)
- [ ] Flow control (`acknowledgeDataEvent` → pause/resume pty reads)
- [ ] Signals + Windows ConPTY resize pixel dimensions
- [ ] Profile sources parity with upstream detection (WSL later)

### Acceptance

- [x] Integrated terminal opens and accepts input
- [ ] Resize works in all layouts
- [ ] Multiple terminals work
- [x] Ctrl+C/kill works
- [ ] Shell integration behaves like original

---

## Phase 6 — Search and Process Services

### Status: ⬜ Not started

- [ ] Search service using ripgrep crates as libraries (`grep-searcher` +
      `grep-regex`) — headless, no subprocess
- [ ] Preserve include/exclude glob behavior (reuse upstream matcher
      semantics; honor watcher excludes)
- [ ] Process service for tasks through the pty host path (headless)
- [ ] Environment inheritance parity
- [ ] Kill/exit code handling parity
- [ ] Task output integration back into the workbench

### Acceptance

- [ ] Search works across workspace with correct ranking
- [ ] Include/exclude globs match original
- [ ] Tasks run and stream output
- [ ] Task exit code and kill work

---

## Phase 7 — Extension Host Integration

### Status: 🟦 In progress (web-worker host works; Node sidecar decision pending)

Current: upstream `WebWorkerExtensionHost` runs in a same-origin iframe —
web extensions work today. Node extensions cannot run in the webview yet.

- [ ] Decision doc: Node.js sidecar (VSCode's own ext host, full
      compatibility, ~80MB) vs Rust/WASM host (small, incompatible).
      Default plan: **Node sidecar**, speaking the upstream extension host
      protocol over the backbone (stdio / JSON-RPC)
- [ ] Bundle/locate Node runtime sidecar
- [ ] Extension management in Rust: discover installed, read
      `package.json`, install/update/uninstall VSIX
- [ ] Extension webview host through the upstream webview stack (native
      child webviews only with a design note)
- [ ] Marketplace: product configuration (Open VSX / MS gallery decision),
      install/update/uninstall from the extensions view
- [ ] Preserve activation events and API compatibility surface

### Acceptance

- [ ] Existing (web) extensions install and activate
- [ ] Extension webviews render
- [ ] Marketplace install/update/uninstall works
- [ ] Node extensions run through the sidecar
- [ ] Extension terminal + filesystem access work

---

## Phase 8 — Debug, Tasks, and Language Features

### Status: ⬜ Not started

- [ ] Verify DAP adapter spawning (through the sidecar once Phase 7 lands)
- [ ] Verify Debug Console output
- [ ] Verify task-based debug prelaunch
- [ ] Verify language server spawning through extensions
- [ ] Verify extension output channels

### Acceptance

- [ ] JS/TS debugging works
- [ ] Language features work
- [ ] Debug UI matches original

---

## Phase 9 — Settings, Keybindings, and Accessibility

### Status: ⬜ Not started

- [ ] Test settings editor against the backbone-backed user data dir
- [ ] Test keybinding service and conflicts
- [ ] Test keyboard shortcuts on Windows (WebView2 quirk audit)
- [ ] Test accessibility and screen reader paths
- [ ] Test locale/time/font handling

### Acceptance

- [ ] Settings apply immediately
- [ ] Keybinding conflicts behave like original
- [ ] Accessibility does not regress

---

## Phase 10 — Full Parity Audit and Cleanup

### Status: ⬜ Not started

- [ ] Run VS Code smoke tests where feasible
- [ ] Build a feature checklist from original VS Code
- [ ] Automated side-by-side comparison: startup, memory, terminal,
      extensions, file operations (baseline metrics from Phase 0 land here)
- [ ] Remove Electron main files patch-by-patch once no client path
      references them (`vs/code/electron-main`, `vs/platform/electron-*`)
- [ ] Remove obsolete Node main-process code (never the ext host sidecar
      while Phase 7 depends on it)
- [ ] Update `ROADMAP.md` with cleanup commits

### Acceptance

- [ ] No Electron dependency remains in the shell
- [ ] All features pass checklist
- [ ] Legacy code removed in isolated cleanup commits

---

## Phase 11 — Release Engineering and Optimization

### Status: 🟦 In progress (auto-release shipped; signing/updater/perf pending)

- [x] Automatic GitHub release on every successful build
      (`dev-<run_number>` prereleases, NSIS exe attached)
- [x] NSIS-only Windows bundling
- [ ] Signed NSIS installer
- [ ] Updater integration (Tauri updater) behind a product flag
- [ ] Performance profiling + startup time reduction
- [ ] Memory profiling vs Electron baseline
- [ ] User-facing changelog generation

### Acceptance

- [x] NSIS installer released automatically
- [x] Release notes present
- [ ] Installer works on a clean Windows machine (dev-5 verified install;
      re-verify after boot fix)

---

## Build & release

- CI (`.github/workflows/windows-nsis-release.yml`):
  1. `build-client` (ubuntu): fetch pinned upstream → prepare (bridge +
     patches) → `npm ci` → `tsc --noEmit` gate → `gulp vscode-web-ci` →
     ship `product.json` → boot-file assertions → artifact
  2. `bundle-windows` (windows-latest): download client → `cargo tauri
     build --config tauri.windows.conf.json` (**NSIS only**) → artifact
  3. `release`: on every green build publish prerelease `dev-<run_number>`
     with the NSIS installer
- Local dev: `scripts/fetch-upstream.sh` → `scripts/prepare-client.sh` →
  `scripts/build-client.sh` → `cargo tauri dev`.

## Status log

- 2026-09-05: Architecture pivot — upstream-first approach replaces the
  discarded lookalike app. Phase 0 complete, Phase 1 in progress.
- 2026-09-05: First fully green pipeline (run 33926758651, release dev-5
  with NSIS installer). The non-ci gulp task's "angler" mangler fails on
  upstream's own code at 1.136.1 — we build with upstream's product-build
  task (`vscode-web-ci`) and keep a full-tree `tsc --noEmit` gate instead.
- 2026-09-05 (later): **Blank-window bug root-caused and fixed.** The
  installed dev-5 exe served the workbench HTML but
  `out/vs/code/browser/workbench/workbench.js` did not exist — upstream's
  `web` esbuild target never bundles the browser shell bootstrap (only
  `server-web` does, which official `code serve-web` serves). Fix:
  `patches/0011-web-target-browser-shell.patch` adds the entry + CSS bundle
  to the `web` target; verified locally by bundling the entry and booting
  the genuine workbench (welcome page, extension host, status bar) in a
  headless browser against the exact dev-5 artifact. Second bug fixed in
  CI: `product.json` was never shipped with the client (build-client.sh was
  not the code path CI used), so the workbench ran on the fallback product
  config — the workflow now injects it with version/commit/date.
  Governance: development consolidated on `main` (constraint 1);
  `windows-nsis-release.yml` replaces ci.yml (NSIS-only, auto-release,
  boot-file regression guards). Linux bundling paused pending explicit
  approval per AGENTS.md constraint 8.
- 2026-09-05 (later): **Branch policy corrected + raw-HTML window bug
  fixed.** The maintainer clarified constraint 1: `main` is the pristine
  `microsoft/vscode` mirror they branch from for experiments — the product
  line lives on `tauri-rebuild`. `main` was restored to upstream
  `1.136.1` (`a44adf7f`) and all product commits + the workflow now target
  `tauri-rebuild` exclusively. The "window shows raw HTML source" bug had
  two server-side root causes: (1) the axum workbench handler returned
  `String`, whose default content type is `text/plain`, so WebView2
  rendered the HTML as literal text; the handler now serves
  `text/html; charset=utf-8`. (2) The injected `globalThis.__VSTAURI__`
  was a hand-rolled `format!` that emitted literal `{{...}}` — a JS syntax
  error that killed the bridge bootstrap; the object is now serialized
  with `serde_json`. Hardening: visible on-page boot-failure watchdog,
  file logging (`vstauri.log` in the app data dir) because release builds
  have no console, fast-fail boot-file assertions in `main.rs`, and a
  headless `--vstauri-smoke` mode that CI uses to assert content types +
  injected config + boot files against the freshly built binary before
  release. CI now runs on `tauri-rebuild` only and caches the client
  build (keyed on upstream pin + patches + bridge), Rust deps
  (`rust-cache`), the `cargo-tauri` binary and NSIS tooling — repeat
  builds drop from ~50 min to a fraction of that.
- 2026-09-05 (later): **UI renders — user confirmed; open-folder/save and the
  node_modules 404 root-caused and fixed.** The maintainer verified the
  workbench now renders in the installed app and reported that opening a
  folder and saving files does not work, plus a runtime 404 for
  `/node_modules/vscode-regexp-languagedetection/dist/index.js`. Root cause
  of the dialogs: the workbench startup applies the global singleton
  registry AFTER `BrowserMain`'s `serviceCollection.set` calls
  (`src/vs/workbench/browser/workbench.ts`), so the bridge's
  `TauriFileDialogService` registered via `serviceCollection.set` was
  silently replaced by upstream's browser `FileDialogService` — which for
  the `file` scheme throws "Can't open folders..." and saves through the
  File System Access API against a provider that is no longer registered.
  Fix: `TauriFileDialogService` is now also pushed into the singleton
  registry from `tauri.contribution.ts` (its module order guarantees it
  lands after upstream's registration, and the last descriptor wins).
  Extended the dialog bridge: native Save (untitled/Save As), Open
  Workspace, `showSaveDialog`/`showOpenDialog` with title + filters
  (`folder`/`file`/`files`/`workspace`/`save` modes in one `dialog.pick`
  RPC). Session restore: the backbone now parses `?folder=`/`?workspace=`/
  `?ew=` exactly like the official server, persists the last folder to
  `last_folder.txt` in the app data dir, and injects `folderUri` into the
  workbench configuration on plain launches — the app reopens the previous
  workspace, desktop-style. The 404: upstream's `gulp vscode-web` packaging
  does not ship node_modules (the official serve-web npm package does); the
  language-detection/oniguruma/tree-sitter packages are now copied into the
  client by `scripts/ship_node_modules.py` (also scans the built JS for
  future references), asserted in CI boot-file checks and smoke
  (`/node_modules/...` 200 + MIME), and the smoke also exercises `?folder=`
  capture + `folderUri` restore injection. CI gates: bridge edits are
  typechecked by the existing `tsc --noEmit` job; bridge changes invalidate
  the client cache so this run rebuilds the full client once.
