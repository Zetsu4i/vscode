# VS Code Tauri Rewrite Roadmap

## Target

A native Windows application that is behaviorally identical to VS Code, but
uses Tauri v2 + WebView2 instead of Electron. **Not** `vscode-web`, **not** a
background HTTP server: the desktop workbench bundle is loaded from disk via a
Tauri custom URI protocol, and every native capability is a Rust service
reached over Tauri IPC.

## Architecture

```
+----------------------------------------------------------+
| Renderer: original VS Code desktop workbench / Monaco     |
| out-vscode/vs/code/electron-browser/workbench/*           |
| Runs in WebView2, loaded via vscode-file:// custom proto  |
+---------------------------+------------------------------+
                            | preload shim (ipcRenderer-shaped)
                            | Tauri IPC / MessagePort bridge
+---------------------------+------------------------------+
| Rust/Tauri Backend (src-tauri/src/)                       |
| - window / lifecycle / native menus / dialogs / clipboard |
| - asset protocol (vscode-file)                            |
| - file system service + notify watchers                   |
| - terminal service (portable-pty / ConPTY)                |
| - search service (grep/ignore crates, rg parity)          |
| - process & task service                                  |
| - state/storage service (SQLite parity)                   |
| - extension manager (discovery, VSIX, marketplace)        |
| - update service (NSIS)                                   |
+---------------------------+------------------------------+
                            | stdio / named-pipe JSON-RPC
+---------------------------+------------------------------+
| Extension Host: Node.js sidecar                           |
| - original VS Code extension host code                    |
| - executes JS extensions, provides vscode API             |
| - language servers / DAP adapters spawn from here         |
+----------------------------------------------------------+
```

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

Establish a safe single-branch baseline, add agent rules, roadmap, and CI.

### Tasks

- [x] Clone `https://github.com/Zetsu4i/vscode.git` and branch `tauri-shell` from `main`
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

- [ ] `tauri-shell` builds successfully
- [ ] CI produces a Windows artifact
- [ ] Baseline metrics saved in `docs/baseline.md`

---

## Phase 1: Tauri Shell Prototype

### Status: ⬜ Not started

### Goal

Open the existing desktop workbench UI inside a Tauri window without replacing
features.

### Tasks

- [ ] Add `src-tauri/` (Cargo workspace, `tauri.conf.json`, `tauri.windows.conf.json`)
- [ ] Implement the `vscode-file` custom protocol handler resolving `out-vscode`
- [ ] Create the preload shim exposing the globals the workbench requires:
  - [ ] `window.vscode.ipcRenderer` (`send`/`on`/`once`/`invoke`/`removeListener`)
  - [ ] `window.vscode.ipcMessagePort`
  - [ ] `window.vscode.webFrame`
  - [ ] `window.vscode.process` (`platform`, `env`, `versions`, `cwd`, `on`)
  - [ ] `window.vscode.context` (`configuration()`, `resolveConfiguration()`)
  - [ ] `Buffer`, `setImmediate`, `global`
- [ ] Feed the workbench its `INativeWindowConfiguration` payload from Rust
- [ ] Get the workbench window to render without fatal errors
- [ ] Keep the Electron app buildable in parallel

### Acceptance

- [ ] Workbench renders in the Tauri window (title bar, activity bar, sidebar, status bar)
- [ ] Editor area renders Monaco with syntax highlighting
- [ ] No crash on initial startup
- [ ] Original Electron build still works

---

## Phase 2: IPC Contract Extraction

### Status: ⬜ Not started

### Goal

Catalog every Electron main/renderer IPC surface before changing it.

### Tasks

- [ ] Scan original source for `ipcMain.handle`, `ipcMain.on`,
      `ipcRenderer.invoke`, `ipcRenderer.send`, `webContents.send`,
      and all `IPCServer`/channel registrations
- [ ] Create `compat/ipc-contract.json`
- [ ] Group services into: Window, Dialog, Menu, Clipboard, Storage, Files,
      Terminal, Search, Process/tasks, Extensions, Update
- [ ] Add contract tests for each group under `compat/tests/`

### Acceptance

- [ ] IPC contract checked into repo
- [ ] Contract tests run in CI

---

## Phase 3: Window, Dialog, Clipboard, and Storage

### Status: ⬜ Not started

### Tasks

- [ ] Window service (title, size, state, fullscreen, zoom, custom title bar)
- [ ] Native dialog service (open/save/message boxes)
- [ ] Native menu + context menu service
- [ ] Clipboard service (text, html, image, resources)
- [ ] Storage/state persistence with SQLite parity
- [ ] Expose through Tauri IPC with the same channel names
- [ ] Keep the Electron implementation until tests pass

### Acceptance

