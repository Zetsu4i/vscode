# VS Code Agents Instructions

This file provides instructions for AI coding agents working with the VS Code codebase.

For detailed project overview, architecture, coding guidelines, and validation steps, see the [Copilot Instructions](.github/copilot-instructions.md).

---

# Tauri Migration Law (Zetsu4i/vscode)

This repository is migrating **VS Code (fork of 1.137.0) from Electron to a Tauri 2 shell with a Rust native-services layer**, while remaining **behaviorally identical** to the original Electron app: same UI, same features, full extension compatibility, full filesystem and terminal access — just faster. Every agent working here is bound by the rules below. They exist to make a very large rewrite survivable, reviewable, and reversible. If this section conflicts with anything else, this section wins for migration concerns.

**Companion documents (keep consistent, they are part of the law):**

- [`ROADMAP.md`](ROADMAP.md) — phases, status, deletion ledger, known gaps. **Update it in the same commit that changes reality.**
- [`docs/tauri/ARCHITECTURE.md`](docs/tauri/ARCHITECTURE.md) — target architecture.
- [`docs/tauri/adr/`](docs/tauri/adr/) — architecture decision records. Every consequential decision gets an ADR before code.

## M-0. Prime directives (non-negotiable)

1. **Parity over cleverness.** No feature, keybinding, setting, dialog, or extension API may regress or silently change behavior. "Identical" is judged against the parity checklists in `docs/tauri/parity/`, never against vibes.
2. **The Electron app stays alive.** Until the Phase 6 cutover is declared done in `ROADMAP.md`, the original Electron build must keep compiling and running. Never break it, never half-delete it.
3. **Extension compatibility is a hard constraint.** Extensions are JavaScript and must keep working unmodified. Any design that breaks the `vscode` API contract or the extension-host protocol is rejected, regardless of other merits.
4. **The workbench is the product.** The UI (`src/vs/**` workbench/browser layer, Monaco editor, themes, layouts) must NOT be rewritten, re-skinned, or ported to another UI framework. We replace the *shell and the native services*, not the UI.
5. **No secrets, ever.** Never write tokens, passwords, or keys into code, docs, commits, issues, or config — including `AGENTS.md` and `ROADMAP.md`. Use the environment's preconfigured `git`/`gh` auth. If a secret is pasted in chat or found in the repo: treat it as compromised, do not repeat it, do not store it, and tell the user to rotate it immediately.
6. **`ROADMAP.md` is the single source of truth.** Phase status, known gaps, and the deletion ledger move in the same commit as the code that changes them.
7. **Small, revertible commits.** One module/service per commit series. Conventional commits (see M-4).

## M-1. The Copy-Then-Delete rule (the core of the whole migration)

Original code is never deleted until its replacement is proven equivalent — and the deletion is always its own dedicated commit. Every Electron-owned module goes through these states, tracked in the **Deletion Ledger** in `ROADMAP.md`:

| State | Name | Meaning | Rules |
|---|---|---|---|
| **A** | Original | Only the Electron/Node implementation exists | Untouchable except for explicitly-marked seams (`// tauri: seam`) |
| **B** | Dual | Rust/Tauri implementation exists alongside; original is still authoritative and still ships | The port mirrors upstream naming/structure (M-5). Original still builds and runs |
| **C** | Cutover | The Tauri implementation ships as the default; the original is kept but unwired | Requires a **green parity checklist** (`docs/tauri/parity/<module>.md`) first |
| **D** | Deleted | Original removed in a dedicated commit | Message: `chore(cutover): remove <module> (electron legacy)` + link to parity evidence. **Never mix deletion into a feature commit** |

Additional rules:

- Never delete shared code that is still referenced by any remaining Electron path.
- The workbench browser layer (`src/vs/**` UI code) is **not legacy** — it ships inside the Tauri app forever and never enters this lifecycle.
- If a port turns out wrong, revert to State B and fix it. Never delete the original to "force" the new path.
- Deletion happens stepwise, module by module, exactly as their parity evidence lands — not in big sweeps.

## M-2. Where code lives

