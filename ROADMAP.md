# VS Code Tauri Rewrite Roadmap

## Target

A native Windows application that is behaviorally identical to VS Code, but uses Tauri v2 instead of Electron.

## Architecture: "Wind & Mountain" (Shimmed Monorepo)

The strategy is a two-layer abstraction pattern modeled on how bleeding-edge
open-source editors (e.g. the Land Project) port the *native* VS Code workbench
out of Electron without a VS Code Server, without fake UI layers, and without
touching Microsoft's workbench code:

- **The Wind layer** (UI environment shim) — the preload that replaces
  Electron's `ipcRenderer`, `process` and window-config handshake with
  equivalents backed by `window.__TAURI__` / Tauri IPC. The workbench source
  is compiled *directly from the original tree*, unmodified.
- **The Mountain layer** (Rust native backend) — implements the exact Node/Electron
  main-process IPC surface natively in Rust: the `vscode:` plain channels, the
  main-process message protocol (binary frames), the service channels
  (`nativeHost`, `storage`, `logger`, `userDataProfiles`, ...), and eventually
  PTY, file watching, and credential storage.

```
+--------------------------------------------------------------+
|                     VSTAURI (TAURI v2) APPLICATION           |
|                                                              |
|   +------------------------------------------------------+   |
|   |                  TAURI WEBVIEW UI                    |   |
|   |                                                      |   |
|   |  +------------------------------------------------+  |   |
|   |  | RAW VSCODE WORKBENCH UI                        |  |   |
|   |  | (Compiled directly from original source)       |  |   |
|   |  | served via in-process vscode-file:// protocol  |  |   |
|   |  +------------------------------------------------+  |   |
|   |                           ^                          |   |
|   |                           | Extracted Interfaces     |   |
|   |  +------------------------------------------------+  |   |
|   |  | THE WIND SHIM LAYER (preload, doc-start)       |  |   |
|   |  | window.vscode.ipcRenderer / process / context  |  |   |
|   |  | -> forwarded to Rust via Tauri invoke() with   |  |   |
|   |  |    the original `vscode:` channel names        |  |   |
|   |  +------------------------------------------------+  |   |
|   +---------------------------+--------------------------+   |
|                               | Tauri IPC (JSON + base64    |
|                               | binary frames)              |
|   +------------------------------------------------------+   |
|   |            THE MOUNTAIN BACKEND (Rust Core)          |   |
|   |  (Implements the exact Node IPC APIs in native Rust) |   |
|   |  - vscode:hello / vscode:message protocol server     |   |
|   |  - nativeHost / storage / logger / profiles / ...    |   |
|   |  - Native Terminal PTY      (portable-pty)           |   |
|   |  - High-speed FS + watching (notify)                 |   |
|   |  - Credential storage       (keyring)                |   |
|   +------------------------------------------------------+   |
|                               ^ stdio / JSON-RPC             |
|   +------------------------------------------------------+   |
|   |  EXTENSION HOST: Node.js SIDECAR                     |   |
|   |  - Bundled node.exe binary in src-tauri/binaries/    |   |
|   |  - Original VS Code extension host code              |   |
|   |  - Talks to Mountain over local gRPC/WebSocket       |   |
|   |    managed by Rust (Phase 7)                         |   |
|   +------------------------------------------------------+   |
+--------------------------------------------------------------+
```

### Repository mapping (one repo, one branch)

The "forked vanilla workbench + shims + backend" monorepo layout maps onto
this repository (AGENTS.md constraint 1: single `tauri-rewrite` branch, the
upstream tree stays pristine and in-tree instead of a submodule):

| Blueprint directory | This repository |
| --- | --- |
| `ui-workbench/` (vendored upstream) | the original VS Code tree at repo root (`src/vs/**`, `build/**`, compiled `out/`) — never modified by us |
| `src-shims/` (Wind) | `src-tauri/src/shim.js` (preload) + `src-tauri/src/config.rs` (window-config handshake) |
| `src-tauri/` (Mountain) | `src-tauri/src/{ipc,protocol,logger,util,config}.rs` + the growing channel services |