- [ ] File open/save dialogs work
- [ ] Window title, size, fullscreen, zoom work
- [ ] Clipboard copy/paste works
- [ ] Settings and window state persist after restart

---

## Phase 4: File System Service

### Status: ⬜ Not started

### Tasks

- [ ] read/write/create/delete/rename/copy/stat/readdir
- [ ] File watching with Rust `notify` matching VS Code watcher semantics
- [ ] Atomic writes, write locks, and unlock-on-save
- [ ] Streaming for large files
- [ ] Encoding + BOM detection and conversion
- [ ] Workspace folder APIs, trash/recycle-bin delete
- [ ] Search traversal hooks

### Acceptance

- [ ] Open/save/edit works identically
- [ ] Watchers fire correctly, external changes detected
- [ ] Large file edits do not block the UI

---

## Phase 5: Terminal Service

### Status: ⬜ Not started

### Tasks

- [ ] PTY service with `portable-pty` (ConPTY backend)
- [ ] Spawn default shell on Windows, profile detection
- [ ] write / read / resize / kill / exit / persistence & reconnect
- [ ] xterm.js renderer IPC integration
- [ ] Shell integration scripts and command decorations
- [ ] cwd and environment handling, env-var collections

### Acceptance

- [ ] Integrated terminal opens and accepts input
- [ ] Resize, multiple terminals, Ctrl+C/kill work
- [ ] Shell integration behaves like the original

---

## Phase 6: Search and Process Services

### Status: ⬜ Not started

### Tasks

- [ ] Text search using the `grep`/`ignore` crates (ripgrep parity)
- [ ] File (fuzzy path) search
- [ ] Preserve include/exclude glob, .gitignore, symlink, binary-detect behavior
- [ ] Process service for tasks with env inheritance
- [ ] Kill/exit-code handling and task output streaming

### Acceptance

- [ ] Workspace search matches original results and ordering
- [ ] Tasks run, stream output, report exit codes, and can be killed

---

## Phase 7: Extension Host Integration

### Status: ⬜ Not started

### Tasks

- [ ] Keep the original extension host as a Node.js sidecar
- [ ] Bundle the Node runtime as a Tauri sidecar binary
- [ ] Replace Electron IPC transport with named-pipe/stdio JSON-RPC
- [ ] Extension management in Rust: discover, read `package.json`,
      install/update/uninstall VSIX, enablement state
- [ ] Extension webview host through Tauri child webviews
- [ ] Marketplace gallery service, activation events, API compatibility

### Acceptance

- [ ] Existing extensions install and activate
- [ ] Extension webviews render
- [ ] Marketplace install/update/uninstall works
- [ ] Extension terminal and filesystem access work

---

## Phase 8: Debug, Tasks, and Language Features

### Status: ⬜ Not started

### Tasks

- [ ] DAP adapter spawning, Debug Console output, prelaunch tasks
- [ ] Language server spawning through extensions (full IntelliSense)
- [ ] Built-in TS/JS server, extension output channels

### Acceptance

- [ ] JS/TS debugging works
- [ ] Completions, hovers, diagnostics, go-to-definition, refactors work
- [ ] Debug UI matches original

---

## Phase 9: Settings, Keybindings, and Accessibility

### Status: ⬜ Not started

### Tasks

- [ ] Settings editor, profiles, sync
- [ ] Keybinding service, native keyboard layout mapping, chords
- [ ] Accessibility / screen reader paths
- [ ] Locale, NLS, font, and high-DPI handling

### Acceptance

- [ ] Settings apply immediately
- [ ] Keybindings and conflicts resolve identically
- [ ] Accessibility does not regress

---

## Phase 10: Full Parity Audit and Cleanup

### Status: ⬜ Not started

### Tasks

- [ ] Run VS Code smoke tests and the integration suite against the Tauri build
- [ ] Build a feature checklist from original VS Code
- [ ] Automated side-by-side comparison: startup, memory, terminal,
      extensions, file operations
- [ ] Remove Electron main files file-by-file in isolated cleanup commits
- [ ] Keep the extension host Node sidecar
- [ ] Update `ROADMAP.md` with each cleanup commit

### Acceptance

- [ ] No Electron dependency remains in the shell
- [ ] All features pass the checklist

---

## Phase 11: Release Engineering and Optimization

### Status: ⬜ Not started

### Tasks

- [ ] Signed NSIS installer
- [ ] Automatic GitHub release on successful build
- [ ] Updater integration
- [ ] Startup, memory, and CPU profiling vs. the Electron baseline
- [ ] User-facing changelog generation

### Acceptance

- [ ] NSIS installer released automatically
- [ ] Release notes present
- [ ] Installer works on a clean Windows VM
- [ ] Startup and memory beat the Phase 0 Electron baseline
