# VSTauri — Visual Studio Code on a Rust backbone

The **real** Visual Studio Code — the genuine upstream workbench, byte-identical
UI — running inside a **Tauri 2** shell with a **Rust backbone** instead of
Electron. No lookalike UI, no hallucinated features: the workbench is built
from `microsoft/vscode` at a pinned release and served by a local Rust server
that provides the native services a browser lacks.

```
upstream (pristine vscode @ UPSTREAM_SHA)
   + patches/  (2 surgical registration seams, ~20 lines)
   + bridge/   (upstream-interface implementations in TS)
   = vscode-web client  ──served by──>  Rust backbone (axum)
                                          ├─ real file system + watcher
                                          ├─ real PTY terminals (ConPTY)
                                          ├─ native dialogs (rfd)
                                          └─ Tauri 2 window shell
```

## Why this architecture

| Old attempt (deleted)                     | Now                                                |
| ----------------------------------------- | -------------------------------------------------- |
| Hand-built React lookalike workbench      | Pristine upstream workbench, 100% identical UI     |
| Hallucinated features & behavior          | Upstream semantics, bridge implements upstream APIs |
| `git`/`rg`/`node` subprocesses → cmd popups on Windows | Pure Rust libraries + ConPTY, fully headless |
| Everything reimplemented                  | Only the *backbone* is rebuilt; UI/UX stays upstream |

## Repository layout

- `upstream/` — pristine `microsoft/vscode` clone at `UPSTREAM_SHA` (fetched by
  `scripts/fetch-upstream.sh`, never committed)
- `patches/` — the only modifications to upstream (see `patches/README.md`)
- `bridge/` — TS code compiled inside the upstream build implementing upstream
  interfaces (`IFileSystemProvider`, `ITerminalBackend`, dialogs)
- `src-tauri/` — Tauri 2 shell + Rust backbone (axum HTTP/WS server, fs, pty,
  dialogs, watcher)
- `scripts/` — fetch/prepare/build the client
- `AGENTS.md` — binding rules for AI agents
- `ROADMAP.md` — living phased roadmap

## Building

```bash
scripts/fetch-upstream.sh     # clone pinned microsoft/vscode
scripts/prepare-client.sh     # copy bridge in, apply patches
scripts/build-client.sh       # npm ci + gulp vscode-web -> ./vscode-web
cargo tauri dev               # or: cargo build --release in src-tauri
```

CI (`.github/workflows/ci.yml`) builds the client from the pinned upstream,
bundles Linux (`.deb`, `.AppImage`) and Windows (**NSIS only**) installers, and
publishes every green build as a GitHub Release (`dev-<run_number>`).

## Status

See `ROADMAP.md`. Phase 0 (architecture, backbone, CI, releases) is done;
Phase 1 (workbench boots and feels like VS Code) is in progress.

## License

MIT, matching upstream VSCode. Microsoft and Visual Studio Code are trademarks
of Microsoft Corporation; this project is an independent build.
