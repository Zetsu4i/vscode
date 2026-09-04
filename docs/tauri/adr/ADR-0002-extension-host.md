# ADR-0002 — Extension host strategy

Status: accepted · Date: 2026-09-04

## Context

Extensions are JavaScript/TypeScript bundles that call the `vscode` API and, on desktop, use Node built-ins. `AGENTS.md` M-0.3 requires extensions to work **unmodified**. Therefore a "pure Rust extension host" that re-implements the API surface would break effectively every extension and is **rejected**. What *can* be Rust is the host around the code: process lifecycle, supervision, IPC, sandboxing, and the fs/pty/process backends (Phases 2–3).

## Decision

1. **Phase 4a — web extensions first.** Upstream already runs a worker-based web extension host; enable it on the Tauri build. Most themes, formatters, and language features ship web variants.
2. **Phase 4b — desktop extensions.** Run the **upstream extension-host TS layer inside a Node-compatible embedded JS runtime** (deno_core-class, with Node built-in shims implemented in Rust where needed), launched and supervised by Rust, backed by the Rust native services.
3. **Fallback (credible, documented, ledgered).** A dedicated **Node sidecar that hosts only the stock extension host**, with everything else in Rust. This preserves full extension compatibility even if the embedded runtime hits a wall; it counts as a Known Gap against the "no Node" end-state, not a silent compromise.

## Consequences

- Highest-risk phase of the roadmap. Gated by a spike: run three flagship extensions (a theme, a language server such as rust-analyzer, a linter) before mass validation.
- The upstream `vscode` API test corpus is the acceptance gate.
- The end-state honest definition: *shell + native services in Rust/Tauri; workbench UI + extension code remain the upstream TypeScript, executing exactly as they do today.* Anything claiming more ("100% of all TS rewritten in Rust") would break the extension compatibility requirement and is out of scope by M-0.3.
