# The Tauri shell

How the native shell hosts the unmodified VS Code workbench, and why each piece
exists. Read `AGENTS.md` first for the constraints this design satisfies.

## The one-sentence version

The renderer is the **real VS Code desktop bundle**. Only the process that owns
the window changed: Electron's main process was replaced by a Rust/Tauri
process, and Electron's preload script was replaced by a shim that presents the
same `globalThis.vscode` object.

## Why not `vscode-web`?

`vscode-web` / `code serve-web` is the browser build. It has no native window,
no ConPTY, no local extension host, and it needs an HTTP server in the
background — which is exactly what a "native, fast, real" build must not be
(`AGENTS.md` hard constraint 11). It also loses the desktop-only services
(native menus, window state, file watching, local extension host), so it can
never reach feature parity.

This shell therefore loads `out-vscode-min/vs/code/electron-browser/workbench/`
— the **desktop** entry point — straight from disk.

## The three seams

VS Code's renderer only touches the outside world through three narrow seams.
Reproducing all three is the whole job; nothing else in `src/vs` needs to
change.

### 1. Asset loading — the `vscode-file://` protocol

Electron's main process registers a `vscode-file` scheme and the renderer
builds every asset URL with `FileAccess.asBrowserUri` →
`vscode-file://vscode-app/<absolute path>`. `workbench.ts` also derives
`globalThis._VSCODE_FILE_ROOT` from `configuration.appRoot` using that scheme.

`src-tauri/src/protocol.rs` registers the *same* scheme in Tauri. It:

- percent-decodes the path and strips query/fragment,
- handles the Windows `/C:/...` form,
- lexically normalizes and rejects `..` traversal,
- refuses anything outside the allowed roots (app root, out dir, extensions,
  user data),
- and returns the correct MIME type.

MIME correctness is not cosmetic. A `.js` served as `text/plain` makes an ES
module import fail silently and the window boots blank; a `.css` served as
`text/plain` is dropped and the UI renders unstyled. `--self-check` asserts
these types in CI.

### 2. Boot configuration — `INativeWindowConfiguration`

`workbench.ts` awaits `preloadGlobals.context.resolveConfiguration()` before it
imports `workbench.desktop.main.js`, then immediately dereferences
`configuration.appRoot`, `.product`, `.nls`, `.colorScheme`, `.profiles`,
`.userDataDir` and more. A missing field is a hard boot failure.

`src-tauri/src/window_config.rs` builds that payload in Rust with the field
names from `src/vs/platform/window/common/window.ts`. The unit tests assert
every field the renderer dereferences is present, and `env_map()` forwards an
allow-list rather than the whole environment so secrets never reach the webview.

### 3. Native calls — `globalThis.vscode`

`src/vs/base/parts/sandbox/electron-browser/globals.ts` destructures six
members off `globalThis.vscode` at module load. If any is missing the workbench
throws before it paints.

`src-tauri/preload/shim.js` is injected as an initialization script and
provides all six, backed by Tauri commands:

| Member | Backing |
| --- | --- |
| `ipcRenderer.send/invoke` | `ipc_send` / `ipc_invoke` → `IpcRouter` |
| `ipcRenderer.on/once/removeListener` | local listener table + a push channel from Rust |
| `ipcMessagePort.acquire` | renderer-created `MessageChannel`, Rust pumps the far end |
| `webFrame.setZoomLevel` | document zoom + native window zoom |
| `webUtils.getPathForFile` | native drop-path table |
| `process` | window configuration + `fetch_shell_env` |
| `context` | `resolve_window_configuration` |

It also installs `global`, `setImmediate`/`clearImmediate` (via `MessageChannel`,
not `setTimeout(0)`, to keep the 4 ms clamp out of the editor's hot paths).

#### MessagePort is the interesting one

Electron transfers a real `MessagePort` from the main process to the renderer.
Tauri cannot transfer ports. The shim inverts it: the **renderer** creates the
`MessageChannel`, keeps `port1`, and posts `port2` to itself with the nonce —
byte-identical to what `ipcMessagePort.acquire` produces in Electron, so
`ipc.mp.ts` needs no change. Rust then pumps `port1` against the real endpoint
(pty host in Phase 5, extension host in Phase 7).

## Channel naming

Every channel keeps its upstream `vscode:` name and the `validateIPC` guard
(only `vscode:`-prefixed channels cross the boundary) is reimplemented in
`ipc.rs::validate`. Channels that are not ported yet return a structured
`Unimplemented` error and log once — they never panic, and
`compat/ipc-contract.json` tracks each one against its roadmap phase.

## Building

```bash
# 1. Build the workbench (needs ~8 GB RAM; do this in CI or on a real machine)
npm ci
npm run gulp minify-vscode
npm run gulp compile-extensions-build

# 2. Verify the shell can serve it, headless
cd src-tauri
cargo test
cargo run -- --self-check --workbench-dir ../out-vscode-min

# 3. Run it
cargo run -- --workbench-dir ../out-vscode-min

# 4. Windows installer
cargo tauri build --config src-tauri/tauri.windows.conf.json
```

## `--self-check`

The headless regression guard CI runs. It proves:

- the workbench boot files exist where the resolver looks,
- each is served with the MIME type a webview requires,
- path traversal is rejected,
- the window configuration contains every field the renderer dereferences.

It prints `SELFCHECK_OK` on success; the workflow greps for that string.

## Current status

Phase 1 (shell prototype). Boot path is implemented; the native services are
stubs that fail loudly. See `ROADMAP.md` for what lands when. The Electron main
process is still present and buildable, per hard constraint 5.
