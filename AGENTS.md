# AGENTS.md — Rules for AI Agents Working on VSTauri

This repository converts **Microsoft's real Visual Studio Code** into a Tauri 2
application. The UI must remain **100% identical to upstream VSCode** — agents
rebuild the *backbone* in Rust, never the interface. Read this file completely
before changing anything. Violating these rules has already cost us one full
rewrite: a previous attempt hand-built a lookalike React workbench, hallucinated
features, and was discarded.

## Source of truth

1. **`upstream/` is the pristine clone of `microsoft/vscode`** at the commit
   pinned in `UPSTREAM_SHA` / `UPSTREAM_TAG`. It is never committed to this
   repo and never edited except through `patches/*.patch` (applied by
   `scripts/prepare-client.sh`).
2. **Never invent or redesign UI or features.** If a feature exists upstream,
   the answer is to wire it to the Rust backbone, not to re-implement it. If
   you find yourself writing React/HTML/CSS for workbench UI, STOP — you are
   hallucinating. The workbench is built from upstream source by
   `scripts/build-client.sh` (`gulp vscode-web`).
3. Reusable upstream code is reused through normal imports. New code lives in
   `bridge/src/vs/workbench/contrib/tauriBridge/**` and compiles *inside* the
   upstream build.

## Translation discipline (old code lives until the new one is 100%)

- The upstream tree is **kept whole**. We do **not** delete upstream code when
  we take over one of its jobs (e.g. Electron main process code). Deletion is
  the **last** step of every phase, never the first.
- A piece of upstream code may only be removed after its replacement is
  **100% translated and verified working** (built in CI *and* manually
  exercised). Until then it stays, dormant, exactly as upstream ships it.
- Deletions happen **step by step through patch files**, each documented, so
  the repo always builds and the diff to upstream stays reviewable:
  `patches/NNNN-short-description.patch`.
- Never modify a patch to "make things easier" — a patch that fights upstream
  is a sign the bridge is wrong, not upstream.

## Patch rules

- Patches are generated from real `git -C upstream diff` output against the
  pinned SHA and must re-apply cleanly (`git apply --check`). If upstream is
  ever re-pinned to a newer release, regenerate every patch in the same
  commit and update `UPSTREAM_SHA`/`UPSTREAM_TAG` with it.
- Keep the patch surface **minimal and surgical**: registration seams in
  composition roots (`web.main.ts`), never editor/workbench internals.
- Every patch hunk is commented with `--- VSTauri bridge (added) ---`.

## Bridge rules

- The bridge (`tauriBridge/**`) may only implement **upstream interfaces**
  (`IFileSystemProvider`, `ITerminalBackend`, `IFileDialogService`, ...).
  Model implementations on their upstream counterparts (`RemotePty`,
  `RemoteTerminalBackend`, `HTMLFileSystemProvider`) instead of inventing
  behavior.
- Behavior parity beats convenience: if upstream semantics are unknown, read
  the upstream implementation in `upstream/src/...` and mirror it (e.g. the
  workbench HTML template rendering mirrors `webClientServer.ts`).
- The bridge must degrade gracefully: when `globalThis.__VSTAURI__` is absent
  (plain vscode-web in a normal browser) everything must behave exactly like
  upstream.

## Backbone (Rust) rules

- **Everything spawns headless.** The old build popped cmd windows on Windows
  because child processes were spawned without `CREATE_NO_WINDOW`. Any future
  use of `std::process::Command` on Windows MUST set
  `creation_flags(0x08000000)` (CREATE_NO_WINDOW). Prefer libraries over
  subprocesses entirely (we use `portable-pty`/ConPTY, `notify`, `trash`,
  `rfd` — none of which open windows). A regression here is release-blocking.
- No subprocess shells out to tools the libraries can do (`git`, `rg`, `node`
  lookups caused the popup plague). New backbone services go in
  `src-tauri/src/server/` and are exposed through `bridge/rpc/<method>`.
- The backbone never trusts the client: every bridge call/websocket carries
  the per-session token; paths are validated server-side.
- Validate Rust with `cargo fmt` locally; full type checking happens in CI.
  If you have no cargo, do not "fix" Rust by guessing — read the pinned crate
  sources or let CI compile.

## Process rules

- `ROADMAP.md` is a **living document**: when a phase starts, mark it
  in-progress; when its acceptance criteria are met, check it off and record
  what actually shipped. Never merge work that isn't reflected there.
- One logical change per commit; every commit must keep CI green.
- CI is the source of truth for builds. Artifacts from a green build are
  published as a GitHub Release automatically — never hand out local builds.
- Windows bundles are **NSIS only** (no MSI) — this is a hard product
  decision; keep it in `tauri.windows.conf.json` and the workflow.

## Security rules

- The backbone binds `127.0.0.1` only and requires the session token on every
  bridge request. Never widen this without a design note in the ROADMAP.
- Never commit tokens, credentials, or the client build output
  (`vscode-web/`, `src-tauri/resources/client/` are gitignored).
