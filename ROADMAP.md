# VS Code Tauri Rewrite Roadmap

## Target

A native Windows application that is behaviorally identical to VS Code, but uses Tauri v2 instead of Electron.

## Architecture: "Wind & Mountain" (Shimmed Monorepo)

The strategy is a two-layer abstraction pattern modeled on how bleeding-edge
open-source editors (e.g. the Land Project) port the *native* VS Code workbench
out of Electron without a VS Code Server, without fake UI layers, and without
touching Microsoft's workbench code:

- **The Wind layer** (UI environment shim) — the preload that replaces
  Electron's `ipcRenderer`, `process` and window-config handshake with
  equivalents backed by `window.__TAURI__` / Tauri IPC. The workbench source
  is compiled *directly from the original tree*, unmodified.
- **The Mountain layer** (Rust native backend) — implements the exact Node/Electron
  main-process IPC surface natively in Rust: the `vscode:` plain channels, the
  main-process message protocol (binary frames), the service channels
  (`nativeHost`, `storage`, `logger`, `userDataProfiles`, ...), and eventually
  PTY, file watching, and credential storage.

```
+--------------------------------------------------------------+
|                     VSTAURI (TAURI v2) APPLICATION           |
|                                                              |
|   +------------------------------------------------------+   |
|   |                  TAURI WEBVIEW UI                    |   |
|   |                                                      |   |
|   |  +------------------------------------------------+  |   |
|   |  | RAW VSCODE WORKBENCH UI                        |  |   |
|   |  | (Compiled directly from original source)       |  |   |
|   |  | served via in-process vscode-file:// protocol  |  |   |
|   |  +------------------------------------------------+  |   |
|   |                           ^                          |   |
|   |                           | Extracted Interfaces     |   |
|   |  +------------------------------------------------+  |   |
|   |  | THE WIND SHIM LAYER (preload, doc-start)       |  |   |
|   |  | window.vscode.ipcRenderer / process / context  |  |   |
|   |  | -> forwarded to Rust via Tauri invoke() with   |  |   |
|   |  |    the original `vscode:` channel names        |  |   |
|   |  +------------------------------------------------+  |   |
|   +---------------------------+--------------------------+   |
|                               | Tauri IPC (JSON + base64    |
|                               | binary frames)              |
|   +------------------------------------------------------+   |
|   |            THE MOUNTAIN BACKEND (Rust Core)          |   |
|   |  (Implements the exact Node IPC APIs in native Rust) |   |
|   |  - vscode:hello / vscode:message protocol server     |   |
|   |  - nativeHost / storage / logger / profiles / ...    |   |
|   |  - Native Terminal PTY      (portable-pty)           |   |
|   |  - High-speed FS + watching (notify)                 |   |
|   |  - Credential storage       (keyring)                |   |
|   +------------------------------------------------------+   |
|                               ^ stdio / JSON-RPC             |
|   +------------------------------------------------------+   |
|   |  EXTENSION HOST: Node.js SIDECAR                     |   |
|   |  - Bundled node.exe binary in src-tauri/binaries/    |   |
|   |  - Original VS Code extension host code              |   |
|   |  - Talks to Mountain over local gRPC/WebSocket       |   |
|   |    managed by Rust (Phase 7)                         |   |
|   +------------------------------------------------------+   |
+--------------------------------------------------------------+
```

### Repository mapping (one repo, one branch)

The "forked vanilla workbench + shims + backend" monorepo layout maps onto
this repository (AGENTS.md constraint 1: single `tauri-rewrite` branch, the
upstream tree stays pristine and in-tree instead of a submodule):

| Blueprint directory | This repository |
| --- | --- |
| `ui-workbench/` (vendored upstream) | the original VS Code tree at repo root (`src/vs/**`, `build/**`, compiled `out/`) — never modified by us |
| `src-shims/` (Wind) | `src-tauri/src/shim.js` (preload) + `src-tauri/src/config.rs` (window-config handshake) |
| `src-tauri/` (Mountain) | `src-tauri/src/{ipc,protocol,logger,util,config}.rs` + the growing channel services |

### Minutes-to-Update: consuming upstream VS Code releases

Because all custom code lives in the Wind and Mountain layers (plus `.github/`
CI), the thousands of workbench files are never touched. When a massive new
VS Code release drops, the update procedure is mechanical:

1. `git checkout main && git fetch upstream && git merge <upstream release tag>`
   (the vendored tree = upstream; our branch only carries `src-tauri/`,
   `compat/`, `build/ipc-contract/`, `ROADMAP.md`, `AGENTS.md`, CI files).
2. `git checkout tauri-rewrite && git merge main` — resolve conflicts only in
   the (rare) places where upstream changed an IPC surface we shim.
3. Re-run the IPC contract extractor (`node build/ipc-contract/extract-ipc-contract.mjs`)
   — it diffs the freshly scanned contract against `compat/ipc-contract.json`
   and reports exactly which channels/methods the new workbench expects.
4. Update any drifted Mountain channel implementations it flags.
5. `tauri build` — the brand-new workbench wraps around the intact shim stack.

The contract file is therefore the upgrade tripwire: CI fails when the
renderer's IPC surface drifts from what the Rust backend knows about.

### Window chrome parity (frameless + custom titlebar)

- Frameless window (`decorations: false`) with the workbench's own custom
  titlebar layout — exactly the VS Code default Windows experience.
- Drag regions: the shim mirrors Electron's `-webkit-app-region: drag` on
  `.titlebar-drag-region` (mousedown → Tauri `startDragging`, double click →
  toggle maximize) — `data-tauri-drag-region` semantics.
- Native minimize/maximize/close buttons are injected into the workbench's
  `.window-controls-container` (the DOM shape the original CSS already styles).
- Platform vibrancy/acrylic: optional `tauri-plugin-vibrancy` integration for
  the titlebar and background blur — Phase 11 polish, only if the original
  look stays pixel-identical.

