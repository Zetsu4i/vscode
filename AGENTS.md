# AGENTS.md — VS Code → Tauri Migration Rules

This file governs every AI agent (and human contributor) working on this fork.
Read it before changing anything. If a change violates these rules, it must be
reworked before it is committed.

## 1. Project Goal

Turn this fork of **Code - OSS** into a desktop application that:

- Runs on **Tauri (Rust + WebView)** instead of **Electron + Node.js**.
- Keeps the **original VS Code UI, workbench, theming, commands, keybindings, and
  general look & feel 100% intact**.
- Keeps the **extension model** working with the same API surface and behavior.
- Keeps **filesystem access, file watchers, integrated terminal, settings,
  search, debug, git, language servers, and all other VS Code features**.
- Does **not** get recreated or redesigned from scratch. It is a migration: same
  product, new native backbone.

It is expected to take many phases and many commits. It is **not** a one-shot
rewrite.

## 2. Absolute Rules

1. **Keep the old code until the replacement is verified.**
   Every existing Electron/Node subsystem is treated as the source of truth and
   as a reference implementation. Never delete or rename it in the same commit
   that adds the Rust/Tauri replacement.

2. **Delete code only after a feature is copied, reimplemented, and passes a
   parity gate.**
   The migration sequence is always:

   ```
   add Rust/Tauri implementation (old code still present)
   -> run parity checks (behavioral diff vs original)
   -> mark subsystem as green in ROADMAP.md
   -> remove the obsolete Electron/Node code for that subsystem
   ```

3. **Never remove working code "just to clean it up".**
   There is no blanket "delete old code" task. Each deletion must reference the
   ROADMAP phase and the parity pass that justified it.

4. **Never redesign the UI.**
   The workbench, editor, Monaco, activity bar, status bar, settings UI, debug
   UI, and all VS Code screens are kept as-is. Only the native shell, lifecycle,
   IPC, filesystem, terminal, and other "backbone" layers are rewritten in Rust.

5. **Never edit unrelated mainline functionality just to make a test pass.**
   If a change is not required by the Tauri migration, do not make it.

6. **Never commit credentials.**
   GitHub tokens, PATs, API keys, or secrets are never written to any file or
   workflow. CI uses `${{ secrets.GITHUB_TOKEN }}`. Never hardcode a personal
   token in `AGENTS.md`, `ROADMAP.md`, workflows, shell history, or scripts.

7. **Keep the repository buildable after every commit.**
   You may add a new Rust/Tauri module that is not yet wired into the product,
   but you may not break the existing TypeScript/Node build for the migration
   phases that still depend on it.

## 3. Directory & Naming Conventions

- Rust/Tauri code lives under `src-tauri/` (or the subdirectory defined by the
  active phase in `ROADMAP.md`).
- The original Code - OSS Electron code stays in the repository as-is until it
  is formally retired per the phase gates.
- Reusable code (Monaco, workbench web assets, extension APIs, protocol
  definitions, language-client protocol, syntax/tokenization data, themes,
  icons, font data) is **reused**, not duplicated.
- A complete reference copy of the upstream `microsoft/vscode` source should be
  kept outside the working migration tree (a fresh clone in e.g. `~/vscode-ref`
  or a local bare mirror) so agents can diff behavior instead of guessing.

## 4. Working Contract

1. **Work on `arena/01a06e77-vscode` only** in this session. Never switch to,
   create, or push another branch for session work.
2. **One subsystem per commit.** Do not bundle unrelated migration work.
3. **Update `ROADMAP.md` in the same commit whenever phase status changes.**
4. Before opening a PR or finishing a phase, run the relevant validation:
   - `git diff --check`
   - the existing hygiene/build checks that are still relevant at that phase
   - the subsystem-specific parity test defined for that phase
5. If a test cannot be run yet, say so. Do not claim a parity gate passed.

## 5. Parity Gate Definition

A subsystem is considered migrated only when **all** of the following hold:

- The feature is available through the same UX (same commands, same UI,
  same keyboard shortcuts, same settings names).
- Native OS behavior matches: file reads/writes, watching, permissions,
  symlinks, paths, shell spawning, environment variables, clipboard, menus,
  drag-and-drop, external URIs, etc.
- Existing extensions that rely on the VS Code API keep working with the same
  extension points and no API-breaking changes.
- A written comparison against the reference upstream build exists in the
  ROADMAP or an attached issue.
- No remaining Electron/Node dependency is needed for that subsystem.

## 6. What an AI Agent Must Do Before Committing a Migration Change

1. Read `ROADMAP.md`.
2. Read the existing implementation for the subsystem it is replacing.
3. Find or write the parity test for that subsystem.
4. Add/replace the Rust/Tauri implementation **alongside** the old code.
5. Run the parity test.
6. If green, mark the phase in `ROADMAP.md`.
7. Delete the old subsystem code only in a **following** commit, referencing the
   green phase.

## 7. Forbidden

- Do not rewrite the Monaco editor or the VS Code workbench UI in native Rust
  widgets as a "clean rewrite" task.
- Do not introduce a different settings schema, command system, extension API,
  or terminal emulator unless it is byte-for-byte compatible with VS Code.
- Do not remove the existing packaging/build scripts until the Tauri build
  produces a distributable with equal feature coverage.
- Do not silence, skip, or delete existing tests that the migration must keep
  passing.
- Do not commit large generated artifacts (target/, node_modules/, dist/, etc.).