### Minutes-to-Update: consuming upstream VS Code releases

Because all custom code lives in the Wind and Mountain layers (plus `.github/`
CI), the thousands of workbench files are never touched. When a massive new
VS Code release drops, the update procedure is mechanical:

1. `git checkout main && git fetch upstream && git merge <upstream release tag>`
   (the vendored tree = upstream; our branch only carries `src-tauri/`,
   `compat/`, `build/ipc-contract/`, `ROADMAP.md`, `AGENTS.md`, CI files).
2. `git checkout tauri-rewrite && git merge main` — resolve conflicts only in
   the (rare) places where upstream changed an IPC surface we shim.
3. Re-run the IPC contract extractor (`node build/ipc-contract/extract-ipc-contract.mjs`)
   — it diffs the freshly scanned contract against `compat/ipc-contract.json`
   and reports exactly which channels/methods the new workbench expects.
4. Update any drifted Mountain channel implementations it flags.
5. `tauri build` — the brand-new workbench wraps around the intact shim stack.

The contract file is therefore the upgrade tripwire: CI fails when the
renderer's IPC surface drifts from what the Rust backend knows about.

### Window chrome parity (frameless + custom titlebar)

- Frameless window (`decorations: false`) with the workbench's own custom
  titlebar layout — exactly the VS Code default Windows experience.
- Drag regions: the shim mirrors Electron's `-webkit-app-region: drag` on
  `.titlebar-drag-region` (mousedown → Tauri `startDragging`, double click →
  toggle maximize) — `data-tauri-drag-region` semantics.
- Native minimize/maximize/close buttons are injected into the workbench's
  `.window-controls-container` (the DOM shape the original CSS already styles).
- Platform vibrancy/acrylic: optional `tauri-plugin-vibrancy` integration for
  the titlebar and background blur — Phase 11 polish, only if the original
  look stays pixel-identical.

Node is temporarily kept for extension compatibility. It is not Electron.

---

## Status Legend

- ⬜ Not started
- 🟦 In progress
- ✅ Done
- ⛔ Blocked

---

## Phase 0: Repository Baseline and Guardrails

### Status: 🟦 In progress

### Goal

Establish a safe main-only baseline, add agent rules, roadmap, and CI.

### Tasks

- [x] Clone `https://github.com/Zetsu4i/vscode.git` and use only `main`
- [x] Add `AGENTS.md`
- [x] Add `ROADMAP.md`
- [x] Add `.github/workflows/windows-nsis-release.yml`
- [ ] Configure branch protection for `main`
- [ ] Verify original Electron build runs on Windows CI
- [ ] Record baseline metrics:
  - [ ] cold startup time
  - [ ] installed size
  - [ ] memory usage with empty workspace
  - [ ] terminal smoke test
  - [ ] extension smoke test

### Acceptance

- [ ] `main` builds successfully
- [x] CI produces a Windows artifact (NSIS exe published as GitHub Release `dev-19`+ from `tauri-rewrite`)
- [ ] Baseline metrics saved in `docs/baseline.md`

---

## Phase 1: Tauri Shell Prototype

### Status: 🟦 In progress

### Goal

Open the existing VS Code workbench UI inside a Tauri window without replacing features.

### Tasks

- [x] Add `src-tauri/` to the repository
- [x] Configure `src-tauri/tauri.conf.json` to load the existing workbench build output
  - Served through the in-process `vscode-file://` custom protocol registered in `src-tauri/src/protocol.rs` (the same scheme + authority the renderer derives its ESM base URL from: `vscode-file://vscode-app/<appRoot>/out/`). No HTTP server, no vscode-web.