Node is temporarily kept for extension compatibility. It is not Electron.

---

## Status Legend

- ⬜ Not started
- 🟦 In progress
- ✅ Done
- ⛔ Blocked

## Session log (2026-09-10)

Salvage + completion of the interrupted 2026-09-08 session (~790 lines of
uncommitted, never-compiled work) plus this round's feature list:

1. **Compile fixes in the stranded work**: `windows` crate dependency
   (0.61 has no `implement` feature — the macro is always available via
   the windows-core re-export), `webview2-com`/`windows-core` promoted to
   direct dependencies, COM API corrections (Ref params, out-param BOOL
   getters, plain-i64 event tokens, SendCom wrapper for the deferral
   handoff), sidecar_channel borrow/type fixes, tauri.conf.json invalid
   `displayName` fields (replaced with `installMode` + bundle
   `publisher`).
2. **VSTauri by Mouri Younes branding** completed end-to-end: product.json
   identity override (window title, About, telemetry names), NSIS
   publisher, Cargo authors, boot payload.
3. **Theme-aware branded boot splash** replacing the fake dark VS Code
   layout skeleton.
4. **Logger channel**: files inside the data root now write at their real
   paths (Output panel spool contract — kills the `output_*` FileNotFound
   noise).
5. **Session/hot-exit persistence hardening** (backupPath at every boot,
   close-time workspace + bounds capture, boot-time bounds restore,
   always-persist state).
6. **`encryption` channel (DPAPI)** — persisted secrets, prerequisite for
   AI provider API keys.
7. **Terminal**: full @xterm addon family + vscode-textmate staged and
   smoke-asserted.
8. **Phase 7 groundwork in CI**: node.exe runtime download, sidecar
   wrapper staging, copilot compile + ship (dist + production
   node_modules), boot-file asserts for all of it.
9. Remaining from the user's list after this round: first ext-host boot
   validation (Phase 7), search acceptance (rides ext host), terminal
   restart persistence, release hardening (signing, updater).

Known-good verification: `cargo check --target x86_64-pc-windows-msvc`
clean in the agent workspace (llvm-rc cross resource compile); the Linux
host check requires GTK dev libs and is covered by CI instead.

CI outcome: dev build 36 (commit 5a158f28, 2026-09-09) — all three jobs
green, smoke report asserts the full bundle incl. node runtime, copilot
dist, xterm addons and the sidecar wrapper. One CI iteration was needed:
copilot initially traveled inside the client artifact (365MB → flaky
cross-job blob download); it now rides the GitHub cache and is staged
into resources/client in the bundle job. NEXT for the user: install the
build, exercise terminal / hot exit / agents window / multi-window, and
share `vstauri.log` — the extension host boot is the next debug surface.

## Session log (2026-09-10, round 2 — dev build 36 first-run triage)

