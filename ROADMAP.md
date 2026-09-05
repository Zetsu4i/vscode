# VS Code Tauri Rewrite Roadmap

## Target

A native Windows application that is behaviorally identical to VS Code, but uses Tauri v2 instead of Electron.

## Architecture

```
+--------------------------------------------------------+
| Renderer: Original VS Code Workbench UI / Monaco       |
| Runs in WebView2                                       |
+----------------------+----------------------------------+
                       | Tauri IPC / JSON-RPC
+----------------------+----------------------------------+
| Rust/Tauri Backend                                      |
| - Window/layout                                          |
| - File system service                                    |
| - Terminal/service                                       |
| - Search service                                         |
| - Process service                                        |
| - Extension manager                                      |
| - Update service                                         |
+----------------------+----------------------------------+
                       | stdio / IPC
+----------------------+----------------------------------+
| Extension Host: Node.js sidecar                         |
| - Original VS Code extension host code                   |
| - Executes JS extensions                                 |
| - Provides VS Code API compatibility                     |
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
- [x] Keep Electron app still buildable in parallel (Electron tree untouched)

### Acceptance

- [ ] VS Code workbench renders in Tauri window
- [ ] Editor area renders Monaco
- [ ] No crash on initial startup
- [x] Original Electron build still works (compile job green on Windows CI)

---

## Phase 2: IPC Contract Extraction

### Status: ⬜ Not started

### Goal

Catalog every Electron main/renderer IPC surface before changing it.

### Tasks

- [ ] Scan original source for:
  - [ ] `ipcMain.handle`
  - [ ] `ipcMain.on`
  - [ ] `ipcRenderer.invoke`
  - [ ] `ipcRenderer.send`
  - [ ] `webContents.send`
- [ ] Create `compat/ipc-contract.json`
- [ ] Group services into:
  - [ ] Window
  - [ ] Dialog
  - [ ] Menu
  - [ ] Clipboard
  - [ ] Storage
  - [ ] Files
  - [ ] Terminal
  - [ ] Search
  - [ ] Process/tasks
  - [ ] Extensions
  - [ ] Update
- [ ] Add contract tests for each group

### Acceptance

- [ ] IPC contract checked into repo
- [ ] Contract tests run in CI

---

## Phase 3: Window, Dialog, Clipboard, and Storage

### Status: ⬜ Not started

### Goal

Replace Electron main process basics with Tauri/Rust services.

### Tasks

- [ ] Implement window service
- [ ] Implement native dialog service
- [ ] Implement clipboard service
- [ ] Implement storage/settings persistence
- [ ] Expose through Tauri IPC with the same channel names
- [ ] Keep old Electron implementation until tests pass

### Acceptance

- [ ] File open/save dialogs work
- [ ] Window title, size, and fullscreen work
- [ ] Clipboard copy/paste works
- [ ] Settings persist after restart

---

## Phase 4: File System Service

### Status: ⬜ Not started

### Goal

Replace Electron file service with Rust file service.

### Tasks

- [ ] Implement read/write/create/delete/rename/copy
- [ ] Implement file watching with Rust `notify`
- [ ] Match VS Code watcher behavior
- [ ] Add support for atomic writes
- [ ] Add support for large files
- [ ] Add encoding and BOM handling
- [ ] Add workspace folder APIs
- [ ] Add search file traversal hooks

### Acceptance

- [ ] Open/save/edit files works identically
- [ ] Editor file watchers fire correctly
- [ ] External file change detection works
- [ ] Large file edits do not block UI

---

## Phase 5: Terminal Service

### Status: ⬜ Not started

### Goal

Replace Electron/node-pty terminal backend with Rust PTY.

### Tasks

- [ ] Implement PTY service with `portable-pty`
- [ ] Spawn default shell on Windows
- [ ] Support write, read, resize, kill, exit
- [ ] Integrate xterm.js renderer IPC
- [ ] Preserve shell integration behavior
- [ ] Preserve cwd and environment handling

### Acceptance

- [ ] Integrated terminal opens and accepts input
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

## Phase 7: Extension Host Integration

### Status: ⬜ Not started

### Goal

Preserve extension host while replacing the Electron extension main bridge.

### Tasks

- [ ] Keep original VS Code extension host as Node.js sidecar
- [ ] Bundle Node runtime sidecar if needed
- [ ] Replace Electron IPC transport with stdio or JSON-RPC
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
