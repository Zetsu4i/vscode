# VS Code Agents Instructions

This file provides instructions for AI coding agents working with the VS Code codebase.

For detailed project overview, architecture, coding guidelines, and validation steps, see the [Copilot Instructions](.github/copilot-instructions.md).

---

# Tauri Migration Law (Zetsu4i/vscode)

This repository is migrating **VS Code (fork of 1.137.0) from Electron to a Tauri 2 shell with a Rust native-services layer**, while remaining **behaviorally and visually 100% identical** to the original Electron app: same VS Code UI, Monaco editor, keybindings, settings, full extension compatibility, full filesystem access, integrated terminal access, and all core VS Code features — but faster, leaner, and with native Rust backbones. Every agent working here is bound by the rules below. They exist to make a very large rewrite survivable, reviewable, deterministic, and reversible. If this section conflicts with anything else, this section wins for migration concerns.

**Companion documents (keep consistent, they are part of the law):**

- [`ROADMAP.md`](ROADMAP.md) — phases, status, deletion ledger, known gaps. **Update it in the same commit that changes reality.**
- [`docs/tauri/ARCHITECTURE.md`](docs/tauri/ARCHITECTURE.md) — target architecture and design.
- [`docs/tauri/adr/`](docs/tauri/adr/) — architecture decision records. Every consequential architectural decision gets an ADR before code.

## M-0. Prime directives (non-negotiable)

1. **Parity over cleverness.** No feature, keybinding, setting, dialog, or extension API may regress or silently change behavior. "Identical" is judged against the parity checklists in `docs/tauri/parity/`, never against assumptions.
2. **The Electron app stays alive.** Until the Phase 6 cutover is declared complete in `ROADMAP.md`, the original Electron build must keep compiling and running. Never break it, never half-delete it.
3. **Extension compatibility is a hard constraint.** Extensions are JavaScript and must keep working unmodified. Any design that breaks the `vscode` API contract or the extension-host protocol is rejected.
4. **The workbench is the product.** The UI (`src/vs/**` workbench/browser layer, Monaco editor, themes, layouts) must NOT be rewritten, re-skinned, or replaced with non-VS-Code UI kits. We replace the *shell and the native services*, keeping the genuine VS Code workbench intact.
5. **No secrets, ever.** Never write tokens, passwords, or keys into code, docs, commits, issues, or config — including `AGENTS.md` and `ROADMAP.md`. Use the environment's preconfigured `git`/`gh` auth. If a secret is pasted in chat or found in the repo: treat it as compromised, do not repeat it, do not store it, and notify immediately.
6. **`ROADMAP.md` is the single source of truth.** Phase status, known gaps, and the deletion ledger move in the same commit as the code that changes them.
7. **Small, revertible commits.** One module/service per commit series. Conventional commits (see M-4).

## M-1. The Copy-Then-Delete rule (the core of the whole migration)

Original code is never deleted until its replacement is proven equivalent — and the deletion is always its own dedicated commit. Keep old code in place until it is fully copied and reimplemented, then delete reimplemented code step by step so that we always have an exact reference copy of original code. Every Electron-owned module goes through these states, tracked in the **Deletion Ledger** in `ROADMAP.md`:

| State | Name | Meaning | Rules |
|---|---|---|---|
| **A** | Original | Only the Electron/Node implementation exists | Untouchable except for explicitly-marked seams (`// tauri: seam`) |
| **B** | Dual | Rust/Tauri implementation exists alongside; original is still authoritative and still ships | The port mirrors upstream naming/structure (M-5). Original still builds and runs |
| **C** | Cutover | The Tauri implementation ships as the default; the original is kept but unwired | Requires a **green parity checklist** (`docs/tauri/parity/<module>.md`) first |
| **D** | Deleted | Original removed in a dedicated commit | Message: `chore(cutover): remove <module> (electron legacy)` + link to parity evidence. **Never mix deletion into a feature commit** |

