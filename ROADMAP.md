# ROADMAP — VS Code on Tauri 2 (Zetsu4i/vscode)

> **Goal.** Ship this fork as a Tauri 2 desktop app that is behaviorally identical to the Electron original: same UI, same features, full extension compatibility, full filesystem and terminal access — with a Rust native layer that makes it faster and lighter.
>
> **Method.** Copy-Then-Delete (see `AGENTS.md` §M-1): original code stays authoritative until its Rust/Tauri replacement passes the parity checklist; then the original is deleted stepwise in dedicated cutover commits. Every phase leaves the Electron build working until Phase 6 says otherwise.
>
> **Last updated:** 2026-09-04 · **Current phase:** 1 — Shell parity (Phase 0 done except parked CI activation + GUI smoke test) · Branch: `arena/01a06ddd-vscode`

Status legend: ⬜ not started · 🟨 in progress · ✅ done · ⏸ blocked

---

## Phase 0 — Foundations & flight plan 🟨

Set up scaffolding, tooling, and rules. Zero behavior change to the Electron app.

- ✅ Migration law: `AGENTS.md` (Copy-Then-Delete lifecycle, commit rules, secrets policy)
- ✅ This roadmap: phase gates, deletion ledger, known gaps
- ✅ Target architecture + initial ADRs (`docs/tauri/`)
- ✅ Tauri 2 scaffold (`src-tauri/`) with shell window (`npm run tauri:dev` wiring included)
- ⏸ CI: `cargo fmt/clippy/check` on Linux/Windows/macOS — workflow file ready at `docs/tauri/ci/tauri-workflow.yml`; **blocked**: needs a maintainer to copy it to `.github/workflows/tauri.yml` (sandbox GitHub connection lacks the `workflows` permission)
- ✅ `npm ci` verified in-sandbox (see `docs/tauri/SANDBOX.md` for the offline adaptations; full script-enabled install works on normal dev machines)
- ✅ Web workbench bundle scripted + verified: `npm run tauri:web` runs the upstream `vscode-web-ci` gulp task (esbuild straight from `src/`, 12 bundles) and packages a servable site into `out/vscode-web/`
- ✅ Parity checklist template ready for use (`docs/tauri/parity/README.md`)

**Exit criteria:** CI green on all three OSes; on any dev machine with Rust installed, `npm run tauri:dev` opens a native window with the placeholder page; Electron build untouched and green.

---

## Phase 1 — Shell parity: Tauri runs the real workbench 🟨

Replace the Electron window with a Tauri window that loads the genuine VS Code web workbench build. The UI must be indistinguishable from Code.

- ✅ Bundle the web workbench (upstream `vscode-web-ci` gulp task) → served from `out/vscode-web/` via Tauri static protocol; bootstrap pre-rendered like upstream's `webClientServer.ts` + browser-shell loader bundled (see `scripts/tauri/build-web.mjs`)
- [ ] **Verify on a machine with Rust + webview deps:** `npm run tauri:dev` opens a real workbench (empty workspace); screenshot-compare vs Electron
- [ ] Window lifecycle parity: multi-window, window titles (folder/file), zoom, fullscreen, focus/restore — Rust port of the behaviors in `src/vs/platform/windows/electron-main/*`
- [ ] Native menu bar (Tauri menu API mirroring the upstream menu model), native dialogs, clipboard, `openExternal`
- [ ] Boot performance baseline recorded vs Electron (`docs/tauri/parity/shell.md`)

**Exit criteria:** `npm run tauri:dev` opens a real Code workbench window (empty workspace, no fs yet) that looks and behaves like Code on first run; screenshots in the parity doc.

---

## Phase 2 — Core platform services in Rust ⬜

Service-by-service A→B→C porting. **Each service is its own commit series + parity checklist + ledger row.**

| Service | Upstream reference | Rust approach |
|---|---|---|
| File system | `src/vs/platform/files` | Tauri IPC file provider: read/write/stream/rename/copy/trash, encoding detection, atomic saves |
| Search | ripgrep integration | ripgrep as library (`grep-searcher`/`grep-regex`/`ignore`), flag-compatible with the CLI invocation upstream uses |
| File watcher | parcel watcher | `notify` crate + coalescing layer matching vscode event semantics (recursive, excludes) |
| Configuration & user data | `src/vs/platform/configuration` | same file formats, same directories (`dataFolderName` from `product.json`) |
| State storage | Electron storage (`stateService`) | SQLite (`rusqlite`), same keys/schema semantics |
| Secret storage | Electron safeStorage | `keyring` crate (OS keychain) |
| Process exec | child_process usage | `tokio::process` with upstream flag/exit-code semantics |

**Exit criteria:** open folder → edit → save → search/replace across files → settings persist across restart — all through the Rust backend, indistinguishable from Electron.

---

## Phase 3 — Terminal, tasks, debug plumbing ⬜

- [ ] PTY host in Rust (`portable-pty`): ConPTY (Windows) / pty (unix), resize, flow control, shell-integration injection — same observable behavior as node-pty
- [ ] Terminal UI stays the workbench's xterm.js (untouched) — only the host moves
- [ ] Tasks + problem matchers running on the Rust process service
- [ ] Debug Adapter Protocol: adapter processes launched via the Rust process service (adapters themselves are extensions → full support in Phase 4)
- [ ] Latency/throughput benchmarks vs Electron recorded

