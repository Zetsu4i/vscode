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
