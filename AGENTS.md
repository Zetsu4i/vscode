# AGENTS.md

> This file supersedes the upstream agent notes for work on the `tauri-shell`
> branch. For upstream VS Code architecture background see
> [.github/copilot-instructions.md](.github/copilot-instructions.md).

## Mission

Rewrite the VS Code Electron shell into a Tauri v2 application while preserving:

- 100% UI/look-and-feel parity
- complete extension support
- filesystem access
- terminal access
- full IntelliSense / language features
- all existing VS Code workbench features

The final release target is Windows NSIS.

## Hard Constraints

1. **1 branch only.** All work happens on `tauri-shell`, branched from `main`.
   Never inspect, merge, or copy from any other branch of this repository.
2. **Do not redesign the UI.** Keep the original workbench, Monaco editor, CSS,
   icons, layout, and renderer code. VS Code's renderer is tightly coupled to
   Electron-specific globals (`process`, `ipcRenderer`, sandbox/preload
   contracts, `BrowserWindow` behavior), so preserving the UI exactly while
   swapping the shell requires a substantial **compatibility layer**, not UI
   edits. All divergence is absorbed by the shim, never by the workbench.
3. **Extension support is critical.** Do not remove the Node extension host
   until a fully tested replacement exists.
4. **No feature deletion.** Filesystem, search, terminal, tasks, debug,
   settings, keybindings, source control, webviews, marketplace, and update
   must remain functional.
5. **Old code remains until replacement is proven.** Keep the legacy
   Electron/Node implementation in place until the Tauri/Rust replacement
   passes all tests.
6. **Never delete large chunks at once.** Delete legacy files file-by-file
   after tests pass.
7. **Never commit secrets, tokens, PATs, or credentials.**
8. **Do not hallucinate APIs.** If unsure, read the original VS Code source or
   create a small spike under `spikes/`.
9. **Windows NSIS builds only.** Do not add MSI/Wix/Mac/Linux bundling unless
   explicitly approved.
10. **Every subsystem must keep parity behavior.** Do not simplify an API just
    to make Rust easier.
11. **No `vscode-web`, no `code serve-web`, no background HTTP server.** The
    product must be native: the workbench is the **desktop** build
    (`out-vscode` / `vs/code/electron-browser/workbench`) loaded into WebView2
    from disk through a Tauri **custom URI protocol** handler. Networking is
    never used to serve the UI.

## Development Process

### Before Replacing Any Electron/Node Code

1. Identify the original component in the VS Code source tree.
2. Document the IPC surface, input/output behavior, and edge cases.
3. Add contract tests under `compat/tests/`.
4. Create or update the relevant phase in `ROADMAP.md`.

### During Implementation

- Use small commits:
  - `feat(tauri): ...`
  - `fix(files): ...`
  - `test(terminal): ...`
  - `chore(cleanup): ...`
- Keep old code in place while new code is developed.
- Add new Rust/Tauri services under `src-tauri/src/`.
- Keep the existing VS Code renderer and extension host code as reusable
  TypeScript.
- Update `ROADMAP.md` in the same commit when status changes.

### When Removing Legacy Code

Only delete a legacy file after its replacement:

- passes unit tests
- passes integration smoke tests
- passes manual comparison on Windows
- has been verified against original VS Code behavior

Deletion must happen in a separate cleanup commit after CI is green.

Never remove:

- `src/vs/workbench/`
- Monaco editor code (`src/vs/editor/`)
- extension host code (`src/vs/workbench/api/`, `src/bootstrap-fork.ts`)
- renderer CSS/UI components
- `resources/` icons and product assets

Those are reusable and must remain intact.

## Technical Rules

- Use **Tauri v2** and Rust stable.
- Use WebView2 on Windows.
- The workbench is loaded from a **custom protocol** (`vscode-file://` style)
  backed by the Rust asset resolver — mirroring Electron's `vscode-file`
  protocol so `asBrowserUri` paths keep working unchanged.
- Tauri IPC mirrors existing VS Code channel names where practical.
- Keep the renderer isolated. Expose a controlled preload shim
  (`src-tauri/preload/`) that matches the Electron APIs VS Code actually uses:
  `vscode/ipcRenderer`, `vscode/ipcMessagePort`, `vscode/webFrame`,
  `vscode/process`, `vscode/context`, `Buffer`, `setImmediate`, `global`.
- Terminal must use a real PTY implementation (`portable-pty` / ConPTY).
- Filesystem watchers use Rust `notify` and preserve VS Code watcher semantics
  (recursive/non-recursive, exclude globs, event coalescing).
- Extension host remains a Node.js sidecar until a later optional phase.
- Extension host transport is a message-port-equivalent over stdio/named pipe
  JSON-RPC, replacing Electron IPC.
- Extension webviews are supported through Tauri child webviews.
- No `panic!()` or `unwrap()` in production service paths.
- Avoid unsafe dependencies unless explicitly justified.
- Every replacement service must be instrumented with logs.

## Security Rules

- Never paste a PAT into source files, commit messages, or workflow files.
  Use repository secrets (`GITHUB_TOKEN`) only.
- Never write tokens to logs.
- Sign Windows builds if signing material is available.

## Status Tracking

Update `ROADMAP.md` for every phase:

- ⬜ Not started
- 🟦 In progress
- ✅ Done
- ⛔ Blocked

A phase is not Done until all acceptance tests pass.
