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

## Phase 1 — Editing depth

- [ ] Incremental document sync for LSP
- [ ] Find & replace inside file (Monaco) and across workspace (search view)
- [ ] Tab drag-reorder, split editor (vertical/horizontal groups)
- [ ] Breadcrumbs; sticky scroll
- [ ] Language configuration per filetype (comments, brackets, folding)
- [ ] Large-file streaming reads; "Open in hex viewer" for binaries
- [ ] Editor settings: font size/ligatures, minimap toggle, word wrap, whitespace

## Phase 1.5 — Theme system & workbench chrome ✅

Product decision: the native rebuild is the main line — no more reliance
on hosting the upstream vscode-web workbench (too restricted). SideX
(github.com/Sidenai/sidex) was studied for architecture: it is the
Code-OSS workbench on Tauri (a plumbing reference, not a rebuild), and it
never shipped a working top menu bar on Windows/Linux nor a working
theme switcher. VSTauri does both natively:

- [x] Theme registry (`src/theme/themes.ts`): 5 pixel-faithful themes
      transcribed from the real microsoft/vscode theme definitions with
      include chains resolved — Dark+ (default dark), Dark Modern,
      Light+ (default light), Light Modern, Monokai
- [x] One theme = chrome CSS variables + Monaco token theme + xterm
      palette, applied atomically on <html>
- [x] Live-preview theme picker in the command palette (arrow keys
      preview the whole workbench, Enter commits, Escape reverts) and
      File > Preferences > Color Theme — the piece SideX never got
      working
- [x] Persistence via localStorage; first frame already themed
- [x] workbench.css fully variableized (no light-hostile hardcoded
      overlays left)
- [x] Complete VSCode menu bar with working editor actions
      (undo/redo/clipboard/find/replace/format/select-all/line ops)
      through `editorBridge.ts`; zoom via Tauri webview `setZoom`
- [x] New keybindings: Ctrl+Shift+X, Ctrl+J, Ctrl+G, Ctrl+=, Ctrl+-,
      Ctrl+0, Ctrl+H, Ctrl+/

## Phase 1.6 — Adopted from the SideX study ✅

- [x] PTY flow control: byte-count high-watermark (1 MiB pause / 256 KiB
      resume) + batched frontend acks (`pty_ack`), plus pre-attach buffering
      (`pty_attach`) so the shell prompt/MOTD is never lost (SideX's best
      terminal idea, done without its `window.eval` injection)
- [x] Window-state persistence: authoritative save on close + debounced
      saves on resize/move; logical-unit geometry; restored in `setup`
      while the window is hidden
- [x] Restore-and-show after first paint (`visible: false` in
      tauri.conf.json + `window_ready`) — kills the white-flash startup
- [x] Debounced workbench-layout persistence: sidebar width, sidebar/panel
      visibility, panel height + tab; applied on first render
- [x] Shell enumeration + default-shell detection (cmd, PowerShell 7+,
      Git Bash, WSL, PATH-scan extras; /etc/shells on Unix) with a terminal
      profile picker dropdown next to the "+" button
- [x] `vstauri://` asset protocol with traversal guards (no `..` components,
      canonicalize + root containment, 64 MiB cap) + image preview pane
      served straight from disk
- [x] Avoid SideX's failure modes: no dead parallel service layers, no
      build-time dependency on a local VSCode install, no `window.eval`
      event injection, no error-swallowing invoke wrappers

## Phase 2 — Personalization

- [ ] `settings.json` + Settings UI (scopes: user / workspace)
- [x] Theme engine core: workbench CSS variables + Monaco themes + xterm
      palettes (Phase 1.5) — `contributes.themes` support remains
- [ ] Icon themes
- [ ] Keybinding editor + user keybindings.json
- [ ] Profiles: recent workspaces store, layout persistence, session restore
- [ ] Auto-save, format-on-save (LSP `textDocument/formatting`)

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
