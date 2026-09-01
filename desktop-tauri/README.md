# Code - OSS on Tauri

A Tauri-based desktop shell for this VS Code fork that replaces Electron while keeping
**100% of VS Code's functionality**: the full workbench, marketplace extensions, the
integrated terminal, real filesystem access, debugging, tasks, source control — everything.

## Why this architecture

VS Code's compatibility promise is anchored in its **Node.js extension host**: every
marketplace extension expects a real Node.js runtime with native modules (`node-pty`
for terminals, filesystem watchers, git integrations, language servers, ...). Rewriting
that in Rust would break the entire extension ecosystem.

So instead of rewriting the workbench, this shell reuses the piece of VS Code that was
*designed* to run the whole product outside Electron: the **VS Code server**
(`vscode-reh-web`, the same build that powers vscode.dev remotes, Codespaces and
openvscode-server). The Tauri app:

```
┌───────────────────────────────────────────────────────────────┐
│  Tauri shell (Rust, ~5 MB)                                    │
│  window management · single instance · sidecar lifecycle      │
│                                                               │
│  ┌─────────────────────────┐   loopback HTTP/WebSocket        │
│  │ OS webview              │ ─────────────────────────────┐   │
│  │ (WebView2 / WKWebView / │   http://127.0.0.1:<random>  │   │
│  │  WebKitGTK)             │   ?tkn=<secret token>        │   │
│  │ renders the workbench   │                              ▼   │
│  └─────────────────────────┘   ┌───────────────────────────┐  │
│                                │ VS Code server (sidecar)  │  │
│                                │ · Node.js extension host  │  │
│                                │ · node-pty terminals      │  │
│                                │ · full filesystem access  │  │
│                                │ · search, git, watchers   │  │
│                                └───────────────────────────┘  │
└───────────────────────────────────────────────────────────────┘
```

Because the workbench and the extension host are the *unmodified* VS Code builds,
everything works exactly as in the Electron app — while the Chromium runtime
(~150–200 MB on disk, hundreds of MB of RAM) is replaced by the webview the OS
already ships.

### Security model

- The server binds to `127.0.0.1` only, on a random free port.
- Every request must carry a per-launch secret connection token (UUIDv4), so no other
  local process can connect to your files or terminals.
- The webview loads only that loopback origin; Tauri IPC is not exposed to it.

## What replaced what

| Electron (`src/vs/code/electron-main`)     | Tauri shell                                             |
| ------------------------------------------ | ------------------------------------------------------- |
| Chromium + Node in the renderer            | OS webview (WebView2 / WKWebView / WebKitGTK)           |
| Electron main process / window management  | `src-tauri/src/main.rs`                                 |
| `sharedProcess` / local extension host     | VS Code server sidecar (`src-tauri/src/server.rs`)      |
| `requestSingleInstanceLock`                | `tauri-plugin-single-instance`                          |
| Native menu bar                            | VS Code's built-in custom menu bar (web mode)           |
| Terminals via `node-pty` in Electron       | `node-pty` in the server — unchanged                    |
| Extensions/marketplace                     | Server extension host — unchanged                       |
| File dialogs                               | Workbench's remote-aware file pickers — unchanged       |

## Building

### Prerequisites

- Everything required to build VS Code itself (see [CONTRIBUTING.md](../CONTRIBUTING.md))
- Rust (stable, 1.77+) — <https://rustup.rs>
- Tauri platform dependencies:
  - **Linux**: `libwebkit2gtk-4.1-dev libgtk-3-dev libayatana-appindicator3-dev librsvg2-dev patchelf`
  - **Windows**: MSVC Build Tools + WebView2 (preinstalled on Win 11)
  - **macOS**: Xcode command line tools

### Release build

```bash
# 1. From the repository root: install dependencies once
npm ci

# 2. Build + stage the VS Code server for your platform
cd desktop-tauri
npm install
npm run prepare-server          # runs gulp vscode-reh-web-<platform>-<arch>-min

# 3. Generate the full icon set (.icns/.ico) and build installers
npm run icons
npm run build                   # deb/rpm/AppImage, msi/nsis, dmg/app
```

Installers land in `desktop-tauri/src-tauri/target/release/bundle/`.

CI can do the same on all three platforms: copy
[`desktop-tauri/ci/tauri-desktop.yml`](ci/tauri-desktop.yml) to
`.github/workflows/tauri-desktop.yml` (kept here because pushing workflow files
requires the `workflows` permission) and it will build deb/rpm/AppImage, msi/nsis
and dmg bundles on every change to `desktop-tauri/`.

### Development loop

```bash
# Repo root: compile VS Code sources (or keep `npm run watch` running)
npm run compile

# Run the shell — it auto-detects the repo checkout and runs
# `node <repo>/out/server-main.js` directly
cd desktop-tauri
npm install
npm run dev
```

Workbench changes are picked up on reload without repackaging. To point the shell at
a different server build, set `VSCODE_TAURI_SERVER_DIR` to an **absolute** path of a
`vscode-reh-web-*` folder or repository checkout.

### Runtime configuration

| Variable                  | Effect                                                              |
| ------------------------- | ------------------------------------------------------------------- |
| `VSCODE_TAURI_SERVER_DIR` | Use this server build (a `vscode-reh-web-*` folder or a repo checkout) instead of the bundled one. |
| `RUST_LOG`                | Shell log level (`info` by default); server output is forwarded here. |

Launching with a folder argument opens it in the workbench: `code-oss-tauri ~/projects/app`.

## Startup sequence

1. `main.rs` creates the window showing the bundled splash (`splash/index.html`).
2. `server.rs` locates a server build (env var → bundled resource → repo checkout)
   and spawns it: `--host 127.0.0.1 --port 0 --connection-token <uuid>
   --accept-server-license-terms --server-data-dir <app-data>/server-data`.
3. The readiness line `Web UI available at http://localhost:<port>?tkn=…`
   (printed by `remoteExtensionHostAgentServer.ts`) is parsed from stdout.
4. The webview navigates to the workbench URL; user settings, extensions and state
   persist under the app-data `server-data` directory.
5. On exit the whole server process group (extension hosts, ptys, watchers) is
   terminated — SIGTERM first, SIGKILL/`taskkill /T` as fallback.

## Known gaps / roadmap

- **Auto-update**: wire up `tauri-plugin-updater` (Electron's updater is not carried over).
- **Deep links** (`vscode://` protocol) for extension auth flows: add
  `tauri-plugin-deep-link` and forward the URI into the workbench.
- **Local OS file dialogs**: the workbench uses its remote-aware HTML pickers; native
  dialogs could be bridged later via Tauri IPC on a dedicated init script.
- **Linux titlebar**: the workbench draws its own menu/title bar (same as
  `window.titleBarStyle: custom`), which is the default look on the web build.