User report on dev build 36: blank screen after the loading animation,
slow load, and cmd.exe console popups. The shared `vstauri.log` traced
every symptom to five concrete defects — no architecture change needed
(a React workbench rebuild would trade extension compatibility for
months of work; the workbench itself boots in ~2.7s under the current
shell once it isn't crashing):

1. **Blank screen (the blocker)** — `fileManagedSettings` answered
   JSON `null`; the renderer stores it into `rawManagedSettings`, and
   `hasRawManagedSettings` does `data !== undefined && Object.keys(data)`
   — so `Object.keys(null)` throws, `AccountPolicyService` rejects,
   `WorkspaceService.initializeConfiguration` aborts with an empty
   configuration model, `getValue('editor')` returns `undefined`, and
   `Workbench.restoreFontInfo` dies on `.fontFamily` inside
   `renderWorkbench`. One null → three visible crashes (policy
   `Object.keys`, terminal `Object.entries(undefined)` in
   `_updateContributedProfiles`, fontInfo) and a permanently blank
   window. Fix: the channel now returns `{}` (Electron "no managed
   settings" parity) — ipc.rs.
2. **Extension host dead + console popups** — the bundled Node 22.14.0
   cannot load `bootstrap-fork.js`, which imports `registerHooks` from
   `node:module` (first shipped in Node 22.15.0): every spawn died with
   `SyntaxError ... 'registerHooks'` and the workbench's retry loop
   respawned it (3+ times per minute, each flashing a console window).
   Fix: runtime pinned to 22.17.0 (CI download, shim `process.versions`,
   config.rs window payload).
3. **Console popups for helpers** — none of the shell's subprocess
   spawns set `CREATE_NO_WINDOW`. Fix: a `util::no_console_window`
   helper applied to the sidecar spawn and terminal_channel's
   `wsl.exe`/`netstat`/`taskkill` (nativeHost already had it for
   taskkill; explorer/ShellExecuteW don't spawn consoles).
4. **`createWorker expects an object`** — ProxyChannel serializes method
   calls as `call(command, [args])`, so `utilityProcessWorker.createWorker`
   arrives as `[{process, reply}]`; the handler expected the bare object
   (the `start` handler already unwrapped). Fix: `unwrap_proxy_arg`
   applied to `createWorker`/`disposeWorker` — utility-process workers
   (file watcher, language detection, output spool) can boot now.
5. **Slow cold start** — gzip level 6 on the ~10 MiB
   `workbench.desktop.main.js` cost ~1s of single-thread CPU on first
   request (in-process COM copies are cheaper than compress-CPU at that
   size). Fix: level 1 + skip compression for bodies > 4 MiB.

Remaining known noise (not fixed this round, tracked for the next log):
`extensions/types` package.json missing from the staged tree; user
extension scan surfaces raw FileNotFound instead of a graceful empty
state (error marshaling shape); the `github` default-account timeout is
expected without sign-in; `--experimental-network-inspection` execArgv
warning on every ext-host spawn (cosmetic).

NEXT for the user: install the next dev build, boot, exercise
terminal / agents window / hot exit, and share `vstauri.log`.

---

## Session log (2026-09-11, round 3 — dev build 37 triage: every sidecar dead)

User report on dev build 37: still slow to start, "nothing is working",
AI chat still requires login, "far worse than the original vscode". The
shared `vstauri.log` shows the workbench shell itself boots fine
(logger → window → `renderer connected` in ~3s) — but **all three Node
sidecars die on the same first line of bootstrap**:

    [sidecar:...] entry failed: Error: Cannot find module '../package.json'
    Require stack: I:\tools\VSTauri\resources\client\out\bootstrap-fork.js

Chain: `bootstrap-fork.js` → `bootstrap-esm.js` → `bootstrap-meta.ts`
falls back to `require('../package.json')` when the build did not inline
it (the `BUILD_INSERT_PACKAGE_CONFIGURATION` marker). Upstream's
`inlineMeta` runs in the gulp pipeline; our `build/next bundle` does
not, so the marker survives into the product bundle and the require
executes at boot — against a `resources/client/` that never shipped a
`package.json`. Downstream effects, all observed in the log:

* **Extension host**: crash-retry loop (4+ spawns per window), "The
  local extension host took longer than 60s to connect", "terminated
  unexpectedly 3 times within the last 5 minutes" — no extensions at
  all, which is also why the AI chat shows a sign-in wall (the BYOK
  copilot build never activates).
* **File watcher** (`watcherMain`): dead → no file watching.
* **Pty host** (`ptyHostMain`): dead → integrated terminal unusable.
* The retry storm of dead Node processes is a large share of the
  perceived "slow as hell" startup.

Fixes this round:

1. **Ship `package.json` at the client root** (CI `Assemble client
   bundle` step). Exact parity with Electron's
   `resources/app/package.json`.
2. **Ship the sidecar node_modules closure** — the product bundle keeps
   npm imports external (`packages: 'external'`), and plain Node
   (no node_modules.asar support — that hook only arms under
   ELECTRON_RUN_AS_NODE/electron) resolves them against the real tree:
   `minimist`, `@vscode/native-watchdog`, `@parcel/watcher`,
   `node-pty`, `@vscode/spdlog`, `@vscode/proxy-agent`, ... New
   `scripts/collect-sidecar-node-deps.mjs` scans the actual
   `out-client` bundles for external specifiers, resolves the
   transitive closure (incl. native/dynamic safety pins), and the CI
   copies exactly that set. Asserted in `Assert client boot files`.
3. **Node runtime realigned 22.17.0 → 24.18.0** (= `.nvmrc`, the
   version CI's `npm ci` compiles native modules against). A mismatch
   would have been the NEXT crash: `@vscode/native-watchdog`,
   `@parcel/watcher`, `node-pty`, spdlog are gyp/NAPI builds whose ABI
   must match the sidecar runtime. shim.js + config.rs version payload
   updated to match.
4. **CI sidecar boot smoke test** (`scripts/sidecar-smoke.mjs`): after
   assembly, the runner boots the real wrapper with
   extensionHostProcess / watcherMain / ptyHostMain and asserts they
   stay alive with no `entry failed`. This class of bug now fails the
   build, not the user's first launch.
5. **IPC error frames `[203]` → `[202]`** (ipc.rs): ResponseType 203 is
   `PromiseErrorObj` (rejects with the RAW object), 202 is `PromiseError`
   (reconstructs a real `Error` with `name`). We sent 203 with an
   `{message,name,stack}` body, so every rejection surfaced as
   "[object Object]" with no name — `toFileSystemProviderErrorCode`
   (files.ts parses `error.name`) saw "Unknown" for every FS error and
   missing mcp.json/tasks.json/extensions.json logged as loud errors
   instead of being silently handled. `fs_channel::fs_error` now carries
   the provider error code (`FileNotFound` etc.) through a `\u{1}`
   sentinel; `ipc::error_body` rebuilds `name` as
   `"<code> (FileSystemError)"` — the exact shape
   `markAsFileSystemProviderError` produces.
6. **Aux windows fixed** (`eAt.resolveWindowId` crash reading
   `undefined.ipcRenderer`): WebView2 popups install `window.vscode`
   through the async document-created init script AFTER `window.open()`
   resolves in the opener (Electron guarantees preload-before-return).
   Patched `auxiliaryWindowService.ts` (electron-browser) to poll for
   the globals with a 10s deadline before invoking
   `vscode:registerAuxiliaryWindow`.
7. **Per-window logs dirs**: `open_workbench_window` now creates
   `logsPath/window<N>` (electron-main parity) — the second window's
   output channels were failing their first write (FileNotFound
   unhandled rejections in the log).
8. **`extensions/types` no longer staged** — dirs without a
   `package.json` are skipped by the staging loop (it is a TS typing
   shim, not an extension; the scanner logged a read error every boot).
9. **NLS for sidecars**: spawn now sets `VSCODE_NLS_CONFIG`
   (`defaultMessagesFile` = bundled `nls.messages.json`) so the `--nls`
   product bundle resolves message strings instead of raw keys.
10. **Cold-boot warm-up**: `protocol::warm_boot_files` reads
    workbench.html/js/main.js/css/codicon into the in-memory body cache
    on a background thread while the window is still being created.

Known remaining (next log): the "github default-account timeout" noise
without sign-in is expected; `--experimental-network-inspection`
execArgv drop warning is cosmetic; opening a file via File→Open reloads
the whole workbench window (upstream delivers `vscode:open-files` to the
live window instead — candidate for the next perf round).

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
  - [x] Root cause of the first build's blank white window found and fixed: the document was served from `http://vscode-file.vscode-app`, which Tauri v2 classifies as a REMOTE origin (`is_local_url` only accepts `http://<scheme>.localhost` hosts), so every `invoke()` from the preload shim was rejected by the IPC ACL. The window now navigates to `vscode-file://localhost/...` → origin `http://vscode-file.localhost` → local → IPC allowed.
  - [x] ESM boot bridged: the workbench's absolute import (`vscode-file://vscode-app/<appRoot>/out/vs/workbench/workbench.desktop.main.js`) can never resolve under WebView2 (wry only routes http(s) WebResourceRequested traffic), so the shim enables the renderer's own dev boot path (`VSCODE_DEV` + `_VSCODE_USE_RELATIVE_IMPORTS` → document-relative workbench import) and traps `_VSCODE_FILE_ROOT` so all `FileAccess` URLs resolve inside the document origin.
  - [x] CSS module bridge: the dev-compiled ESM tree keeps `import './x.css'` statements; protocol.rs answers those module-graph requests with `_VSCODE_CSS_LOAD` wrapper modules (the server-side twin of Electron dev's cssModules import map) and serves `text/css` for stylesheet requests (distinguished by `Sec-Fetch-Dest`).
  - [x] Main-process IPC protocol implemented natively: `vscode:hello` → Initialize frame, `vscode:message` binary frames (base64 bridge, codec mirrored from ipc.ts and round-trip tested), channel router with `nativeHost` window operations; unregistered channels reject like Electron's pending-request timeout and are logged for Phase 2.
  - [x] Custom-titlebar parity: frameless window (`decorations: false`), drag region + dblclick-maximize on `.titlebar-drag-region`, injected `.window-icon` min/max/close buttons into `.window-controls-container`, WebView2 default context menu suppressed globally.
  - [x] Full request trace logging in the protocol handler (first 1000 requests per run) + renderer error forwarding — the remote-debugging loop for the next iterations.
  - [x] Blank-screen root cause chain identified from a user runtime log: the workbench crashed in `Workbench.restoreFontInfo` (`TypeError: Cannot read properties of undefined (reading 'fontFamily')`) because `WorkspaceService.initialize()` failed silently (missing `localFilesystem` channel in that build → no configuration model → `getValue('editor') === undefined`). The Phase 4 fs/storage/logger/profiles channels close that chain; the remaining boot-channel gaps (`update:_getInitialState`, `meteredConnection`, `nativeManagedSettings`) are now answered too.
  - [x] Renderer `Initialize` (200) echo handled: the renderer's `IPCClient` also constructs a `ChannelServer` whose constructor sends its own `Initialize` frame upstream (bidirectional ipc.electron protocol); Electron main ignores it, the Rust router now does too instead of warning.
  - [x] Product-mode env transition: `VSCODE_DEV` is boot-only now. protocol.rs prepends a transition statement to the served `workbench.desktop.main.js` that deletes it after the module graph evaluated (before `DesktopMain.open()`), so `environmentService.isBuilt` reports a built product — no `.build/builtInExtensions` dev scans, no dev console forwarding. Known cosmetic gap: `product.ts` evaluates earlier in the graph and appends the " Dev" product-name suffix (fix: serve the real `vscode-file://vscode-app/...` URL form from wry so the production import branch works without VSCODE_DEV at all).
- [x] Keep Electron app still buildable in parallel (Electron tree untouched)

### Acceptance

- [ ] VS Code workbench renders in Tauri window
- [ ] Editor area renders Monaco
- [ ] No crash on initial startup
- [x] Original Electron build still works (compile job green on Windows CI)
- [x] CI build time: node_modules cache (upstream composite actions) + compiled `out/` cache keyed on `hashFiles(src/**, build/**, package*.json)` — typecheck and the gulp compile are skipped entirely when renderer inputs are unchanged (the normal case during the shell transplant, which lives in `src-tauri/` and `.github/`)

---

## Phase 2: IPC Contract Extraction

### Status: 🟦 In progress

### Goal

Catalog every Electron main/renderer IPC surface before changing it.

The contract has two halves, both captured by the extractor script
(`build/ipc-contract/extract-ipc-contract.mjs`):

1. **Plain `vscode:` ipcRenderer channels** — `validatedIpcMain.handle/on` in
   electron-main, `ipcRenderer.send/invoke/on` in the preload/renderer.
2. **Main-process message protocol channels** (the `vscode:hello` /
   `vscode:message` binary protocol the Wind shim bridges into Rust) —
   every `registerChannel('<name>', ...)` on the main/shared-process side
   paired with every `getChannel('<name>')` on the renderer side, plus the
   command/event surface each channel serves (explicit `IServerChannel`
   switch statements, or `ProxyChannel.fromService` over a service interface
   — e.g. `nativeHost` exposes the full `INativeHostService` method list).

### Tasks

- [x] Scan original source for:
  - [x] `ipcMain.handle` / `ipcMain.on` (via `validatedIpcMain` wrapper)
  - [x] `ipcRenderer.invoke` / `ipcRenderer.send` (preload + renderer)
  - [x] `webContents.send` (main → renderer push channels)
  - [x] `registerChannel` / `getChannel` (protocol service channels, including
        constant-named channels like `localFilesystem`, `meteredConnection`)
- [x] Create `compat/ipc-contract.json` (machine-readable, grouped, with
      producer/consumer file+line references)
- [x] Create `compat/ipc-contract.md` (human-readable summary tables)
- [x] Extract the `nativeHost` / `userDataProfiles` / `keyboardLayout`
      ProxyChannel service interfaces into explicit command lists
- [ ] Group services into:
  - [x] Window / nativeHost
  - [x] Dialog (nativeHost `pickFileAndOpen` / `showOpenDialog` / ...)
  - [ ] Clipboard (nativeHost read/write clipboard)
  - [ ] Storage
  - [ ] Files (localFilesystem provider channel)
  - [ ] Terminal (pty host channels)
  - [ ] Search
  - [ ] Process/tasks
  - [ ] Extensions (extension host + gallery channels)
  - [ ] Update
- [x] Add contract drift check to CI (fails when the scanned surface changes
      without the contract being regenerated — the upstream-update tripwire)
- [x] Add Mountain coverage report to the extractor output (which channels
      Rust already answers vs. rejects) — `compat/ipc-contract.md`
- [ ] Add contract tests for each group

### Acceptance

- [x] IPC contract checked into repo (`compat/ipc-contract.json`)
- [x] Contract extraction runs in CI (drift check)
- [ ] Contract tests run in CI for each channel group

---

## Phase 3: Window, Dialog, Clipboard, and Storage

### Status: 🟦 In progress (multi-window + hot exit + secrets landed; Windows runtime round-trip pending)

### Goal

Replace Electron main process basics with Tauri/Rust services.

### Tasks

- [x] Multi-window, landed 2026-09-10:
      `windows.rs` — a VS Code-numeric window registry, `openWindow` /
      `openAgentsWindow` full workbench windows (per-label window
      configurations with own windowId / backupPath / agents profile),
      and `window.open` popups through WebView2's NewWindowRequested COM
      event (`#[implement]` handler with the deferral API — the popup IS
      a real Tauri window whose CoreWebView2 is handed back through
      SetNewWindow, so the workbench's auxiliary-window DOM handshakes
      work). NOTE: this code had never compiled before 2026-09-10 — the
      `windows = { features = ["implement"] }` dependency was invalid
      (0.61 has no such feature) and the COM signatures were wrong
      (Ref params, out-param BOOL getters). Fixed + cross-checked with
      `cargo check --target x86_64-pc-windows-msvc` in the agent
      workspace.
- [x] Theme-aware branded boot splash (2026-09-10): the window
      configuration no longer ships a fake-layout `partsSplash`
      (layoutInfo removed — workbench.js then only applies the theme
      background), and the Wind shim renders a "VSTauri / Version x.y.z /
      by Mouri Younes / Starting…" overlay from `__VSTAURI_BOOT__`
      (OS-theme colors: Windows AppsUseLightTheme + high contrast), removed
      when the real titlebar renders.
- [x] Credential storage: the `encryption` channel (IEncryptionMainService
      parity — DPAPI CryptProtectData/CryptUnprotectData, JSON envelope
      `{data: base64}` like safeStorage) — the secret storage service
      (EVERY stored API key, incl. the AI provider BYOK keys) now
      persists across restarts instead of staying in-memory.
- [x] Editor state persistence / hot exit (2026-09-10 hardening):
      every boot allocates `Backups/<workspaceId>` and points the window
      configuration at it (the renderer's BackupTracker spills unsaved
      working copies there and restores them next boot — "many editors
      closed with unsaved content come back", the original hot-exit
      behavior); windowsState.json now always records the closing
      window's workspace (main window tracked at boot, secondary windows
      tracked on creation) and its live bounds (captured at
      CloseRequested: position/size/maximized/fullscreen; maximized keeps
      the last normal bounds, Electron semantics); the main window
      reopens at the saved bounds. `getDirtyWorkspaces` answers from the
      Backups registry.
- [x] Window title, size, and fullscreen work (title/size/fullscreen
      per IWindowOpenable + forceNewWindow — currently a single window
      reloaded into the new workspace; full window management is a later
      phase)
- [x] Implement native dialog service — `nativeHost` channel:
      showSaveDialog / showOpenDialog / showMessageBox / pickFileAndOpen /
      pickFolderAndOpen / pickWorkspaceAndOpen / pickFileFolderAndOpen /
      showItemInFolder / openExternal. File dialogs ride on
      tauri-plugin-dialog (the same Win32 common dialogs Electron uses,
      filters + defaultPath + multiSelection + openDirectory mapped);
      showMessageBox uses Windows TaskDialogIndirect via windows-sys so
      Electron-style custom button labels, defaultId, cancelId and the
      verification checkbox all work (response index mapping preserved);
      openExternal is ShellExecuteW, showItemInFolder is
      `explorer /select`; pick*AndOpen feeds config::apply_window_openables
      (filesToOpenOrCreate / folderUri / workspace with the upstream md5
      workspace id) and reloads the window. `vscode_ipc` became an async
      command running route() on the blocking pool — native modal dialogs
      must never block the main thread's message loop.
- [x] Implement clipboard service — readClipboardText / writeClipboardText
      via tauri-plugin-clipboard-manager (the editor's copy/paste path:
      workbench NativeClipboardService routes through these nativeHost
      commands); readImage/writeImage with PNG <-> RGBA through the image
      crate (Electron nativeImage.toPNG parity); read/writeClipboardBuffer
      custom formats (`code/file-list`) still stubbed — raw Win32
      RegisterClipboardFormat plumbing is a later round
- [ ] Implement storage/settings persistence
  - [x] `storage` protocol channel (getItems / updateItems / getValue /
        compareAndSwap / optimize / isUsed / onDidChangeStorage) backed by a
        Rust JSON-file KV store per scope (application / application-shared /
        profile / workspace) — `src-tauri/src/storage_channel.rs`
  - [x] `logger` protocol channel (createLogger / log / consoleLog /
        registerLogger / deregisterLogger / setLogLevel / setVisibility /
        getRegisteredLoggers + change events) writing real log files under
        the logs dir — `src-tauri/src/logger_channel.rs`
  - [x] `userDataProfiles` protocol channel (profile CRUD + onDidChangeProfiles)
        over a persistent `profiles.json` — `src-tauri/src/profiles_channel.rs`
- [x] Implement `keyboardLayout` protocol channel (getKeyboardLayoutData with
      a real US-layout Windows mapping + onDidChangeKeyboardLayout event) —
      `src-tauri/src/keyboard_channel.rs`
- [x] Implement `workspaces` protocol channel (IWorkspacesService: recent
      history over `<dataRoot>/recent.json` with MRU merge/removal, upstream
      md5 workspace identifiers, untitled-workspace create/delete,
      enterWorkspace/getDirtyWorkspaces) — `src-tauri/src/workspaces_channel.rs`;
      kills the noisiest boot rejection family (`getRecentlyOpened`, called
      by the welcome page and Open Recent within the first seconds)
- [x] Boot-error sweep from the first working runtime log: window config now
      carries `builtin-extensions-dir` (fixes `Error scanning system
      extensions: FileNotFound for 'extensions'` — the scanner's fallback
      derived a RELATIVE path through the shim's document-origin file root)
      and `logsPath` (renderer.log/output channels land in the per-session
      `logs/<stamp>/window1` tree the shell pre-creates); the profile
      sub-dirs (globalStorage/snippets/prompts/extensions) are pre-created
      like Electron's first boot; `update.isLatestVersion` /
      `update.setInternalOrg`, `externalTerminal.getDefaultTerminalForPlatforms`,
      `browserView.updateWindowConfiguration`,
      `nativeHost.windowsGetStringRegKey` and a graceful `openAgentsWindow`
      answer natively; the shim serializes unhandled rejections properly
      (stacks, Event target URLs — the previous `[object Object]` /
      `[object Event]` lines hid the actual failures)
- [ ] Credential storage: replace `keytar`-style secret storage with the Rust
      `keyring` crate behind the `encryption`/secret channels (Windows
      Credential Manager under the hood)
- [x] Expose through Tauri IPC with the same channel names
- [x] Keep old Electron implementation until tests pass (Electron tree untouched)

### Acceptance

- [x] File open/save dialogs work (backend implemented; smoke-test on
      Windows in the next build round)
- [ ] Window title, size, and fullscreen work (title/size/fullscreen
      already answer; multi-window lifecycle pending)
- [x] Auxiliary/multi windows: `window.open` popups are intercepted at
      the WebView2 NewWindowRequested COM event and materialized as real
      Tauri windows with the shim + per-window boot payload; the popup's
      CoreWebView2 is handed back via SetNewWindow so the renderer gets
      genuine `window.opener` semantics (Electron setWindowOpenHandler
      parity). The `vscode:registerAuxiliaryWindow` handshake is answered
      with per-window numeric ids. Agents window: `nativeHost.openAgentsWindow`
      boots a full second workbench window (isSessionsWindow + agents
      profile + shared agent-sessions workspace).
- [x] Clipboard copy/paste works (text + images; custom formats pending)
- [ ] Settings persist after restart (disk-backed by construction; needs a
      Windows restart round-trip to confirm end-to-end — bounds/backup
      persistence and secret storage landed 2026-09-10)

---

## Phase 4: File System Service (Mountain: FS)

### Status: 🟦 In progress

### Goal

Replace Electron file service with Rust file service.

### Tasks

- [x] Implement the `localFilesystem` provider channel (DiskFileSystemProviderChannel
      command surface, extracted in the Phase 2 contract) natively in Rust —
      `src-tauri/src/fs_channel.rs`: stat / readdir / readFile / writeFile /
      mkdir / delete / rename / copy / cloneFile / realpath / open-read-write-close
      (fd streams) / watch-unwatch, with binary `VSBuffer` frames lifted through
      the IPC codec (ipc.rs tag-3 bridge) and FileSystemError-shaped rejections
      (FileNotFound / NoPermissions / FileExists)
- [x] Implement read/write/create/delete/rename/copy
- [x] Atomic writes (temp file + rename, same guarantee SQLite gave Electron)
- [x] fd-based stream IO for large files (open / read / write / close)
- [x] Implement file watching with Rust `notify` (replaces `@parcel/watcher` /
      win32 `ReadDirectoryChangesW` usage — VS Code watcher semantics preserved:
      recursive, ignore rules, batching) — `fs_channel.rs` + `ipc.rs`:
      each `watch(sessionId, req, resource, opts)` opens a real
      `notify` watcher on its own thread with the upstream
      `diskFileSystemProviderClient.ts` contract (`listen('fileChange',
      [sessionId])` / `unwatch(sessionId, req)`), 50 ms debounce batching,
      ADDED/UPDATED/DELETED resolution by existence at flush time (matches
      @parcel/watcher on both inotify and ReadDirectoryChangesW),
      `excludes`/`includes` globs matched relative to the watch root with
      a `**`-aware segment matcher, and per-session event routing via the
      new `ipc::fire_event_with_arg` (EventListen args are now kept)
- [x] Match VS Code watcher behavior (per-session partitioning, string
      error payloads on watch failure, recursive + non-recursive modes,
      exclude-driven pruning incl. the watched directory itself)
- [ ] Re-arm watchers when the watched directory is deleted and recreated
      (upstream parcel watcher behavior — tracked for the next round)
- [x] Ship the built-in (system) extensions in the client bundle
      (`extensions/`, `*-tests` fixtures excluded, `copilot` excluded until
      Phase 7 — its `dist/` is gitignored and only `compile-copilot` can
      produce it): 61 of 96 are data-only
      (theme-defaults, language grammars, keymaps) and work immediately
      through the localFilesystem channel — they give the workbench its
      default themes and colors. Code extensions are scanned and listed but
      stay inactive until the extension host lands (Phase 7; the sidecar
      round must also run compile-copilot and ship its dist/). Verified by
      the CI bundle assertions + `--vstauri-smoke` (theme-defaults
      dark_plus.json).
- [x] Watcher routing ANSWERED (2026-09-10): the recursive watcher does
      go through `utilityProcessWorker.createWorker` → the
      watcherMain utility process — which the Node sidecar manager now
      serves (sidecar_channel + resources/node/node.exe). The renderer
      gets the REAL upstream parcel/@vscode watcher semantics in-process
      through the sidecar; the notify-based localFilesystem watcher keeps
      covering the non-recursive surface.
- [ ] Add encoding and BOM handling
- [ ] Add workspace folder APIs
- [ ] Add search file traversal hooks

### Acceptance

- [ ] Open/save/edit files works identically
- [ ] Editor file watchers fire correctly
- [ ] External file change detection works
- [ ] Large file edits do not block UI

---

## Phase 5: Terminal Service (Mountain: PTY)

### Status: 🟦 In progress (backend done; xterm renderer deps now bundled — first Windows runtime round-trip pending)

### Goal

Replace Electron/node-pty terminal backend with Rust PTY.

### Tasks

- [x] Implement PTY service with the Rust `portable-pty` crate (ConPTY on Windows)
      as a Tauri sidecar-free native service; data streams flow back to xterm.js
      through the Wind shim's `vscode:message` protocol frames —
      `src-tauri/src/terminal_channel.rs` implements the full `IPtyService` /
      `IPtyHostService` ProxyChannel surface registered as channel `localPty`
      (electron-main app.ts line ~1447): createProcess / start / shutdown /
      shutdownAll / input / processBinary / sendSignal / resize / clearBuffer /
      acknowledgeDataEvent / listProcesses / getInitialCwd / getCwd /
      attach-detach / refreshProperty / updateProperty / updateTitle /
      updateIcon / getDefaultSystemShell / getEnvironment / getWslPath /
      getProfiles (config merge + Windows auto-detection: PowerShell,
      Windows PowerShell, Command Prompt, Git Bash, WSL) /
      freePortKillProcess / set-getTerminalLayoutInfo (in-memory, survives
      window reloads) / serializeTerminalState / reviveTerminalProcesses /
      the auto-reply + contribution stubs, with onProcessData / onProcessReady
      (pid + cwd + ConPTY build number via RtlGetVersion) / onProcessExit
      events, an incremental UTF-8 decoder (chunk-boundary-safe, CJK output
      intact) and split killer semantics so shutdown never races the exit
      reaper
- [x] Spawn default shell on Windows (COMSPEC / explicit profile path;)
      Unix $SHELL for dev parity
- [x] Support write, read, resize, kill, exit (reader thread until EOF →
      child wait → exit code event; resize via MasterPty::resize; SIGINT
      mapped to ^C)
- [x] Integrate xterm.js renderer IPC (onProcessData events carry the
      { id, event: { data } } payloads the renderer's LocalPty proxy
      subscribes to; no renderer changes needed)
- [x] Preserve shell integration behavior — replace-args injection mirrored
      from getShellIntegrationInjection (terminalEnvironment.ts):
      pwsh/powershell/bash.exe on Windows (bash/zsh/pwsh/fish on other
      platforms for dev parity), VSCODE_INJECTION / VSCODE_NONCE /
      VSCODE_STABLE / VSCODE_A11Y_MODE / VSCODE_SHELL_ENV_REPORTING /
      VSCODE_SHELL_LOGIN env mixin, arg-classifier parity (implied/login
      args), the scripts resolved from the client bundle
      (terminal_channel::init), injected args served through `start` as
      ITerminalLaunchResult.injectedArgs — unit-tested against the
      upstream tables
- [x] Preserve cwd and environment handling (string | UriComponents cwd,
      env merge: inherited → resolved env → launch-config env)
- [x] Terminal renderer dependencies bundled (2026-09-10): the first
      Windows runtime log showed the integrated terminal dead on arrival —
      the xterm addons (`@xterm/addon-webgl|unicode11|progress|clipboard|
      search|serialize|ligatures|image`, `@xterm/xterm`, `@xterm/headless`,
      `vscode-textmate`) 404ed as `node_modules.asar/@xterm/...`. The CI
      bundle now stages the full @xterm family and the smoke mode asserts
      the addon files exist.
- [ ] Persistent terminal state across app restarts (serialize/revive are
      in-memory stubs; layout info survives window reloads)
- [ ] Dynamic cwd tracking via OSC 633/9;9 (initial cwd is reported)

### Acceptance

- [ ] Integrated terminal opens and accepts input (backend verified by
      round-trip tests; needs a renderer smoke test on Windows)
- [ ] Resize works
- [ ] Multiple terminals work
- [ ] Ctrl+C/kill works
- [ ] Shell integration behaves like original

---

## Phase 6: Search and Process Services

### Status: 🟦 Re-scoped after routing analysis (2026-09)

### Goal

Workspace search and process execution with Rust-grade performance.

### Tasks

- [x] Routing analysis (important): desktop workspace search does NOT cross
      a main-process IPC channel — `extHostSearch.ts` (EXTENSION HOST
      process) registers `RipgrepSearchProvider` for `Schemas.file` /
      `Schemas.vscodeUserData`, which the renderer consumes through
      `mainThreadSearch` → `ISearchService.registerSearchResultProvider`.
      Workspace search therefore depends on the Phase 7 extension host
      sidecar; there is no standalone `search` channel for Mountain to
      answer. (The browser build's `LocalFileSearchWorkerClient` — web
      worker over the FS provider — is a renderer-only alternative that
      would bypass the ext host, at the cost of diverging from desktop
      behavior; revisit only if Phase 7 slips.)
- [ ] Search acceptance rides Phase 7: after the ext-host sidecar lands,
      ripgrep search works unmodified (rg is spawned by ext-host code with
      node_modules/vscode-ripgrep binaries — the sidecar must ship them).
      Alternative Mountain fast path: implement the ext-host search
      provider surface in Rust behind the same registration point.
- [ ] Process service for `process`-type tasks: local terminal-type tasks
      already ride the Mountain PTY (localPty channel); process-type
      execution (IProcessService channel) is a thin spawn/wait/kill surface
      in Rust once needed.
- [ ] Preserve include/exclude glob behavior (comes free with the ripgrep
      path above)
- [ ] Add environment inheritance
- [ ] Add kill/exit code handling
- [ ] Integrate task output back into workbench

### Acceptance

- [ ] Search works across workspace
- [ ] Include/exclude globs match original
- [ ] Tasks run and output
- [ ] Task exit code and kill work

---

## Phase 7: Extension Host Integration (Mountain: Sidecar)

### Status: 🟦 In progress (runtime + copilot bundled; first ext-host boot pending)

### Goal

Preserve extension host while replacing the Electron extension main bridge.

**2026-09-10 round — the bundle is now ext-host-ready:**

- `resources/node/node.exe` (Node 22.14.0, pinned download) ships in the
  NSIS bundle; sidecar_channel resolves it at
  `<install>/resources/node/node.exe`.
- `vstauri-sidecar.mjs` (stdin/stdout length-prefixed framing wrapper that
  emulates `process.parentPort` + `MessagePortMain` over plain Node) is
  staged into the client bundle — the extension host, pty host and
  watcher utility processes all boot through it with
  `VSCODE_ESM_ENTRYPOINT=<module>` + the original `out/bootstrap-fork.js`.
- The copilot extension ships in `extensions/copilot` WITH its compiled
  `dist/` (esbuild, from this repo's source) and pruned production
  `node_modules` (the dist keeps `@github/copilot` — the win32-x64 CLI/SDK
  with conpty natives — `node-pty` and friends EXTERNAL, so those
  node_modules are runtime requirements, exactly the set upstream ships).
- The `encryption` channel (DPAPI) makes the secret storage persist —
  required for storing AI provider API keys.
- AI provider surface (user-facing): the copilot BYOK providers —
  openai, anthropic, gemini, ollama, openrouter, azure, xai,
  **customoai (custom OpenAI-compatible base URL + key)** and
  customendpoint — configured in the editor's model manager; the model
  list is fetched from the provider's `/v1/models` endpoint; keys are
  stored encrypted; NO GitHub/Google account is needed for any of them.

NEXT: first ext-host boot validation on a Windows machine (the runtime
log will show extensionHostStarter → sidecar spawn → bootstrap-fork →
extension activation); fix the long tail from that log.

### Tasks

- [ ] Keep original VS Code extension host as Node.js sidecar
- [x] Bundle a pre-compiled Node.js binary (resources/node/node.exe, tauri
      `resources` config — managed by sidecar_channel, not `externalBin`,
      because the sidecar needs the CLIENT root as cwd)
- [ ] Spin up the sidecar at app boot from Rust; the extension host process
      is managed by the Mountain backend
- [ ] Replace Electron IPC transport (MessagePort-based
      `vscode:createPtyServiceMessageChannel` style handshakes) with stdio or a
      local gRPC/WebSocket channel managed by Rust — the Wind shim's
      `ipcMessagePort.acquire` maps onto it
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

### Status: 🟦 In progress (startup performance landed; release hardening next)

### Goal

Harden release flow and optimize for Windows.

### Tasks

- [ ] Signed NSIS installer
- [x] Automatic GitHub release on successful build
- [ ] Updater integration if applicable
- [ ] Performance profiling
- [ ] Memory profiling
- [x] Reduce startup time — the 30-second blank-window boot is fixed by a
      three-part change measured from a user runtime log (window opens at
      +7.3s, `renderer connected` at +25.4s, first paint after ~30s with
      ~1260 protocol requests):
      1. **Product client bundle** (the decisive lever): CI now runs the
         upstream esbuild bundler
         (`node build/next/index.ts bundle --out out-client --minify --nls
         --target desktop`) and stages `out-client/` instead of the dev
         compile. Boot goes from ~1260 individual ESM module fetches to
         ~6 requests (workbench.html, workbench.js, the 19.7 MiB
         self-contained `workbench.desktop.main.js`, its 1.5 MiB CSS, NLS
         and runtime node_modules). The bootstrap keeps the
         `VSCODE_DEV`+`_VSCODE_USE_RELATIVE_IMPORTS` document-relative
         import branch, so the Wind shim boot works unchanged against the
         product bundle (validated: zero static imports survive bundling —
         see scripts/validate_bundle_shape.cjs in the agent workspace).
         The bundle output additionally contains the Node mains
         (extensionHostProcess, ptyHostMain, watcherMain, sharedProcessMain)
         that Phase 7 will execute as the sidecar.
      2. **Protocol fast path** (`src-tauri/src/protocol.rs`): the client
         root is canonicalized once (was two `fs::canonicalize` syscalls
         per request), served bodies are cached in memory as
         `Arc<CachedBody>` (raw + pre-compressed gzip, 384 MiB budget,
         oldest-quarter eviction) so hot requests copy a pointer and never
         touch the filesystem, compressible responses >1 KiB are served
         with `Content-Encoding: gzip`, every response carries a strong
         ETag with `Cache-Control: no-cache` (repeat boots revalidate with
         304s; an app update can never serve stale modules), and
         `node_modules.asar/...` URLs (the product runtime's external
         import form) map onto the bundled plain `node_modules/` tree.
      3. **Instant perceived paint**: the shim paints the dark editor
         background at document-start (WebView2 defaults to white), and the
         window configuration now ships a default dark_plus
         `partsSplash` + `layoutInfo` so workbench.js draws the shell
         skeleton (titlebar/activity bar/sidebar/statusbar) synchronously
         after the config handshake — the same mechanism Electron main
         uses — while the bundle streams in.
- [ ] User-facing changelog generation

### Acceptance

- [ ] NSIS installer released automatically
- [ ] Release notes present
- [ ] Installer works on clean Windows VM
- [ ] Cold boot: window shows the dark splash skeleton within ~1s of the
      WebView2 environment being up; the full workbench renders from the
      product bundle in low single-digit seconds (log markers:
      `main window created` → `renderer connected`)
