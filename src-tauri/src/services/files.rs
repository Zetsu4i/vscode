// tauri: Phase 2 seam — Rust file system service.
//
// Mirrors the platform file operations behind `IFileSystemProvider`
// (src/vs/platform/files/common/files.ts) and is consumed by the
// TauriFileSystemProvider seam (src/vs/workbench/services/tauri/browser/).
// Upstream reference: src/vs/platform/files/node/diskFileSystemProvider.ts.
//
// Phase 2 slice A (state B — dual, see ROADMAP.md Deletion Ledger):
// stat/exists/readdir/read/write/mkdir/rename/delete. Not yet implemented
// (ledgered gaps): file watching, trash, recursive copy, atomic writes.
// Error strings are temporary; they must converge on upstream
// `FileOperationError` codes before State C (see docs/tauri/parity/files.md).

use serde::Serialize;

/// Mirrors `FileType` in src/vs/platform/files/common/files.ts.
const FILE_TYPE_UNKNOWN: u8 = 0;
const FILE_TYPE_FILE: u8 = 1;
const FILE_TYPE_DIRECTORY: u8 = 2;
const FILE_TYPE_SYMBOLIC_LINK: u8 = 3;

/// Mirrors `FilePermission.Readonly` in src/vs/platform/files/common/files.ts.
const FILE_PERMISSION_READONLY: u8 = 1;

/// Mirrors `IStat` (camelCase fields via serde).
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FileStat {
    file_type: u8,
    ctime: u64,
    mtime: u64,
    size: u64,
    permissions: Option<u8>,
}

fn file_type_of(is_symlink: bool, is_dir: bool, is_file: bool) -> u8 {
    if is_symlink {
        FILE_TYPE_SYMBOLIC_LINK
    } else if is_dir {
        FILE_TYPE_DIRECTORY
    } else if is_file {
        FILE_TYPE_FILE
    } else {
        FILE_TYPE_UNKNOWN
    }
}

fn epoch_millis(time: std::io::Result<std::time::SystemTime>) -> u64 {
    time.ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map_or(0, |d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX))
}

fn permissions_of(md: &std::fs::Metadata) -> Option<u8> {
    if md.permissions().readonly() {
        Some(FILE_PERMISSION_READONLY)
    } else {
        None
    }
}

fn stat_from_metadata(md: std::fs::Metadata) -> FileStat {
    let file_type = file_type_of(md.file_type().is_symlink(), md.is_dir(), md.is_file());
    FileStat {
        file_type,
        ctime: epoch_millis(md.created()),
        mtime: epoch_millis(md.modified()),
        size: md.len(),
        permissions: permissions_of(&md),
    }
}

/// Mirrors `IFileSystemProvider.stat` (lstat semantics: does not follow symlinks).
#[tauri::command]
pub async fn fs_stat(path: String) -> Result<FileStat, String> {
    tokio::fs::symlink_metadata(&path)
        .await
        .map(stat_from_metadata)
        .map_err(|e| e.to_string())
}

/// Existence probe used before operations that must not create anything.
#[tauri::command]
pub async fn fs_exists(path: String) -> Result<bool, String> {
    tokio::fs::try_exists(&path)
        .await
        .map_err(|e| e.to_string())
}

/// Mirrors `IFileSystemProvider.readdir`: entries as `[name, FileType]` tuples.
#[tauri::command]
pub async fn fs_readdir(path: String) -> Result<Vec<(String, u8)>, String> {
    let mut rd = tokio::fs::read_dir(&path)
        .await
        .map_err(|e| e.to_string())?;
    let mut entries = Vec::new();
    while let Some(entry) = rd.next_entry().await.map_err(|e| e.to_string())? {
        let name = entry.file_name().to_string_lossy().into_owned();
        let ft = entry.file_type().await.map_err(|e| e.to_string())?;
        let file_type = file_type_of(ft.is_symlink(), ft.is_dir(), ft.is_file());
        entries.push((name, file_type));
    }
    Ok(entries)
}

/// Mirrors `IFileSystemProvider.readFile`.
#[tauri::command]
pub async fn fs_read_file(path: String) -> Result<Vec<u8>, String> {
    tokio::fs::read(&path).await.map_err(|e| e.to_string())
}

/// Mirrors `IFileSystemProvider.writeFile` (slice A: direct write;
/// upstream's atomic tmp-file+rename save lands before State C).
#[tauri::command]
pub async fn fs_write_file(path: String, contents: Vec<u8>) -> Result<(), String> {
    tokio::fs::write(&path, contents)
        .await
        .map_err(|e| e.to_string())
}

/// Mirrors `IFileService.mkdir` (creates all missing parents).
#[tauri::command]
pub async fn fs_mkdir(path: String) -> Result<(), String> {
    tokio::fs::create_dir_all(&path)
        .await
        .map_err(|e| e.to_string())
}

/// Mirrors `IFileSystemProvider.rename` with `IFileOverwriteOptions.overwrite`.
#[tauri::command]
pub async fn fs_rename(from: String, to: String, overwrite: bool) -> Result<(), String> {
    if overwrite && tokio::fs::try_exists(&to).await.unwrap_or(false) {
        fs_delete_impl(&to, true).await?;
    }
    tokio::fs::rename(&from, &to)
        .await
        .map_err(|e| e.to_string())
}

/// Mirrors `IFileSystemProvider.delete` with `IFileDeleteOptions`.
/// Ledgered gap: `useTrash` is ignored in slice A (permanent delete only).
#[tauri::command]
pub async fn fs_delete(path: String, recursive: bool, use_trash: bool) -> Result<(), String> {
    if use_trash {
        // Parity gap (ROADMAP known gaps): OS trash arrives with the watcher slice.
        let msg = "fs_delete: useTrash not implemented yet (see parity/files.md)";
        return Err(msg.to_string());
    }
    fs_delete_impl(&path, recursive).await
}

async fn fs_delete_impl(path: &str, recursive: bool) -> Result<(), String> {
    let md = tokio::fs::symlink_metadata(path)
        .await
        .map_err(|e| e.to_string())?;
    if md.is_dir() {
        if recursive {
            tokio::fs::remove_dir_all(path)
                .await
                .map_err(|e| e.to_string())
        } else {
            tokio::fs::remove_dir(path).await.map_err(|e| e.to_string())
        }
    } else {
        tokio::fs::remove_file(path)
            .await
            .map_err(|e| e.to_string())
    }
}