Additional rules:

- Never delete shared code that is still referenced by any remaining Electron path.
- The workbench browser layer (`src/vs/**` UI code) is **not legacy** — it ships inside the Tauri app forever and never enters this deletion lifecycle.
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
| `out/vscode-web/` | Frontend served inside the webview. `index.html` placeholder is committed so bare `cargo check`/CI work without a frontend build; `npm run tauri:web` replaces the directory with the built VS Code web workbench |

## M-3. Parity discipline

- Every service that reaches State C needs `docs/tauri/parity/<module>.md`: a checklist of concrete scenarios (steps → expected result, copied from real VS Code behavior) plus automated tests where feasible.
- Anything user-visible is screenshot-compared against the Electron build. The UI must look and feel identical.
- Performance: record startup time, memory, and per-service benchmarks vs the Electron baseline in the parity doc. The Tauri build must be **equal or faster** — that is a stated goal of this migration.
- Anything not yet working goes into **Known Gaps** in `ROADMAP.md`. A silent stub is a bug.

## M-4. Commit conventions

- Conventional commits, scoped: `feat(tauri):`, `fix(tauri):`, `docs(roadmap):`, `refactor(seam):`, `chore(cutover): remove <module> (electron legacy)`.
- Every commit body ends with the roadmap task it advances, e.g. `Phase: 2-fs`.
- Validation expected per commit:
  - TypeScript changes: upstream build/lint stays green (targeted `npm run compile` / gulp task).
  - Rust changes: `cargo fmt --check`, `cargo clippy -- -D warnings`, `cargo check` in `src-tauri/`.
  - In sandbox sessions without crates.io egress: use local rustfmt (`@rustbin`) to verify formatting; Rust compile checks are validated in CI (`.github/workflows/tauri.yml`).

## M-5. Porting style

- Mirror upstream file, module, and service names so the port is reviewable against the original (`IWindowsService` → `src-tauri/src/services/windows.rs` + client adapter `windowsService.ts`).
- Keep upstream license headers; never relicense. New Rust crates get entries in `cgmanifest.json` per the repo's OSS policy.
- No `unsafe` without a note in an ADR. No new heavyweight frameworks or UI kits. Rust: rustfmt defaults, clippy-clean (`-D warnings`). TypeScript: upstream eslint/tsfmt rules.
- Match upstream behavior *exactly* — same defaults, same error messages, same edge cases. If upstream behavior looks like a bug: keep it, and open a ledger entry. Parity first; intentional deviations need an ADR and a roadmap note.

## M-6. Packaging & CI / CD

- CI runs via `.github/workflows/tauri.yml` validating formatting, clippy, and builds.
- Windows installer packaging: **only use NSIS builds** (`npx tauri build --bundles nsis`).
- Automated GitHub Releases: successful builds publish/update release artifacts automatically.

## M-7. Forbidden

- Force pushes, history rewrites, or anything that touches `.git`.
- Deleting or weakening existing tests; port them along with the code.
- Replacing the workbench web client or Monaco with alternative lightweight mock UIs.
- Swapping out upstream dependencies inside `src/vs/**`.
- Editing generated paths (`out/`, `out-vscode*`, `node_modules`, `dist-tauri/*` except the committed placeholder page).
- Committing secrets or requesting credentials.

## M-8. How to pick work

1. Open `ROADMAP.md` → current phase → **Next up**.
2. Move the task to **In progress** in the same commit as your first code for it.
3. Implement following M-1 through M-5; write/extend the parity checklist.
4. Update status, ledger, and gaps. Commit. Repeat.

## M-9. Environment notes

- Node 22+ & npm available.
- Local rustfmt toolchain configured via `@rustbin` for formatting validation.
- CI workflow `.github/workflows/tauri.yml` covers automated builds, clippy checks, and NSIS release artifact generation.
- All session work happens on branch `arena/01a06e68-vscode`; push with `git push origin arena/01a06e68-vscode`.
