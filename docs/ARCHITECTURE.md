# VSTauri Architecture

This document describes how VSTauri is built, why it is built this way, and
where the project is going. It is the map new contributors should read first.

## 1. Design principles

1. **Native where it counts.** Anything that touches the OS — filesystem,
   processes, terminals, git, language servers — lives in Rust. The web layer
   only renders and dispatches intent.
2. **The workbench is data, not DOM soup.** All workbench state lives in
   typed stores (zustand). Components are thin projections of store state.
3. **No hidden magic.** Every backend capability is an explicit, reviewed
   Tauri command. No `NodeIntegration`, no ad-hoc `eval`.
4. **Iterate commit by commit.** Every feature lands as a reviewable commit on
   top of a working app, the same way the original editor was built.

## 2. Process model

```
┌────────────────────────────────────────────────────────────┐
│ VSTauri (single native process)                            │
│                                                            │
│  ┌──────────────────────────┐   ┌───────────────────────┐  │
│  │ Rust core (Tauri 2)      │   │ OS webview window     │  │
│  │ - command handlers       │◄─►│ - React workbench UI  │  │
│  │ - state (pty/lsp/watch)  │   │ - Monaco, xterm.js    │  │
│  └───┬──────────┬───────────┘   └───────────────────────┘  │
│      │          │                                          │
└──────┼──────────┼──────────────────────────────────────────┘
       │          │
   spawn       spawn
       │          │
┌──────▼───┐ ┌────▼─────────────┐ ┌──────────────────┐
│ shells   │ │ language servers │ │ future: WASM     │
│ (PTYs)   │ │ (LSP over stdio) │ │ extensions       │
└──────────┘ └──────────────────┘ └──────────────────┘
```

Unlike Electron, there is **no bundled Chromium and no bundled Node runtime**.
The webview is the OS one. Shells and language servers are children of the
Rust process and are reaped deterministically (see `pty::PtyState::kill_all`,
wired to `CloseRequested`).

## 3. Backend modules (`src-tauri/src/`)

| Module | Responsibility | Key crates |
|---|---|---|
| `fs.rs` | list/read/write/create/rename/delete, binary sniffing, Quick Open file index | `std::fs`, `ignore` |
| `watcher.rs` | recursive debounced watch → `fs-changed` events | `notify`, `notify-debouncer-full` |
| `pty.rs` | PTY sessions; output pump threads; `ChildKiller`-based kill | `portable-pty` |
| `search.rs` | workspace search (background task, progress events) | `regex`, `ignore` |
| `gitcmd.rs` | porcelain status parsing, stage/unstage/commit/diff/show | `git` CLI |
| `lsp.rs` | generic stdio LSP client: framing, request/response correlation, notifications → events | `tokio`, `serde_json` |
| `ext/` | extension manifests, discovery, runtime trait (WASM in Phase 3) | `serde` |

### Why the git CLI instead of libgit2?

Phase 0 optimizes for buildability everywhere: the CLI is always present, has
zero build-system impact (no cmake/perl chains), and covers status/stage/
commit/diff. `git2` (libgit2) is a candidate later for in-process watch-style
performance; the command surface (`gitcmd`) is designed so the frontend cannot
tell the difference.

### LSP client design

- One server per language id, spawned lazily on first file of that language.
- Framing: `Content-Length` headers, full-document sync (simplest and
  universally supported), UTF-16 positions matching Monaco natively.
- Requests get `oneshot` channels; responses are correlated by id; a 30 s
  initialize timeout and 5 s request timeouts keep the UI responsive.
- `textDocument/publishDiagnostics` is bridged to the frontend as an event and
  converted to Monaco markers plus Problems-panel rows.
- Per-workspace override file: `.vstauri/lsp.json`.

## 4. Frontend (`src/`)

```
src/
├── main.tsx            entry — fonts, codicons, xterm css, workbench css
├── monaco.ts           Monaco worker bootstrap (Vite ?worker), theme install
├── theme/darkplus.ts   the real Dark+ token colors + xterm palette
├── ipc.ts              typed invoke wrappers + event listeners
├── commands.ts         command registry (palette entries + handlers)
├── hooks/useKeybindings.ts   global Ctrl-key layer
├── state/              zustand stores — single source of truth
│   ├── workspaceStore  root, tree cache, expanded dirs, recents
│   ├── editorStore     tabs, buffers (text/dirty/version), problems
│   ├── terminalStore   pty sessions
│   ├── gitStore        status/branch/changes
│   ├── searchStore     query flags + results
│   └── uiStore         layout, palette, menus, dialogs, cursor
└── components/         titlebar · activitybar · sidebar/* · editor/*
                         panel/* · palette · menus · dialogs
```

### State flow rules

- Components never call `invoke` directly for state mutations; they call store
  actions which call `ipc.ts`.
- Backend events (`fs-changed`, `search-progress`, `search-done`,
  `lsp-diagnostics`, `pty-output-*`) are subscribed **once** and dispatched
  into stores; components just render.
- Monaco models are owned by `MonacoPane`'s module-level registry (path →
  model), preserving unsaved state and view state per tab.

### The extension system (Rust-native)

Phase 0 ships the **contract**:

```jsonc
// ~/.vstauri/extensions/acme.hello/extension.json
{
  "id": "acme.hello",
  "name": "hello",
  "publisher": "acme",
  "version": "0.1.0",
  "description": "Says hello",
  "main": "extension.wasm",              // executed by wasmtime (Phase 3)
  "activationEvents": ["onStartup"],
  "contributes": {
    "commands": [{ "command": "hello.world", "title": "Hello World" }],
    "keybindings": [{ "command": "hello.world", "key": "ctrl+alt+h" }],
    "themes": [{ "label": "Midnight", "path": "midnight.json", "kind": "dark" }]
  }
}
```

Phase 3 will execute `main` inside **wasmtime** with a capability-based host
API (`vstauri.*` namespace): commands, editor edits, notifications, scoped
storage, and workspace fs access gated by manifest permissions. WASM gives us
sandboxing, fast cold-start, and language-agnostic authoring (Rust, Go, C, AS)
— the trade-off is that VSCode JS extensions will not run; that is the price
of the native-API direction chosen for this project.

## 5. Theming

`theme/darkplus.ts` installs the authentic Dark+ rules into Monaco (comments
`#6a9955`, strings `#ce9178`, keywords `#569cd6`, …) and the workbench CSS
variables mirror VSCode's palette (`--bg-side: #252526`, status bar
`#007acc`, activity bar `#333333`, …). A future theme engine will map
`contributes.themes` JSON onto these same variables.

## 6. Performance notes

- Editor tab switch is O(1): models persist, only `setModel` + view state
  restore happen.
- Search streams progress and caps results (5000 hits / 20000 files) so the
  UI never blocks; it runs on a background task of Tauri's tokio runtime.
- PTY output is pumped on dedicated threads and batched as base64 events —
  no JSON array boxing of bytes.
- The tree is lazily materialized per directory; `fs-changed` refreshes only
  loaded directories.

## 7. Known Phase-0 limitations

- Intermediate commits on this branch are logical snapshots; the canonical
  always-buildable tree is the branch head (CI verifies it).
- Monaco keeps disposed-tab models in memory to preserve undo/view state
  (bounded by session length; LRU eviction planned).
- File reads are capped at 5 MB (binary/very large files are refused with a
  message tab) — streaming/lazy loading is on the roadmap.
- LSP does full-document sync (fine for typical file sizes; incremental sync
  planned with the settings layer).
- No drag-reorder of tabs yet; middle-click close and keyboard flows exist.