**Exit criteria:** integrated terminal and tasks feel and behave identically; numbers in `docs/tauri/parity/terminal.md`.

---

## Phase 4 — Extension host on Tauri (highest risk) ⬜

Extensions are JavaScript; a pure-Rust extension host would break them and is rejected by `AGENTS.md` M-0.3. Execute [ADR-0002](docs/tauri/adr/ADR-0002-extension-host.md):

- [ ] **4a:** web extension host first (upstream already runs extensions in a web worker)
- [ ] **4b:** desktop extensions — upstream extension-host TS layer executing inside a Rust-managed, Node-compatible embedded JS runtime; Rust owns process lifecycle, IPC, and the Phase-2/3 native backends. Fallback: dedicated Node sidecar hosting *only* the extension host
- [ ] Extension scanning, VSIX install (zip/signatures), marketplace per `product.json`
- [ ] Run upstream's `vscode` API test corpus; validate flagship extensions (theme, language server, linter, SCM)

**Exit criteria:** desktop extensions install and behave; API compat suite green.

---

## Phase 5 — Full feature parity & performance ⬜

- [ ] SCM (git via the git CLI exactly like upstream), notebooks, testing UI, profiles & settings sync
- [ ] Remote/tunnels — reuse the existing Rust `cli/` crate instead of reimplementing
- [ ] Accessibility & IME audits on all three system webviews (WebView2 / WKWebView / WebKitGTK)
- [ ] i18n/language packs, updates (Tauri updater), crash reporting, enterprise policies
- [ ] Platform hardening: every rendering/IME/perf quirk ledgered and fixed or explicitly documented
- [ ] Performance vs Electron: startup, memory, large-repo fs/search/terminal — publish numbers here

**Exit criteria:** parity checklists 100% green except ledgered gaps; performance ≥ Electron.

---

## Phase 6 — Cutover & cleanup ⬜

- [ ] Deletion ledger executed stepwise: Electron main process, node-only services, Electron packaging — each in its own `chore(cutover):` commit
- [ ] Tauri bundler packaging: icons + product identity from `product.json`, per-OS signing, update channel
- [ ] Final state: the repo produces no Electron app; `Code - Tauri` release artifacts for Windows/macOS/Linux

**Exit criteria:** zero Electron runtime dependencies; parity dashboard clean.

---

## Deletion Ledger

Copy-Then-Delete states — **A** original → **B** dual → **C** cutover → **D** deleted (definitions in `AGENTS.md` §M-1). Every row change happens in the same commit as the code.

| Module | State | Rust/Tauri home | Parity doc | Deleted in |
|---|---|---|---|---|
| *(empty — scaffold only so far)* | | | | |

---

## Known Gaps (what the Tauri build can't do *yet*)

| Gap | Planned phase |
|---|---|
| No filesystem access from the workbench | 2 |
| No integrated terminal | 3 |
| No desktop extension host | 4 |
| WebKitGTK / WKWebView rendering & IME audit not yet run | 5 |
| Rust compile checks can't run in this sandbox (no crates.io egress) — CI covers them | 0 |
| 3 marketplace built-ins (js-debug, js-debug-companion, js-profile-table) are **disabled in sandbox builds only** (`~/.vscode-oss-dev/extensions/control.json`) because the release-assets CDN is blocked here; normal machines build them from GitHub releases automatically | 1 |
| Sandbox web bundle is unminified (`vscode-web-ci`); releases use `vscode-web-min` | 5 |
| Dynamic workbench bootstrap (per-request nonce, query-driven folder open) is pre-rendered static for now; the Rust custom protocol replaces this when window/query parity lands | 1–2 |

---

## Risks

| Risk | Mitigation |
|---|---|
| WebKitGTK (Linux) rendering/IME/performance differences vs Chromium | early audit in Phase 1, per-quirk ledger, per-OS CI |
| Extension-host JS runtime compatibility (Phase 4) | ADR-0002 with credible fallback; upstream API test corpus as the gate |
| Fork drift vs upstream (monthly VS Code releases) | monthly upstream merges; the seam layer stays thin and name-mirrored (M-5) |
| "Identical" is fuzzy | parity checklists + screenshots are the definition of done, no exceptions |

---

## Next up

1. Maintainer: activate CI (`docs/tauri/ci/tauri-workflow.yml` → `.github/workflows/tauri.yml`)
2. On a Rust machine: `npm run tauri:web && npm run tauri:dev` → verify the real workbench boots; capture screenshots into `docs/tauri/parity/shell.md`
3. Begin Phase 2 seam design: `files` service (Rust `files.rs` + TS adapter at the workbench factory seam)

---

*Sandbox environment notes live in [`docs/tauri/SANDBOX.md`](docs/tauri/SANDBOX.md). Rules of engagement live in [`AGENTS.md`](AGENTS.md). Architecture decisions live in [`docs/tauri/`](docs/tauri/ARCHITECTURE.md). Update this file in the same commit as the work it describes.*
