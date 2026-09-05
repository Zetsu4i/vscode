# AGENTS.md — Instructions and Restrictions for AI Agents

This repository performs a **shell transplant** of Microsoft's real Visual
Studio Code: the renderer/workbench UI, Monaco editor, CSS, icons and
extension host stay **exactly as upstream ships them**; the Electron main
process is replaced by a **Rust/Tauri v2 shell**; native services are
re-implemented in Rust **one subsystem at a time**. The final release target
is **Windows NSIS**.

A previous attempt hand-built a lookalike React workbench, hallucinated
features, and was discarded. Do not repeat it. Read this file completely
before changing anything.

## Mission

Preserve, while replacing the Electron shell with Tauri v2:

- 100% UI / look-and-feel parity
- complete extension support
- filesystem access
- terminal access
- all existing VS Code workbench features

## Hard constraints

1. **`tauri-rebuild` branch only.** ALL product development, commits and
   CI happen on `tauri-rebuild`. `main` is the pristine `microsoft/vscode`
   mirror the maintainer branches from for their own experiments — never
   push product commits to `main`, never trigger workflows from it.
2. **Do not redesign the UI.** Keep the original workbench, Monaco editor,
   CSS, icons, layout, and renderer code. If you are writing workbench
   UI code (React/HTML/CSS), STOP — you are hallucinating.
3. **Extension support is critical.** Do not remove the extension host
   (web-worker host today, Node sidecar tomorrow) until a fully tested
   replacement exists.
4. **No feature deletion.** Filesystem, search, terminal, tasks, debug,
   settings, keybindings, source control, webviews, marketplace and update
   must remain functional or get wired to the Rust backbone.
5. **Old code remains until replacement is proven.** Upstream/Electron code
   stays in place until the Tauri/Rust replacement passes all tests.
6. **Never delete large chunks at once.** Legacy removal is file-by-file
   (here: patch-by-patch), in separate cleanup commits, after CI is green.
7. **Do not hallucinate APIs.** If unsure, read the pinned source in
   `upstream/` or run a small spike. Never guess interfaces or DI shapes.
8. **Windows NSIS builds only.** Do not add MSI/WiX/Mac/Linux bundling
   unless explicitly approved; keep `tauri.windows.conf.json` and the
   workflow NSIS-only.
9. **Every subsystem must keep parity behavior.** Do not simplify an API
   just to make Rust easier — mirror upstream semantics (see
   `webClientServer.ts`, `terminalEnvironment.ts`, watcher semantics).

## Source of truth

1. **`upstream/` is the pristine clone of `microsoft/vscode`** at the commit
   pinned in `UPSTREAM_SHA` / `UPSTREAM_TAG`. It is never committed to this
   repo and never edited except through `patches/*.patch` (applied by
   `scripts/prepare-client.sh`).
2. **Never invent or redesign UI or features.** If a feature exists upstream,
   wire it to the Rust backbone; do not re-implement it.
3. Reusable upstream code is reused through normal imports. New code lives in
   `bridge/src/vs/workbench/contrib/tauriBridge/**` and compiles *inside* the
   upstream build.

## Development process

### Before replacing any Electron/Node code

1. Identify the original component in the upstream source tree.
2. Document the IPC surface, input/output behavior, and edge cases (the
   bridge RPC method list in `src-tauri/src/server/mod.rs` mirrors this).
3. Add contract tests under `compat/tests/` as the surface grows.
4. Create or update the relevant phase in `ROADMAP.md`.

### During implementation

- Use small commits: `feat(tauri): ...`, `fix(files): ...`,
  `test(terminal): ...`, `chore(cleanup): ...`.
- Keep old code in place while new code is developed.
- Add new Rust/Tauri services under `src-tauri/src/` (server services in
  `src-tauri/src/server/`, exposed through `bridge/rpc/<method>`).
- Keep the VS Code renderer and extension host code as reusable TypeScript —
  bridge classes implement upstream interfaces only.
- Update `ROADMAP.md` in the same commit when status changes.

### When removing legacy code

Only delete a legacy file after its replacement:

- passes unit tests
- passes integration smoke tests
- passes manual comparison on Windows
- has been verified against original VS Code behavior

Deletion happens in a separate cleanup commit after CI is green, through a
documented patch file. Never remove:

- `src/vs/workbench/`
- Monaco editor code
- extension host code
- renderer CSS/UI components

Those are reusable and must remain intact.

## Patch rules

- Patches are generated from real `git -C upstream diff` output against the
  pinned SHA, must re-apply cleanly (`git apply --check`), and are documented
  in `patches/README.md`. Keep the surface minimal and surgical: registration
  seams in composition roots and build entry lists, never editor internals.
- Every patch hunk is commented with `--- VSTauri bridge (added) ---`.

## Technical rules

- Use **Tauri v2** and Rust stable; WebView2 on Windows.
- The backbone serves the workbench over local HTTP and exposes services via
  JSON-RPC (`/bridge/rpc/<method>`) + a WebSocket event bus
  (`/bridge/ws`) — keep channel/method names aligned with the upstream
  service semantics they replace.
- Keep the renderer isolated: it only talks to the backbone through the
  token-authed bridge; no direct process/env access beyond what the served
  configuration provides.
- Terminal must use a real PTY: `portable-pty` / ConPTY.
- Filesystem watchers use Rust `notify` and must preserve VS Code watcher
  semantics (excludes, atomic writes, event batching).
- Extension host stays the upstream web-worker host today; the Node sidecar
  (stdio/JSON-RPC transport over the backbone) is the compatibility path for
  Node extensions — do not delete it once it exists until a proven
  replacement lands.
- Extension webviews run through the upstream webview stack in the webview;
  native child-webview approaches need a design note first.
- No `panic!()` or `unwrap()` in production service paths; return
  `Result` and log.
- Avoid unsafe dependencies unless explicitly justified.
- Every replacement service must be instrumented with logs (`eprintln!`/
  `tracing` at backbone level is fine today).
- **Everything spawns headless.** The old build popped cmd windows on Windows
  because child processes lacked `CREATE_NO_WINDOW`. Any
  `std::process::Command` on Windows MUST set `creation_flags(0x08000000)`.
  Prefer libraries over subprocesses (`portable-pty`, `notify`, `trash`,
  `rfd`, `grep-searcher` — none open windows). A regression here is
  release-blocking.
- Validate Rust with `cargo fmt` locally; full type checking happens in CI.
  If you have no cargo, do not "fix" Rust by guessing — read the pinned crate
  sources or let CI compile.

## Security rules

- The backbone binds `127.0.0.1` only; every bridge call/websocket carries
  the per-session token; paths are validated server-side.
- Never paste a PAT into source files; never write tokens to logs.
- Never commit tokens, credentials, or client build output (`vscode-web/`,
  `src-tauri/resources/client/` are gitignored).
- Sign Windows builds when possible (tracked in ROADMAP Phase 11).

## Status tracking

Update `ROADMAP.md` for every phase:

- ⬜ Not started
- 🟦 In progress
- ✅ Done
- ⛔ Blocked

A phase is not Done until all its acceptance tests pass.
