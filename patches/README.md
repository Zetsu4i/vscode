# Patch notes

Patches are the **only** permitted modifications to the pristine upstream tree
(`upstream/`). They are generated with `git -C upstream diff` against the SHA
pinned in `UPSTREAM_SHA`, applied in filename order by
`scripts/prepare-client.sh`, and must always re-apply cleanly.

## Current patches

### 0010-tauri-bridge-registration.patch

Target: `src/vs/workbench/browser/web.main.ts` (composition root of the web
workbench).

1. Imports: `SyncDescriptor`, `IFileDialogService`, the tauri bridge
   contribution module, and the bridge classes.
2. After the `ISignService` registration: if the bridge is present
   (`TauriBridge.get()`), override `IFileDialogService` with
   `TauriFileDialogService` (native dialogs from the Rust backbone).
3. File provider registration: when the bridge is present, register
   `TauriFileSystemProvider` for the `file` scheme (real file system);
   otherwise keep upstream behavior (`HTMLFileSystemProvider` behind the
   File System Access API).

Total: 17 inserted lines, 1 modified line. Everything else in upstream stays
byte-identical.

### 0011-web-target-browser-shell.patch

Target: `build/next/index.ts` (the esbuild bundler used by the
`vscode-web-ci` gulp task).

Upstream's `web` esbuild target bundles only
`vs/workbench/workbench.web.main.internal` — it is meant for vscode.dev's
external-shell hosting. The `server-web` target (what official
`code serve-web` serves from) additionally bundles the browser shell
bootstrap `vs/code/browser/workbench/workbench` and bundles its CSS. Our
backbone serves the workbench exactly like `webClientServer.ts` does, whose
template (`out/vs/code/browser/workbench/workbench.html`) references
`workbench.js` and `workbench.css` — without this patch both 404 and the
window stays blank (this was the "installed exe shows nothing" bug, verified
against the dev-5 artifact).

1. Add `'vs/code/browser/workbench/workbench'` to the `web` target entry
   points (same as `server-web` already has).
2. Add it to the `web` target CSS bundle entry set so `workbench.css` is
   emitted next to it.

Total: 2 inserted lines. Verified locally: bundling this entry on top of the
dev-5 artifact boots the genuine workbench (welcome page renders, extension
host starts) in a headless browser.

## Regenerating

After editing upstream files inside `upstream/` (never commit them there):

```bash
git -C upstream diff > patches/NNNN-name.patch
# verify roundtrip:
git -C upstream checkout -- .
git -C upstream apply --check "$(pwd)/patches/NNNN-name.patch"
```

If upstream is re-pinned to a newer release (see AGENTS.md), regenerate every
patch and commit them together with the `UPSTREAM_SHA`/`UPSTREAM_TAG` bump.