| Path | Contents |
|---|---|
| `src/vs/**` | The product UI/workbench (TypeScript). Additive seams only, marked `// tauri: seam` (e.g. service adapters injected at the web workbench factory) |
| `src-tauri/**` | Rust: Tauri shell + native services (window mgmt, fs, search, watch, pty, storage, secrets, process) |
| `src-tauri/src/services/<name>.rs` | One Rust module per upstream platform service, mirroring upstream names (`files`, `search`, `pty`, `windows`, …) |
| `cli/**` | Upstream's existing Rust CLI (`code-cli` crate, incl. tunnels). **Reuse it; never fork or duplicate it** |
| `docs/tauri/**` | Architecture, ADRs, parity checklists |
| `dist-tauri/` | Frontend served inside the webview. Only the placeholder page is committed; the built VS Code web workbench lands here in Phase 1 (gitignored) |

## M-3. Parity discipline

- Every service that reaches State C needs `docs/tauri/parity/<module>.md`: a checklist of concrete scenarios (steps → expected result, copied from real VS Code behavior) plus automated tests where feasible.
- Anything user-visible is screenshot-compared against the Electron build. The UI must look the same.
- Performance: record startup time, memory, and per-service benchmarks vs the Electron baseline in the parity doc. The Tauri build must be **equal or faster** — that is a stated goal of this migration.
- Anything not yet working goes into **Known Gaps** in `ROADMAP.md`. A silent stub is a bug.

## M-4. Commit conventions

- Conventional commits, scoped: `feat(tauri):`, `fix(tauri):`, `docs(roadmap):`, `refactor(seam):`, `chore(cutover): remove <module> (electron legacy)`.
- Every commit body ends with the roadmap task it advances, e.g. `Phase: 2-fs`.
- Validation expected per commit:
  - TypeScript changes: upstream build/lint stays green (targeted `npm run compile` / gulp task).
  - Rust changes: `cargo fmt --check`, `cargo clippy -- -D warnings`, `cargo check` in `src-tauri/`.
  - If the environment has no crates.io egress (true of the current sandbox), write `rust-check: deferred (no crates.io egress)` in the commit body. At most **one** consecutive batch of deferred Rust commits; the next environment with access (normally CI — `.github/workflows/tauri.yml`) must validate before more Rust work stacks on top.

## M-5. Porting style

- Mirror upstream file, module, and service names so the port is reviewable against the original (`IWindowsService` → `src-tauri/src/services/windows.rs` + client adapter `windowsService.ts`).
- Keep upstream license headers; never relicense. New Rust crates get entries in `cgmanifest.json` per the repo's OSS policy.
- No `unsafe` without a note in an ADR. No new heavyweight frameworks or UI kits. Rust: rustfmt defaults, clippy-clean. TypeScript: upstream eslint/tsfmt rules.
- Match upstream behavior *exactly* — same defaults, same error messages, same edge cases. If upstream behavior looks like a bug: keep it, and open a ledger entry. Parity first; intentional deviations need an ADR and a roadmap note.

## M-6. Forbidden

- Force pushes, history rewrites, or anything that touches `.git`.
- Deleting or weakening existing tests; port them along with the code.
- Replacing the workbench web client or Monaco with alternative UIs.
- Swapping out upstream dependencies inside `src/vs/**`.
- Editing generated paths (`out/`, `out-vscode*`, `node_modules`, `dist-tauri/*` except the committed placeholder page).
- Inventing new product behavior upstream doesn't have (features require an ADR + roadmap entry).
- Committing secrets (see M-0.5) or asking the user for credentials — GitHub auth is preconfigured; if it fails, tell the user to reconnect GitHub in Arena instead of requesting tokens.

## M-7. How to pick work

1. Open `ROADMAP.md` → current phase → **Next up**.
2. Move the task to **In progress** in the same commit as your first code for it.
3. Implement following M-1 through M-5; write/extend the parity checklist.
4. Update status, ledger, and gaps. Commit. Repeat.

## M-8. Environment notes (current sandbox)

- Node 22 + npm available; npm registry reachable.
- **No Rust toolchain and no crates.io egress** in this sandbox: Rust compile checks run in CI (`.github/workflows/tauri.yml` — `rust-check` job on 3 OSes, plus a Windows release-build job). Follow the deferral rule in M-4.
- All work happens on the session branch `arena/01a06ddd-vscode`; push with `git push origin arena/01a06ddd-vscode`.
