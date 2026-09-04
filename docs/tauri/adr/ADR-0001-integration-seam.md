# ADR-0001 — Integration seam between the workbench and the Rust backend

Status: accepted · Date: 2026-09-04

## Context

The VS Code web build (what vscode.dev runs) can instantiate its workbench either (a) against a *remote* server speaking the VS Code remoting protocol, or (b) in *web/factory mode*, where client-side service implementations are injected at the workbench construction seam. We need the Tauri webview to run a fully functional **local** workbench whose filesystem, terminal, search, and storage live in Rust.

## Decision

**Factory-mode service injection.** Build the upstream web workbench unchanged, load it in the Tauri webview, and inject thin TS adapters that implement upstream service interfaces and forward over typed Tauri IPC to Rust services. All seams are marked `// tauri: seam` and concentrated in designated files.

## Alternatives considered

1. **Rust server speaking the VS Code remote wire protocol** — the web client connects as if to a remote machine. Most faithful long-term, but heavy up-front protocol work before any user-visible progress. Revisit if the seam grows unwieldy.
2. **Stock node `code server` sidecar with Tauri as a pure shell** — fastest path to a working app, but keeps Node for everything forever, contradicting the migration goal. **Kept as a CI reference implementation** for parity testing and as an emergency fallback during Phases 1–3.

## Consequences

- We own a thin seam layer that must track upstream: monthly merges; name-mirroring (`AGENTS.md` M-5) keeps the diff reviewable.
- No wire-protocol compatibility burden during Phase 2.
- Parity testing can diff Tauri behavior against both the Electron build and the sidecar reference.
