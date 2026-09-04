# Sandbox environment notes (AI-agent sessions)

The Arena sandbox used for this migration has **restricted network egress** and **no Rust toolchain**. This file records every adaptation needed to build and verify the VS Code web workbench inside it, so future sessions don't rediscover them. None of these adaptations are needed on a normal dev machine or in CI.

## Reachability matrix (measured 2026-09-04)

| Host | Status |
|---|---|
| registry.npmjs.org, github.com, api.github.com, codeload.github.com | ✅ reachable |
| nodejs.org, electronjs.org, crates.io, rust-lang.org, marketplace/update endpoints, `release-assets.githubusercontent.com`, npmmirror | ❌ blocked |

## Install recipe (sandbox)

1. `npm ci --ignore-scripts` — root deps without lifecycle scripts (skips node-gyp/electron binary, whose header/binary hosts are blocked).
2. `cd build && npm install --ignore-scripts` — the gulp/esbuild toolchain.
3. `NODE_EXTRA_CA_CERTS=/etc/ssl/certs/ca-certificates.crt npm_command=ci npm_config_ignore_scripts=true node build/npm/postinstall.ts` — installs all sub-dirs (extensions, remote/web, …). The CA env is required because node's fetch doesn't trust the sandbox proxy CA.
   - `ensureElectronTypes()` downloads `electron.d.ts` from a blocked host: work around by placing the file from the npm tarball `electron@<version>` (identical content, sha256 verified against `build/checksums/electron.txt`) into `.build/typings/electron.d.ts`.
4. Marketplace built-ins: create `~/.vscode-oss-dev/extensions/control.json` disabling them (upstream developer mechanism, see `build/lib/builtInExtensions.ts`) since GitHub release assets are blocked:
   `{"ms-vscode.js-debug-companion":"disabled","ms-vscode.js-debug":"disabled","ms-vscode.vscode-js-profile-table":"disabled"}`

## Build recipe (sandbox)

`npm run tauri:web` (≈3.5 min) with these env tweaks:

- **Memory**: the esbuild bundle OOM-kills at default sizes. Create swap first:
  `sudo fallocate -l 6G /swapfile && sudo chmod 600 /swapfile && sudo mkswap /swapfile && sudo swapon /swapfile` (swap is outside the repo/workspace; re-create if the sandbox restarts).
- `NODE_OPTIONS="--max-old-space-size=6144"` — the node-side NLS collector needs >2 GB heap.
- `GOGC=40` — tames esbuild (Go) peak memory.

Outputs: `out-vscode-web/` (bundle, gitignored) → packaged site `../vscode-web` (outside repo) → harness copies to `out/vscode-web/` (gitignored, snapshot-excluded) and post-processes:

- renders `workbench.html` like upstream `webClientServer.ts` (`renderWorkbenchTemplate`)
- bundles the browser-shell loader `vs/code/browser/workbench/workbench.js` (the upstream `web` target omits it by design — "no browser shell")
- copies `resources/server/*` for the template's brand links

## Rust work in the sandbox

No cargo/crates.io. Rules: `AGENTS.md` §M-4 — write `rust-check: deferred (no crates.io egress)` in the commit body (max one consecutive batch) and let CI (`.github/workflows/tauri.yml` — see `docs/tauri/ci/`) validate. The Rust shell (`src-tauri/`) is minimal until Phase 1 Rust work begins.

## What normal machines don't need

A regular dev machine (or CI) with unrestricted network just runs:

```bash
npm ci            # full install incl. native modules + electron binary
npm run tauri:web # build + package the web workbench
npm run tauri:dev # open the Tauri shell
```
