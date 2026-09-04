# Phase 1 — Electron Backbone → Tauri Backbone Inventory

This is a mapping/inventory for replacing the Electron main-process surface
with Tauri equivalents. It does **not** decide VS Code functionality; it only
maps the native host pieces.

Legend:
- **Electron** = the current implementation to keep for reference.
- **Tauri** = the replacement target.
- **Keep old** = leave the current implementation untouched until the parity
  gate for that subsystem is green.

## Window / App Lifecycle

| Electron concept | Existing code to keep as reference | Tauri target |
|---|---|---|
| `app.whenReady()` / quit / relaunch | `src/vs/code/electron-main/main.ts` | `tauri::Builder::run()` + `RunEvent` |
| `BrowserWindow` | `src/vs/code/electron-main/app.ts` | `tauri::WebviewWindowBuilder` |
| Window open / close / focus / minimize / maximize | Electron `BrowserWindow` API | Tauri window methods |
| Window state restore | Electron `BrowserWindow` bounds store | Rust state + settings store |
| Single instance / second-instance | Electron `app.requestSingleInstanceLock()` | `tauri-plugin-single-instance` |
| Native menu | Electron `Menu` | Tauri `Menu` |
| Dock / taskbar icon | Electron `app.dock` / window icon | Tauri window/app icon |
| External links | Electron `shell.openExternal()` | Tauri opener plugin or Rust `open` |

## IPC

| Electron concept | Existing code to keep as reference | Tauri target |
|---|---|---|
| `ipcMain.handle()` | Electron main-process handlers | Tauri `#[tauri::command]` + `invoke_handler` |
| `ipcMain.on()` | Electron main-process event handlers | Tauri commands or channel events |
| `webContents.send()` | Electron renderer main-process events | Tauri `app.emit` / `window.emit` |
| Structured payloads | Electron IPC preserves objects | Tauri commands use serde JSON/structured types |
| Dialog | Electron `dialog` | `tauri-plugin-dialog` |
| Clipboard | Electron `clipboard` | `tauri-plugin-clipboard-manager` |
| File open / save / URI | Electron native dialogs + protocol handling | Rust dialog + custom protocol |

## Filesystem

| Electron concept | Existing code to keep as reference | Tauri target |
|---|---|---|
| `fs`-like host APIs | `src/vs/platform/files/**` | Rust `std::fs` / `tokio::fs` behind Tauri commands |
| File watching | Electron `fs.watch` / VS Code watcher services | notify / inotify / ReadDirectoryChangesW |
| Path normalization | `path` Node module | `dunce` + Rust `Path` |
| Symlinks / permissions / errors | Node `fs` behavior | Rust `std::fs` behavior mapped to VS Code error contract |
| Search/glob | `src/vs/platform/search/**` | Rust `globset`/`ignore` compatible with existing rules |

## Terminal / Processes / Shell

| Electron concept | Existing code to keep as reference | Tauri target |
|---|---|---|
| `child_process` spawn | `src/vs/platform/terminal/**`, tasks, debug | Rust process/PTY |
| PTY | `node-pty` | Rust `portable-pty` / `portable-pty`-based command |
| New terminal | Electron host terminal | Rust PTY job feeding the existing xterm widget |
| Environment, cwd, shell | Node `child_process` options | Rust `Command` / `CommandExt` |
| Kill / signals / process tree | Node process API | Rust signal handling + process tree logic |

## WebView Content

| Electron concept | Existing code to keep as reference | Tauri target |
|---|---|---|
| `BrowserWindow.loadFile()` | workbench dev html templates | Tauri custom protocol / asset handler |
| `webContents` preload | `src/vs/code/electron-main/**` | Tauri preload script (TS) |
| CSP / nonce / workbench templates | `src/vs/code/browser/workbench/*` | Keep untouched; fill templates in Rust |
| WebView controls / DevTools | Electron devtools | Tauri devtools |

## Native features to preserve parity on

- Native file pickers (open file, open folder).
- Native save dialogs.
- System clipboard (plain text, HTML, images where supported).
- External URI opening.
- Native notifications (when used by VS Code notifications).
- Window state and multi-window behavior.
- Drag-and-drop of files into the editor.
- File association / launching from Explorer/Finder.
- Context menus (if VS Code relies on native ones).

## Migration gate for `tauri.conf.json`

Do **not** add `src-tauri/tauri.conf.json` to the branch until the following
are both true:

1. The shell compiles with the Tauri toolchain in CI.
2. The shell loads the existing VS Code workbench web assets (same UI, same
   look and feel, same extension host wiring).

Adding the config earlier would make the release workflow publish an app that
does not yet match VS Code. Keep the workflow in its current guarded state until
then.
