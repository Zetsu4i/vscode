# Target Architecture — VS Code on Tauri 2

Companion to `AGENTS.md` (the law) and `ROADMAP.md` (the plan). This page describes what we are building. Decisions live in [`adr/`](adr/).

## The three planes

```
┌────────────────────────────────── Tauri 2 app ──────────────────────────────────┐
│                                                                                 │
│  ┌── Rust native plane (src-tauri/) ──┐      ┌── Web plane (system webview) ──┐ │
│  │ window mgmt · menus · dialogs      │◄─IPC─┤ The genuine VS Code workbench  │ │
│  │ fs · search · watcher · pty        │      │ (src/vs UI, Monaco, xterm.js,  │ │
│  │ storage (SQLite) · secrets         │      │  themes — byte-for-byte VS Code)│ │
│  │ process exec · updates             │      │ + typed service adapters       │ │
│  └──────────────┬─────────────────────┘      └──────────────▲─────────────────┘ │
│                 │ manages / feeds                            │ speaks            │
│  ┌──────────────▼─────────────────────┐      ┌──────────────┴─────────────────┐ │
│  │ Extension-host plane               │◄─────┘ vscode extension APIs (ADR-0002) │ │
│  │ Node-compatible JS runtime,        │                                         │
│  │ supervised by Rust; upstream       │                                         │
│  │ extension-host TS runs inside      │                                         │
│  └────────────────────────────────────┘                                         │
└─────────────────────────────────────────────────────────────────────────────────┘
```

- **Web plane** — the real VS Code workbench, built from this repo with the upstream web bundling tasks (`build/gulpfile.vscode.web.ts`) and served inside the Tauri webview. This is what the user sees; it stays exactly the VS Code UI.
- **Rust native plane** — everything Electron's main process and node-side services used to do. Exposed to the web plane as typed IPC commands that mirror upstream service interfaces (`IFileService`, `ISearchService`, `IPtyService`, …). One Rust module per upstream service (`src-tauri/src/services/<name>.rs`).
- **Extension-host plane** — extensions are JavaScript and must run unmodified (`AGENTS.md` M-0.3). Rust owns lifecycle/supervision/backends; a Node-compatible JS runtime executes extension code (ADR-0002).

## IPC design

- Typed commands, one Rust service module per upstream service; one TS adapter per service implementing the upstream interface, injected at the web-workbench factory seam (ADR-0001).
- Evented services (fs events, pty data, logs) use multiplexed channels; payload shapes match upstream events exactly.
- All TS-side integration points are marked `// tauri: seam` and concentrated in designated files — no scattered edits.

## What we deliberately do NOT rewrite

- The workbench UI, Monaco, themes, layouts — that is the product itself (M-0.4).
- Git: shell out to the git CLI exactly like upstream does.
- The upstream Rust `cli/` crate (tunnels, etc.) — reused as-is.

## Boot sequence (target)

1. Tauri spawns → Rust shell reads `product.json`, restores windows/session like the Electron main process did.
2. Window webview loads the workbench bundle from `dist-tauri/` via the custom protocol.
3. Workbench instantiates its services → TS adapters bind to Tauri IPC → Rust answers.
4. Extension-host plane starts per ADR-0002.

## Decision records

| ADR | Title | Status |
|---|---|---|
| [ADR-0001](adr/ADR-0001-integration-seam.md) | Integration seam between workbench and Rust backend | accepted |
| [ADR-0002](adr/ADR-0002-extension-host.md) | Extension host strategy | accepted |
| [ADR-0003](adr/ADR-0003-native-services.md) | Rust crates for native services | accepted |

New consequential decisions get new ADR files; keep this table updated in the same commit.
