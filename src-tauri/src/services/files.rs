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
    let millis = time.ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok());
    match millis {
        Some(d) => d.as_millis() as u64,
        None => 0,
    }
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
    let md = tokio::fs::symlink_metadata(&path).await;
    match md {
        Ok(md) => Ok(stat_from_metadata(md)),
        Err(e) => Err(e.to_string()),
    }
}

/// Existence probe used before operations that must not create anything.
#[tauri::command]
pub async fn fs_exists(path: String) -> Result<bool, String> {
    let exists = tokio::fs::try_exists(&path).await;
    match exists {
        Ok(exists) => Ok(exists),
        Err(e) => Err(e.to_string()),
    }
}

/// Mirrors `IFileSystemProvider.readdir`: entries as `[name, FileType]` tuples.
#[tauri::command]
pub async fn fs_readdir(path: String) -> Result<Vec<(String, u8)>, String> {
    let mut rd = tokio::fs::read_dir(&path).await;
    match rd {
        Err(e) => Err(e.to_string()),
        Ok(mut rd) => {
            let mut entries = Vec::new();
            loop {
                let next = rd.next_entry().await;
                match next {
                    Err(e) => return Err(e.to_string()),
                    Ok(None) => return Ok(entries),
                    Ok(Some(entry)) => {
                        let name = entry.file_name().to_string_lossy().into_owned();
                        let ft = entry.file_type().await;
                        let file_type = match ft {
                            Err(e) => return Err(e.to_string()),
                            Ok(ft) => file_type_of(ft.is_symlink(), ft.is_dir(), ft.is_file()),
                        };
                        entries.push((name, file_type));
                    }
                }
            }
        }
    }
}

/// Mirrors `IFileSystemProvider.readFile`.
#[tauri::command]
pub async fn fs_read_file(path: String) -> Result<Vec<u8>, String> {
    let data = tokio::fs::read(&path).await;
    match data {
        Ok(data) => Ok(data),
        Err(e) => Err(e.to_string()),
    }
}

/// Mirrors `IFileSystemProvider.writeFile` (slice A: direct write;
/// upstream's atomic tmp-file+rename save lands before State C).
#[tauri::command]
pub async fn fs_write_file(path: String, contents: Vec<u8>) -> Result<(), String> {
    let written = tokio::fs::write(&path, contents).await;
    match written {
        Ok(()) => Ok(()),
        Err(e) => Err(e.to_string()),
    }
}

/// Mirrors `IFileService.mkdir` (creates all missing parents).
#[tauri::command]
pub async fn fs_mkdir(path: String) -> Result<(), String> {
    let created = tokio::fs::create_dir_all(&path).await;
    match created {
        Ok(()) => Ok(()),
        Err(e) => Err(e.to_string()),
    }
}

/// Mirrors `IFileSystemProvider.rename` with `IFileOverwriteOptions.overwrite`.
#[tauri::command]
pub async fn fs_rename(from: String, to: String, overwrite: bool) -> Result<(), String> {
    if overwrite {
        let target_exists = tokio::fs::try_exists(&to).await.unwrap_or(false);
        if target_exists {
            fs_delete_impl(&to, true).await?;
        }
    }
    let renamed = tokio::fs::rename(&from, &to).await;
    match renamed {
        Ok(()) => Ok(()),
        Err(e) => Err(e.to_string()),
    }
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
    let md = tokio::fs::symlink_metadata(path).await;
    match md {
        Err(e) => Err(e.to_string()),
        Ok(md) => {
            let removed = if md.is_dir() {
                if recursive {
                    tokio::fs::remove_dir_all(path).await
                } else {
                    tokio::fs::remove_dir(path).await
                }
            } else {
                tokio::fs::remove_file(path).await
            };
            match removed {
                Ok(()) => Ok(()),
                Err(e) => Err(e.to_string()),
            }
        }
    }
}
