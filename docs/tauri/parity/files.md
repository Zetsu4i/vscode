# Parity: files

State: **B** (dual — Rust/Tauri implementation exists and is wired in the Tauri shell; original Electron/Node paths untouched and still authoritative for the Electron build)

Upstream ref: `src/vs/platform/files/common/files.ts` (`IFileSystemProvider`), `src/vs/platform/files/node/diskFileSystemProvider.ts`
Rust/Tauri home: `src-tauri/src/services/files.rs` (+ seam client `src/vs/workbench/services/tauri/browser/tauriFileSystemProvider.ts`, registered in `src/vs/workbench/browser/web.main.ts` `// tauri: seam`)

## Scenarios (steps → expected result, from real Electron VS Code behavior)

- [ ] Open folder (`workbench.html?folder=file:///<path>`) → explorer shows the folder tree with correct names/ordering
- [ ] Open a text file from explorer → content renders, model language detected
- [ ] Edit + Ctrl+S → file on disk changes (verify with an external tool), dirty marker clears
- [ ] Create new file in explorer → exists on disk; create in nested new folder → parents created
- [ ] Rename file/folder in explorer (overwrite case) → disk reflects it
- [ ] Delete file / folder / non-empty folder without trash → disk reflects it; non-recursive delete of non-empty folder errors like Electron
- [ ] Readonly file (chmod 444) → stat.permissions Readonly → editor read-only affordances
- [ ] Symlink to file → stat type SymbolicLink (lstat semantics, not followed)
- [ ] Save a large file (~10 MB) → completes; perf noted vs Electron baseline (gap: JSON-array IPC)
- [ ] Encoding round-trip: save UTF-8 with BOM / Latin-1 → bytes on disk match Electron behavior
- [ ] Error surfaces: open non-existent path → user-visible error identical to Electron's

## Automated tests

- [ ] Rust unit tests for `files.rs` stat/mapping helpers (follow-up slice)
- [ ] Provider contract tests against a temp dir (TS side, follow-up slice)
- [ ] n/a — manual only for slice A (GUI verification needs a Tauri-capable machine)

## Perf vs Electron baseline

pending first GUI verification (needs Tauri-capable machine). Known slice-A cost: IPC payloads are JSON (readFile = number arrays, writeFile = Array.from) — switch to raw `tauri::ipc::Response`/ArrayBuffer before State C.

## Screenshots / notes

Slice-A ledgered gaps (also in ROADMAP.md): no file watching (`watch` returns `Disposable.None`) — Rust watcher service is the next Phase 2 slice; `useTrash` rejected (no trash service yet); no `copy`/`cloneFile`; no `readFileStream`/open-close; error strings are `io::Error` text, not upstream `FileOperationError` codes.
