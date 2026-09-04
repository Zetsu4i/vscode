# VSTauri Roadmap

Built the way the original was built: **step by step, commit after commit.**
Each phase below lands as a series of small, reviewable commits on top of a
working app. Nothing merges if the app does not run.

## Phase 0 — Foundation ✅ (this branch)

- [x] Tauri 2 + Vite + React + TypeScript scaffold
- [x] Workbench shell: custom title bar, activity bar, side bar, status bar (Dark+)
- [x] Rust filesystem backend + explorer tree + file watching
- [x] Monaco editor, tab management, dirty tracking, save
- [x] Welcome page, window controls, menus
- [x] Real PTY terminal (portable-pty + xterm.js), multiple terminals
- [x] Workspace search on ripgrep's engine + walker
- [x] Git panel: status, stage/unstage, commit, diff editor
- [x] Command palette + Quick Open + keybinding layer
- [x] LSP client: diagnostics, hover, completions; Problems panel
- [x] Extension manifests + discovery + runtime trait
- [x] CI: Linux (deb/AppImage) + Windows (NSIS/MSI) installers

## Phase 1 — Editing depth ✅

- [x] Incremental document sync for LSP (ranged `didChange` + `didSave`)
- [x] Find & replace inside file (Monaco find widget) and across workspace
      (search view replace-all with confirmation; open buffers resync)
- [x] Tab drag-reorder, split editor (vertical/horizontal groups, resizable
      binary split grid, tab drag across groups)
- [x] Breadcrumbs (path crumbs with explorer reveal); sticky scroll
- [x] Language configuration per filetype (region folding markers, proto
      config, markdown list continuation)
- [x] Large-file handling: 256 KB capped byte reads + virtualized hex viewer
      for binaries; truncation banner for oversized text files
- [x] Editor settings: font size/ligatures, minimap toggle, word wrap,
      whitespace rendering, tab size (persisted; palette commands + keys)

## Phase 2 — Personalization ✅

- [x] `settings.json` + Settings UI (scopes: user / workspace)
- [x] Theme engine: `contributes.themes` → workbench CSS variables + Monaco themes
- [x] Icon themes (inline-SVG "VSTauri Color" set + Minimal)
- [x] Keybinding editor + user keybindings.json (press-to-capture, unbind rules)
- [x] Session restore (editors + split layout per workspace) and window layout persistence; recent workspaces on the welcome page
- [x] Auto-save (`afterDelay` + delay setting), format-on-save via LSP `textDocument/formatting`

## Phase 3 — Extensions

- [ ] wasmtime-based WASM runtime sandbox (fuel-limited, capability-gated)
- [ ] Host API v1: `commands`, `editor` (get/set selection, decorations),
      `window` (show info/warning/error), `workspace.fs` (scoped, permissioned)
- [ ] Extension activation events: `onStartup`, `onCommand:…`, `onLanguage:…`
- [ ] Extension manager UI: enable/disable/uninstall
- [ ] Sample extensions: Hello World (Rust), Theme pack, Todo highlighting
- [ ] Optional `.vsix`-style packaging (zip of folder + manifest)

## Phase 4 — IDE depth

- [ ] Debug Adapter Protocol client (breakpoints, variables, call stack, REPL)
- [ ] Tasks (`tasks.json`) and problem matchers
- [ ] Inline git blame + gutter decorations; branch switching UI
- [ ] Multi-root workspaces
- [ ] Remote development groundwork: edit-over-SSH via backend transport swap

## Phase 5 — Polish & scale

- [ ] Model LRU eviction + undo stack limits
- [ ] Search replace-all with preview diff
- [ ] Localization framework
- [ ] macOS target (icns, notarization path)
- [ ] Telemetry-free usage analytics (opt-in, local-only)

## Working agreement

1. Every commit leaves the app runnable (`cargo check` + `npm run build`).
2. Every phase updates this file and the README status table.
3. New backend commands must be added to `capabilities` review — no blanket
   permissions.
