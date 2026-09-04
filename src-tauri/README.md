# Tauri Shell — Phase 1 Design Notes

This directory is reserved for the Rust/Tauri migration. It intentionally does
**not** contain a buildable app yet. Adding a `tauri.conf.json` here will enable
the `Tauri Release Build` workflow, so it must only be added once the shell has
a real path to load the existing VS Code workbench.

## Goals

- Replace the Electron main process with a Tauri (Rust + WebView) host.
- Keep the **existing VS Code workbench UI** (Monaco, workbench, settings,
  extension UI) untouched.
- Replace only the native backbone: windowing, filesystem, terminal/PTY,
  process spawning, IPC, native menus, native dialogs, clipboard, and commands.
- Keep the extension host API contract the same so existing VS Code extensions
  keep working.

## Architecture Direction

```
+----------------------- Tauri Rust host (Rust) -----------------------+
|  window / app lifecycle                                             |
|  native menu / dialog / clipboard / URIs                            |
|  filesystem, watchers, process/PTY, settings, state                 |
|  Tauri commands + events exposed over IPC                           |
+-------------------------------------+-------------------------------+
                                      |
                       Tauri WebView (system WebView2 / WebKitGTK / WKWebView)
                                      |
                    VS Code Workbench web UI (unchanged)
                    src/vs/code/browser/workbench/*
```

The existing web workbench entry points are:

- `src/vs/code/browser/workbench/workbench.html`
- `src/vs/code/browser/workbench/workbench.ts`

These files are template-driven and need the compiled assets (`out/vs/...`) to be
served by the Tauri host. The Tauri shell should serve them through a custom
protocol or Tauri asset handler, not through a redesigned UI.

## Electron Backbone Modules To Replace Later

- `src/vs/code/electron-main/app.ts`
- `src/vs/code/electron-main/main.ts`
- `src/vs/code/electron-utility/sharedProcess/sharedProcessMain.ts`
- Electron main-process services in `src/vs/platform/*` (files, terminal,
  dialogs, menus, clipboard, windows, lifecycles).

## Phase 1 Work Items

1. Create `src-tauri/Cargo.toml`, `src-tauri/src/main.rs`,
   `src-tauri/tauri.conf.json`, and `src-tauri/build.rs`.
2. Define how the workbench assets are produced and copied into the Tauri
   bundle.
3. Add a custom protocol so the WebView can load:
   - `workbench.html` (template-expanded)
   - `out/vs/code/browser/workbench/workbench.js`
   - the workbench CSS/fonts
4. Implement window creation, single-instance behavior, state persistence,
   native menus, and app lifecycle.
5. Add Windows NSIS bundling config.
6. Keep `src-tauri/tauri.conf.json` **out of the branch** until the shell can
   actually load the workbench. Otherwise the release workflow would create
   releases that are not real VS Code builds.

## Restrictions

- Do **not** commit `src-tauri/tauri.conf.json` as a placeholder if it cannot
  load the VS Code workbench yet. That would make CI create a misleading
  release.
- Do **not** rewrite `workbench.html`, `workbench.ts`, Monaco, or the
  workbench CSS.
- Do **not** invent new extension APIs or a new command registry.
