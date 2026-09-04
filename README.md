# VSTauri

**A from-scratch rebuild of the VSCode workbench on [Tauri 2](https://tauri.app) + Rust — lighter, faster, native.**

VSTauri is not a fork that patches Electron out of VSCode. It is a ground-up
rebuild: a Rust backend that owns the filesystem, the terminal, search and git;
a web frontend that reproduces the VSCode workbench pixel-for-pixel; and a
capability-based extension system designed natively for Rust.

Unaffiliated with Microsoft. "Visual Studio Code" and "VSCode" are trademarks
of their respective owners; this project is an independent editor inspired by
the same workflow.

---

## Status — Phase 1: Editing Depth

| Area | Status | Notes |
|---|---|---|
| Workbench shell (title bar, activity bar, side bar, status bar) | ✅ | Custom window chrome, real Dark+ palette |
| File explorer tree | ✅ | Lazy loading, context menus, create/rename/delete |
| File system backend | ✅ | Rust `std::fs` commands, binary detection, 5 MB guard |
| File watching | ✅ | `notify` + debounce, live tree refresh |
| Monaco editor | ✅ | Same engine as VSCode, tabs, dirty state, per-tab view state |
| **Editor groups (split editor)** | ✅ | Binary split grid, resizable splitters, tab drag-reorder + drag across groups |
| **LSP sync** | ✅ | Incremental ranged `didChange` + `didSave` |
| **Replace across workspace** | ✅ | Regex/literal/word flags, `$1` captures, open buffers stay in sync |
| **Breadcrumbs + sticky scroll** | ✅ | Path crumbs with explorer reveal; toggleable sticky scroll |
| **Editor settings** | ✅ | Font zoom, ligatures, minimap, word wrap, whitespace (persisted) |
| **Hex viewer** | ✅ | Virtualized offset/hex/ASCII grid for binaries; large-file banner |
| **Language configuration** | ✅ | Region folding, proto config, markdown list continuation |
| Terminal | ✅ | Real PTY (`portable-pty`), xterm.js, colors, resize, multiple terminals |
| Global search | ✅ | ripgrep's engine (`regex`) + walker (`ignore`), .gitignore-aware |
| Source control (git) | ✅ | status, stage/unstage, commit, Monaco diff editor |
| Command palette + Quick Open | ✅ | Fuzzy matching, Ctrl+Shift+P / Ctrl+P |
| LSP client | ✅ | stdio transport; diagnostics, hover, completions (rust-analyzer, tsserver, …) |
| Problems panel | ✅ | fed by LSP diagnostics |
| Extension system | 🟡 | manifests + discovery in place; WASM runtime is Phase 3 |
| Settings UI, themes, keybinding editor | ⬜ | Phase 2 |
| Debugger (DAP), tasks, remote | ⬜ | Phase 4+ |

## Why Tauri

- **Memory**: no Chromium + Node runtime shipped per app — uses the OS webview
  (WebKitGTK on Linux, WebView2 on Windows).
- **Startup**: native Rust process boot vs. Electron's Node bootstrap.
- **Footprint**: installer in the tens of MB instead of ~200 MB.
- **Security**: capability-based IPC; every backend command is explicit Rust.

## Getting Started

### Prerequisites

- **Rust** (stable): https://rustup.rs
- **Node.js ≥ 18** + npm
- **Linux only** — WebKitGTK dev packages:

```bash
sudo apt install libwebkit2gtk-4.1-dev build-essential libssl-dev libgtk-3-dev \
  libayatana-appindicator3-dev librsvg2-dev pkg-config
```

- **Windows**: WebView2 ships with Windows 11 / recent 10 builds; nothing else needed.

### Run in development

```bash
npm install
npm run tauri dev
```

### Build a release bundle

```bash
npm run tauri build
# Linux: .deb + .AppImage  → src-tauri/target/release/bundle/
# Windows: NSIS .exe + .msi
```

CI builds installers for Linux and Windows on every push to this branch —
see the **Actions** tab.

### Language servers (optional)

VSTauri auto-spawns well-known servers when you open matching files:

| Language | Server | Install |
|---|---|---|
| Rust | `rust-analyzer` | `rustup component add rust-analyzer` |
| TS/JS | `typescript-language-server` | `npm i -g typescript typescript-language-server` |
| Python | `pylsp` | `pip install python-lsp-server` |
| C/C++ | `clangd` | system package |
| Go | `gopls` | `go install golang.org/x/tools/gopls@latest` |

Override or add servers per workspace with `.vstauri/lsp.json`:

```json
{
  "python": { "command": "basedpyright", "args": ["--stdio"] }
}
```

## Keybindings

| Keys | Action |
|---|---|
| `Ctrl+Shift+P` | Command Palette |
| `Ctrl+P` | Quick Open (go to file) |
| `Ctrl+B` | Toggle side bar |
| `` Ctrl+` `` | Toggle terminal |
| `Ctrl+Shift+`` ` | New terminal |
| `Ctrl+S` | Save |
| `Ctrl+W` | Close editor |
| `Ctrl+N` | New file |
| `Ctrl+\` | Split editor right |
| `Ctrl+=` / `Ctrl+-` | Font zoom in / out |
| `Alt+Z` | Toggle word wrap |
| `Ctrl+Shift+E / F / G / M` | Explorer / Search / SCM / Problems |

## Architecture in one screen

```
┌──────────────────────────────────────────────────────────────┐
│  Web frontend (OS webview)                                   │
│  React + zustand + Monaco + xterm.js                         │
│  workbench shell · explorer · search · scm · palette · panel │
└───────────────▲──────────────────────────┬───────────────────┘
                │ events (fs-changed,      │ invoke (typed commands)
                │ pty-output, lsp-diag…)   │
┌───────────────┴──────────────────────────▼───────────────────┐
│  Rust backend (Tauri 2)                                      │
│  fs · watcher · pty(portable-pty) · search(regex+ignore)     │
│  gitcmd(git CLI) · lsp(stdio JSON-RPC client) · ext(manifest)│
└──────────────────────────────────────────────────────────────┘
```

Full details: [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) ·
Roadmap: [docs/ROADMAP.md](docs/ROADMAP.md)

## Extending VSTauri

Extensions are folders containing `extension.json` (manifest) — placed in
`~/.vstauri/extensions/<publisher.name>/` or `<workspace>/.vstauri/extensions/`.
See `examples/extensions/hello-world/` for the format. The WASM execution
runtime (wasmtime, capability-gated host API) is the next major milestone —
the manifest format, discovery and registry are already in place.

## License

MIT. See [LICENSE](LICENSE).
