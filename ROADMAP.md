# ROADMAP — VS Code → Tauri Migration

> **Status legend**
> - [ ] Not started
> - [~] In progress
> - [x] Done / verified

> **How to update this file**
> - Update the status checkboxes before committing a phase.
> - Add a short line in the *Phase log* for every subsystem that is moved.
> - Never mark a subsystem `[x]` unless the parity gate in
>   [AGENTS.md](./AGENTS.md#5-parity-gate-definition) is satisfied.
> - Never delete old Electron/Node code without a separate commit that points
>   to the parity pass.

---

## Phase 0 — Foundation: process, docs, CI harness

- [x] Define migration rules in `AGENTS.md`.
- [x] Publish the phase plan in `ROADMAP.md`.
- [x] Add `Tauri release build` workflow that runs on pushes to this branch and
      creates GitHub Releases after successful Windows NSIS builds.
- [x] Add `.gitignore` entries for Rust/Tauri build artifacts.
- [ ] Add a reference clone of upstream `microsoft/vscode` to a local
      non-migration folder for behavior diffing (`~/vscode-ref`).
- [ ] Name the Rust crate/directory (`src-tauri/`) and decide where generated
      WebView assets live.
- [ ] Decide release/versioning scheme (Tauri version vs upstream Code version).

**Phase 0 log**
- Foundation commit adds migration rules, roadmap, release workflow, and
  artifact ignores. No Rust engine exists yet; the workflow is a guard that
  runs and skips until `src-tauri/tauri.conf.json` exists.

---

## Phase 1 — Native shell

Goal: replace the Electron main-process shell with a Tauri shell that opens the
existing VS Code workbench web UI without changing that UI.

- [~] Scaffold `src-tauri/` (Cargo project + `tauri.conf.json`). Design notes
      are in `src-tauri/README.md` and the Electron→Tauri mapping inventory is
      in `src-tauri/phase1-subsystems.md`. The real scaffold is not committed
      yet so the release workflow does not publish a placeholder app.
- [ ] Implement the window lifecycle, title bar, menus, dock behavior, and
      single-instance behavior to match Electron settings.
- [ ] Serve/load the existing VS Code workbench assets (Monaco + workbench)
      from the Tauri shell using a custom protocol or bundled asset path.
- [ ] Replace `WebContents` and `BrowserWindow` APIs with Tauri equivalents.
- [ ] Maintain window/session state (workspaces, restore windows, screens).
- [ ] Add Windows NSIS bundling config.
- [ ] Wire the release workflow to the real build.

**Parity gate**
The app opens the same workbench with the same layout, themes, and first-run
experience; window behavior and restore behavior match the reference.

**Phase 1 log**
- Added `src-tauri/README.md` with the shell architecture direction and a clear
  gate: do not add `tauri.conf.json` until the shell can load the real VS Code
  workbench assets.
- Added `src-tauri/phase1-subsystems.md` as an inventory of the Electron
  backbone concepts that need Tauri replacements (window/lifecycle, IPC,
  filesystem, terminal/process, WebView loading, native features). It maps the
  current VS Code bootstrap code paths to Tauri concepts without changing any
  UI.
- No Rust scaffold is committed yet. This keeps the release workflow from
  publishing a placeholder app.

---

## Phase 2 — IPC and host bridge

Goal: replace Electron `ipcMain`/`ipcRenderer` with Tauri commands and events
while keeping the VS Code workbench communication layer intact.

- [ ] Inventory every Electron IPC channel consumed outside the workbench.
- [ ] Implement a Tauri IPC bridge that exposes the same logical services.
- [ ] Migrate dialog, clipboard, file-open, external-URL, upload/window-open,
      and native-menu channels.
- [ ] Implement persistent event handling for window events and workspace
      changes.
- [ ] Keep the same serialization behavior (no loss of types/structured data).

**Parity gate**
Commands triggered from the UI reach the host with identical payloads and
return identical results; dialogs and native menus behave the same.

---

## Phase 3 — Extension host

Goal: preserve the **VS Code extension model** and run extensions against the
same extension point API.

- [ ] Keep the existing extension API protocol and typings as the contract.
- [ ] Define how extension processes run under the Tauri host (embedded vs
      subprocess).
- [ ] Reimplement host lifecycle, activation, deactivation, and
      `vscode.workspace`/`vscode.window`/`vscode.env` accessors.
- [ ] Port language services and debug adapters that need filesystem/process
      access.
- [ ] Ensure marketplace/VSIX installation, uninstallation, and disabled-state
      behavior remain identical.

**Parity gate**
A representative set of existing VS Code extensions (including ones that read,
watch, and write files) behaves identically in the Tauri build and the
reference build.

---

## Phase 4 — Filesystem, paths, and workspace

Goal: native Rust filesystem layer behind the workbench.

- [ ] Implement file read/write/stat/watch/rename/delete with correct
      permissions, symlinks, casing, and error codes.
- [ ] Port the workspace folder/root resolution and multi-root workspace logic.
- [ ] Implement filesystem watching (inotify/ReadDirectoryChangesW/FSEvents
      equivalents) with stable events.
- [ ] Port search and file-glob behavior.
- [ ] Keep path handling consistent on Windows/macOS/Linux.

**Parity gate**
Delete, copy, move, rename, watch, conflict, permission-denied, and symlink
tests pass against the same fixture tree used by the reference build.

---

## Phase 5 — Integrated terminal and processes

Goal: Rust-backed shell/PTY layer for the integrated terminal and task runner.

- [ ] Implement pty/process spawning in Rust and stream to the existing
      terminal widget.
- [ ] Support environment variables, working directories, shells, and PTY
      resize.
- [ ] Port process tree, signal, exit-code, and kill behavior.
- [ ] Port tasks, launch/terminate, and debug process setup.
- [ ] Match Windows ConPTY vs *nix PTY behavior.

**Parity gate**
New terminal runs, resizes, handles Ctrl-C/Ctrl-D, kills children, and preserves
environment exactly like the reference.

---

## Phase 6 — Settings, broad features, and resilience

Goal: make the rest of the product surface work without Electron.

- [ ] Settings: JSON/CJSON read/write, defaults, overrides, workspace scopes.
- [ ] Keybindings and command registry parity.
- [ ] Search, find/replace, global search, and workspace symbol search.
- [ ] Git/Debug/SCM integrations that rely on spawning processes.
- [ ] Update mechanism, telemetry/consent parity, crash/native error reporting.
- [ ] Remote and tunnels: ensure the Tauri build can reach existing remote
      extension hosts, or document a deliberate decision.

**Parity gate**
Daily drive (open project, edit, terminal, extensions, search, git, debug,
settings, restart) succeeds without Electron.

---

## Phase 7 — Packaging, release automation, parity, and retirement

Goal: produce installers and remove the retired Electron/Node code.

- [ ] Windows: **NSIS builds only** (no MSI/portable as the default release).
- [ ] macOS/Linux packaging (optional but should match the same product).
- [ ] GitHub Actions creates a release after each successful build with correct
      artifacts named by version.
- [ ] Full smoke test suite on the Tauri build.
- [ ] Remove each obsolete Electron/Node subsystem in its own commit once its
      parity gate is green.
- [ ] Final cleanup of the repository and docs.

**Parity gate**
The installed Tauri app is functionally indistinguishable from the reference
desktop app for the agreed feature matrix, and CI only requires the Tauri
toolchain.

---

## Milestones

| Milestone | What must be true | Phase |
|---|---|---|
| M1 — Shell runs | Tauri shell opens the workbench UI | 1 |
| M2 — Host talks | Commands/dialogs/folders work through Rust host | 2 |
| M3 — Extensions work | Extension host runs extensions against same API | 3 |
| M4 — FS + Terminal | Native filesystem and PTY paths work | 4–5 |
| M5 — Product parity | Daily drive, settings, debug, search, git all work | 6 |
| M6 — Released | NSIS installers + auto GitHub releases | 7 |
| M7 — Electron retired | No Electron/Node required for the shipped app | 7 |

---

## Phase Log

- **2026-09-04 — Phase 0**
  - Added `AGENTS.md` with agent migration rules and parity gates.
  - Added `ROADMAP.md`.
  - Added `.github/workflows/tauri-release.yml` with automatic GitHub releases
    after successful Windows NSIS builds, and `.gitignore` entries for
    Rust/Tauri artifacts.
  - **No Rust engine has been built yet.** This must not be mistaken for
    completion of Phase 1+.
