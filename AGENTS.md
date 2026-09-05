# AGENTS.md

## Mission

Rewrite the VS Code Electron shell into a Tauri v2 application while preserving:

- 100% UI/look-and-feel parity
- complete extension support
- filesystem access
- terminal access
- all existing VS Code workbench features

The final release target is Windows NSIS.

## Hard Constraints

1. **1 branch only.** Never inspect, merge, or copy from any other branch except the one you created and started working on that is copy of the main branch.
2. **Do not redesign the UI.** Keep the original workbench, Monaco editor, CSS, icons, layout, and renderer code, but VS Code's renderer is tightly coupled to Electron-specific globals (process, ipcRenderer, BrowserWindow constraints). Preserving the UI exactly while swapping the shell will require a more substantial compatibility.
3. **Extension support is critical.** Do not remove the Node extension host until a fully tested replacement exists.
4. **No feature deletion.** Filesystem, search, terminal, tasks, debug, settings, keybindings, source control, webviews, marketplace, and update must remain functional.
5. **Old code remains until replacement is proven.** Keep legacy Electron/Node implementation in place until the Tauri/Rust replacement passes all tests.
6. **Never delete large chunks at once.** Delete legacy files file-by-file after tests pass.
7. **Never commit secrets, tokens, PATs, or credentials.**
8. **Do not hallucinate APIs.** If unsure, read the original VS Code source or create a small spike.
9. **Windows NSIS builds only.** Do not add MSI/Wix/Mac/Linux bundling unless explicitly approved.
10. **Every subsystem must keep parity behavior.** Do not simplify an API just to make Rust easier.
11. **Dont use vscode-web/server or host vscode in background its not native and not what we need**

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
- Keep existing VS Code renderer and extension host code as reusable TypeScript.
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
- Monaco editor code
- extension host code
- renderer CSS/UI components

Those are reusable and must remain intact.

## Technical Rules

- Use **Tauri v2** and Rust stable.
- Use WebView2 on Windows.
- Tauri IPC should mirror existing VS Code channel names where practical.
- Keep the renderer isolated. Expose a controlled preload shim that matches Electron APIs used by VS Code.
- Terminal must use a real PTY implementation such as `portable-pty` / ConPTY.
- Filesystem watchers should use Rust `notify` and preserve VS Code watcher semantics.
- Extension host remains a Node.js sidecar until a later optional phase.
- Extension host transport should be JSON-RPC or stdio IPC, replacing Electron IPC.
- Extension webviews must be supported through Tauri child webviews.
- No `panic!()` or `unwrap()` in production service paths.
- Avoid unsafe dependencies unless explicitly justified.
- Every replacement service must be instrumented with logs.

## Security Rules

- Never paste a PAT into source files.
- Never write tokens to logs.
- Sign Windows builds if possible.

## Status Tracking

Update `ROADMAP.md` for every phase:

- ⬜ Not started
- 🟦 In progress
- ✅ Done
- ⛔ Blocked

A phase is not Done until all acceptance tests pass.