- [x] Create a Tauri preload shim that exposes minimal browser globals:
  - [x] `window.vscode.process.platform` / `.arch` / `.env` / `.versions` / `.execPath` (full `ISandboxNodeProcess` surface from `globals.ts`)
  - [x] `setImmediate`
  - [x] `window.vscode.context.resolveConfiguration()` (replaces the `--vscode-window-config` IPC handshake; configuration built in Rust in `src-tauri/src/config.rs`)
  - [x] `window.vscode.ipcRenderer` (send/invoke/on/once/removeListener with original `vscode:` channel names, routed to Rust and logged to `ipc-calls.jsonl`)
  - [x] `Buffer` / `global` intentionally NOT shimmed (Electron sandboxed renderers do not expose them either)
- [ ] Get the workbench window to render without fatal errors (needs first run on Windows; error feedback lands in `%APPDATA%\VSTauri\logs\vstauri.log` + `ipc-calls.jsonl`)
  - [x] Root cause of the first build's blank white window found and fixed: the document was served from `http://vscode-file.vscode-app`, which Tauri v2 classifies as a REMOTE origin (`is_local_url` only accepts `http://<scheme>.localhost` hosts), so every `invoke()` from the preload shim was rejected by the IPC ACL. The window now navigates to `vscode-file://localhost/...` → origin `http://vscode-file.localhost` → local → IPC allowed.
  - [x] ESM boot bridged: the workbench's absolute import (`vscode-file://vscode-app/<appRoot>/out/vs/workbench/workbench.desktop.main.js`) can never resolve under WebView2 (wry only routes http(s) WebResourceRequested traffic), so the shim enables the renderer's own dev boot path (`VSCODE_DEV` + `_VSCODE_USE_RELATIVE_IMPORTS` → document-relative workbench import) and traps `_VSCODE_FILE_ROOT` so all `FileAccess` URLs resolve inside the document origin.
  - [x] CSS module bridge: the dev-compiled ESM tree keeps `import './x.css'` statements; protocol.rs answers those module-graph requests with `_VSCODE_CSS_LOAD` wrapper modules (the server-side twin of Electron dev's cssModules import map) and serves `text/css` for stylesheet requests (distinguished by `Sec-Fetch-Dest`).
  - [x] Main-process IPC protocol implemented natively: `vscode:hello` → Initialize frame, `vscode:message` binary frames (base64 bridge, codec mirrored from ipc.ts and round-trip tested), channel router with `nativeHost` window operations; unregistered channels reject like Electron's pending-request timeout and are logged for Phase 2.
  - [x] Custom-titlebar parity: frameless window (`decorations: false`), drag region + dblclick-maximize on `.titlebar-drag-region`, injected `.window-icon` min/max/close buttons into `.window-controls-container`, WebView2 default context menu suppressed globally.
  - [x] Full request trace logging in the protocol handler (first 1000 requests per run) + renderer error forwarding — the remote-debugging loop for the next iterations.
- [x] Keep Electron app still buildable in parallel (Electron tree untouched)

### Acceptance

- [ ] VS Code workbench renders in Tauri window
- [ ] Editor area renders Monaco
- [ ] No crash on initial startup
- [x] Original Electron build still works (compile job green on Windows CI)
- [x] CI build time: node_modules cache (upstream composite actions) + compiled `out/` cache keyed on `hashFiles(src/**, build/**, package*.json)` — typecheck and the gulp compile are skipped entirely when renderer inputs are unchanged (the normal case during the shell transplant, which lives in `src-tauri/` and `.github/`)

---

## Phase 2: IPC Contract Extraction

### Status: 🟦 In progress

### Goal

Catalog every Electron main/renderer IPC surface before changing it.

The contract has two halves, both captured by the extractor script
(`build/ipc-contract/extract-ipc-contract.mjs`):

1. **Plain `vscode:` ipcRenderer channels** — `validatedIpcMain.handle/on` in
   electron-main, `ipcRenderer.send/invoke/on` in the preload/renderer.
2. **Main-process message protocol channels** (the `vscode:hello` /
   `vscode:message` binary protocol the Wind shim bridges into Rust) —
   every `registerChannel('<name>', ...)` on the main/shared-process side
   paired with every `getChannel('<name>')` on the renderer side, plus the
   command/event surface each channel serves (explicit `IServerChannel`
   switch statements, or `ProxyChannel.fromService` over a service interface
   — e.g. `nativeHost` exposes the full `INativeHostService` method list).

### Tasks

- [x] Scan original source for:
  - [x] `ipcMain.handle` / `ipcMain.on` (via `validatedIpcMain` wrapper)
  - [x] `ipcRenderer.invoke` / `ipcRenderer.send` (preload + renderer)
  - [x] `webContents.send` (main → renderer push channels)
  - [x] `registerChannel` / `getChannel` (protocol service channels, including
        constant-named channels like `localFilesystem`, `meteredConnection`)
- [x] Create `compat/ipc-contract.json` (machine-readable, grouped, with
      producer/consumer file+line references)
- [x] Create `compat/ipc-contract.md` (human-readable summary tables)
- [x] Extract the `nativeHost` / `userDataProfiles` / `keyboardLayout`
      ProxyChannel service interfaces into explicit command lists
- [ ] Group services into:
  - [x] Window / nativeHost
  - [x] Dialog (nativeHost `pickFileAndOpen` / `showOpenDialog` / ...)
  - [ ] Clipboard (nativeHost read/write clipboard)
  - [ ] Storage
  - [ ] Files (localFilesystem provider channel)
  - [ ] Terminal (pty host channels)
  - [ ] Search
  - [ ] Process/tasks
  - [ ] Extensions (extension host + gallery channels)
  - [ ] Update
- [x] Add contract drift check to CI (fails when the scanned surface changes
      without the contract being regenerated — the upstream-update tripwire)
- [x] Add Mountain coverage report to the extractor output (which channels
      Rust already answers vs. rejects) — `compat/ipc-contract.md`
- [ ] Add contract tests for each group

### Acceptance

- [x] IPC contract checked into repo (`compat/ipc-contract.json`)
- [x] Contract extraction runs in CI (drift check)
- [ ] Contract tests run in CI for each channel group

---

## Phase 3: Window, Dialog, Clipboard, and Storage

### Status: 🟦 In progress

### Goal

Replace Electron main process basics with Tauri/Rust services.

### Tasks

- [ ] Implement window service (multi-window lifecycle: new/focus/close
      per IWindowOpenable + forceNewWindow — currently a single window
      reloaded into the new workspace; full window management is a later
      phase)
- [x] Implement native dialog service — `nativeHost` channel:
      showSaveDialog / showOpenDialog / showMessageBox / pickFileAndOpen /
      pickFolderAndOpen / pickWorkspaceAndOpen / pickFileFolderAndOpen /
      showItemInFolder / openExternal. File dialogs ride on
      tauri-plugin-dialog (the same Win32 common dialogs Electron uses,
      filters + defaultPath + multiSelection + openDirectory mapped);
      showMessageBox uses Windows TaskDialogIndirect via windows-sys so
      Electron-style custom button labels, defaultId, cancelId and the
      verification checkbox all work (response index mapping preserved);
      openExternal is ShellExecuteW, showItemInFolder is
      `explorer /select`; pick*AndOpen feeds config::apply_window_openables
      (filesToOpenOrCreate / folderUri / workspace with the upstream md5
      workspace id) and reloads the window. `vscode_ipc` became an async
      command running route() on the blocking pool — native modal dialogs
      must never block the main thread's message loop.
- [x] Implement clipboard service — readClipboardText / writeClipboardText
      via tauri-plugin-clipboard-manager (the editor's copy/paste path:
      workbench NativeClipboardService routes through these nativeHost
      commands); readImage/writeImage with PNG <-> RGBA through the image
      crate (Electron nativeImage.toPNG parity); read/writeClipboardBuffer
      custom formats (`code/file-list`) still stubbed — raw Win32
      RegisterClipboardFormat plumbing is a later round
- [ ] Implement storage/settings persistence
  - [x] `storage` protocol channel (getItems / updateItems / getValue /
        compareAndSwap / optimize / isUsed / onDidChangeStorage) backed by a
        Rust JSON-file KV store per scope (application / application-shared /
        profile / workspace) — `src-tauri/src/storage_channel.rs`
  - [x] `logger` protocol channel (createLogger / log / consoleLog /
        registerLogger / deregisterLogger / setLogLevel / setVisibility /
        getRegisteredLoggers + change events) writing real log files under
        the logs dir — `src-tauri/src/logger_channel.rs`
  - [x] `userDataProfiles` protocol channel (profile CRUD + onDidChangeProfiles)
        over a persistent `profiles.json` — `src-tauri/src/profiles_channel.rs`
- [x] Implement `keyboardLayout` protocol channel (getKeyboardLayoutData with
      a real US-layout Windows mapping + onDidChangeKeyboardLayout event) —
      `src-tauri/src/keyboard_channel.rs`
- [ ] Credential storage: replace `keytar`-style secret storage with the Rust
      `keyring` crate behind the `encryption`/secret channels (Windows
      Credential Manager under the hood)
- [x] Expose through Tauri IPC with the same channel names
- [x] Keep old Electron implementation until tests pass (Electron tree untouched)

### Acceptance

- [x] File open/save dialogs work (backend implemented; smoke-test on
      Windows in the next build round)
- [ ] Window title, size, and fullscreen work (title/size/fullscreen
      already answer; multi-window lifecycle pending)
- [x] Clipboard copy/paste works (text + images; custom formats pending)
- [ ] Settings persist after restart (disk-backed by construction; needs a
      Windows restart round-trip to confirm end-to-end)

---

## Phase 4: File System Service (Mountain: FS)

### Status: 🟦 In progress

### Goal

Replace Electron file service with Rust file service.

### Tasks

- [x] Implement the `localFilesystem` provider channel (DiskFileSystemProviderChannel
      command surface, extracted in the Phase 2 contract) natively in Rust —
      `src-tauri/src/fs_channel.rs`: stat / readdir / readFile / writeFile /
      mkdir / delete / rename / copy / cloneFile / realpath / open-read-write-close
      (fd streams) / watch-unwatch, with binary `VSBuffer` frames lifted through
      the IPC codec (ipc.rs tag-3 bridge) and FileSystemError-shaped rejections
      (FileNotFound / NoPermissions / FileExists)
- [x] Implement read/write/create/delete/rename/copy
- [x] Atomic writes (temp file + rename, same guarantee SQLite gave Electron)
- [x] fd-based stream IO for large files (open / read / write / close)
- [x] Implement file watching with Rust `notify` (replaces `@parcel/watcher` /
      win32 `ReadDirectoryChangesW` usage — VS Code watcher semantics preserved:
      recursive, ignore rules, batching) — `fs_channel.rs` + `ipc.rs`:
      each `watch(sessionId, req, resource, opts)` opens a real
      `notify` watcher on its own thread with the upstream
      `diskFileSystemProviderClient.ts` contract (`listen('fileChange',
      [sessionId])` / `unwatch(sessionId, req)`), 50 ms debounce batching,
      ADDED/UPDATED/DELETED resolution by existence at flush time (matches
      @parcel/watcher on both inotify and ReadDirectoryChangesW),
      `excludes`/`includes` globs matched relative to the watch root with
      a `**`-aware segment matcher, and per-session event routing via the
      new `ipc::fire_event_with_arg` (EventListen args are now kept)
- [x] Match VS Code watcher behavior (per-session partitioning, string
      error payloads on watch failure, recursive + non-recursive modes,
      exclude-driven pruning incl. the watched directory itself)
- [ ] Re-arm watchers when the watched directory is deleted and recreated
      (upstream parcel watcher behavior — tracked for the next round)
- [ ] Add encoding and BOM handling
- [ ] Add workspace folder APIs
- [ ] Add search file traversal hooks

### Acceptance

- [ ] Open/save/edit files works identically
- [ ] Editor file watchers fire correctly
- [ ] External file change detection works
- [ ] Large file edits do not block UI

---

## Phase 5: Terminal Service (Mountain: PTY)

### Status: 🟦 In progress (core backend done; renderer bring-up next)

### Goal

Replace Electron/node-pty terminal backend with Rust PTY.

### Tasks

- [x] Implement PTY service with the Rust `portable-pty` crate (ConPTY on Windows)
      as a Tauri sidecar-free native service; data streams flow back to xterm.js
      through the Wind shim's `vscode:message` protocol frames —
      `src-tauri/src/terminal_channel.rs` implements the full `IPtyService` /
      `IPtyHostService` ProxyChannel surface registered as channel `localPty`
      (electron-main app.ts line ~1447): createProcess / start / shutdown /
      shutdownAll / input / processBinary / sendSignal / resize / clearBuffer /
      acknowledgeDataEvent / listProcesses / getInitialCwd / getCwd /
      attach-detach / refreshProperty / updateProperty / updateTitle /
      updateIcon / getDefaultSystemShell / getEnvironment / getWslPath /
      getProfiles (config merge + Windows auto-detection: PowerShell,
      Windows PowerShell, Command Prompt, Git Bash, WSL) /
      freePortKillProcess / set-getTerminalLayoutInfo (in-memory, survives
      window reloads) / serializeTerminalState / reviveTerminalProcesses /
      the auto-reply + contribution stubs, with onProcessData / onProcessReady
      (pid + cwd + ConPTY build number via RtlGetVersion) / onProcessExit
      events, an incremental UTF-8 decoder (chunk-boundary-safe, CJK output
      intact) and split killer semantics so shutdown never races the exit
      reaper
- [x] Spawn default shell on Windows (COMSPEC / explicit profile path;)
      Unix $SHELL for dev parity
- [x] Support write, read, resize, kill, exit (reader thread until EOF →
      child wait → exit code event; resize via MasterPty::resize; SIGINT
      mapped to ^C)
- [x] Integrate xterm.js renderer IPC (onProcessData events carry the
      { id, event: { data } } payloads the renderer's LocalPty proxy
      subscribes to; no renderer changes needed)
- [ ] Preserve shell integration behavior (injectedArgs stay empty for
      now — script injection is the next round)
- [x] Preserve cwd and environment handling (string | UriComponents cwd,
      env merge: inherited → resolved env → launch-config env)
- [ ] Persistent terminal state across app restarts (serialize/revive are
      in-memory stubs; layout info survives window reloads)
- [ ] Dynamic cwd tracking via OSC 633/9;9 (initial cwd is reported)

### Acceptance

- [ ] Integrated terminal opens and accepts input (backend verified by
      round-trip tests; needs a renderer smoke test on Windows)
- [ ] Resize works
- [ ] Multiple terminals work
- [ ] Ctrl+C/kill works
- [ ] Shell integration behaves like original

---

## Phase 6: Search and Process Services

### Status: ⬜ Not started

### Goal

Replace ripgrep/process backend with Rust equivalents.

### Tasks

- [ ] Implement search service using Rust grep/ripgrep-compatible crate
- [ ] Preserve include/exclude glob behavior
- [ ] Implement process service for tasks
- [ ] Add environment inheritance
- [ ] Add kill/exit code handling
- [ ] Integrate task output back into workbench

### Acceptance

- [ ] Search works across workspace
- [ ] Include/exclude globs match original
- [ ] Tasks run and output
- [ ] Task exit code and kill work

---

## Phase 7: Extension Host Integration (Mountain: Sidecar)

### Status: ⬜ Not started

### Goal

Preserve extension host while replacing the Electron extension main bridge.

### Tasks

- [ ] Keep original VS Code extension host as Node.js sidecar
- [ ] Bundle a pre-compiled Node.js binary into `src-tauri/binaries/` and
      register it as a Tauri sidecar (`externalBin` in tauri.conf.json)
- [ ] Spin up the sidecar at app boot from Rust; the extension host process
      is managed by the Mountain backend
- [ ] Replace Electron IPC transport (MessagePort-based
      `vscode:createPtyServiceMessageChannel` style handshakes) with stdio or a
      local gRPC/WebSocket channel managed by Rust — the Wind shim's
      `ipcMessagePort.acquire` maps onto it
- [ ] Implement extension management in Rust:
  - [ ] Discover installed extensions
  - [ ] Read `package.json`
  - [ ] Install/update/uninstall VSIX
- [ ] Implement extension webview host through Tauri child webviews
- [ ] Preserve extension marketplace behavior
- [ ] Preserve extension activation events
- [ ] Preserve extension API compatibility

### Acceptance

- [ ] Existing extensions install and activate
- [ ] Extension webviews render
- [ ] Marketplace install/update/uninstall works
- [ ] Extension terminal access works
- [ ] Extension filesystem access works

---

## Phase 8: Debug, Tasks, and Language Features

### Status: ⬜ Not started

### Goal

Ensure DAP/LSP-based features continue to work through the new backend.

### Tasks

- [ ] Verify DAP adapter spawning
- [ ] Verify Debug Console output
- [ ] Verify task-based debug prelaunch
- [ ] Verify language server spawning through extensions
- [ ] Verify extension output channels

### Acceptance

- [ ] JS/TS debugging works
- [ ] Language features work
- [ ] Debug UI matches original

---

## Phase 9: Settings, Keybindings, and Accessibility

### Status: ⬜ Not started

### Goal

Verify all user settings and keybindings behave identically.

### Tasks

- [ ] Test settings editor
- [ ] Test keybinding service
- [ ] Test keyboard shortcuts
- [ ] Test accessibility and screen reader paths
- [ ] Test locale/time/font handling

### Acceptance

- [ ] Settings apply immediately
- [ ] Keybinding conflicts work
- [ ] Accessibility does not regress

---

## Phase 10: Full Parity Audit and Cleanup

### Status: ⬜ Not started

### Goal

Systematically remove Electron main process code after full verification.

### Tasks

- [ ] Run VSCode smoke tests
- [ ] Run integration test suite
- [ ] Build a feature checklist from original VS Code
- [ ] Automated side-by-side comparison:
  - [ ] Startup
  - [ ] Memory
  - [ ] Terminal
  - [ ] Extensions
  - [ ] File operations
- [ ] Remove Electron main files file-by-file
- [ ] Remove obsolete Node main-process code
- [ ] Keep extension host Node sidecar for now
- [ ] Update `ROADMAP.md` with cleanup commits

### Acceptance

- [ ] No Electron dependency remains in shell
- [ ] All features pass checklist
- [ ] Legacy code removed in isolated cleanup commits

---

## Phase 11: Release Engineering and Optimization

### Status: ⬜ Not started

### Goal

Harden release flow and optimize for Windows.

### Tasks

- [ ] Signed NSIS installer
- [ ] Automatic GitHub release on successful build
- [ ] Updater integration if applicable
- [ ] Performance profiling
- [ ] Memory profiling
- [ ] Reduce startup time
- [ ] User-facing changelog generation

### Acceptance

- [ ] NSIS installer released automatically
- [ ] Release notes present
- [ ] Installer works on clean Windows VM
