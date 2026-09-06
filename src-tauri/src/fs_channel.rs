//! Mountain: `localFilesystem` protocol channel.
//!
//! Implements the `DiskFileSystemProviderChannel` command surface
//! (src/vs/platform/files/node/diskFileSystemProviderServer.ts, registered
//! under the constant name `localFilesystem`) natively in Rust. This is the
//! renderer FileService's disk backend: user settings, keybindings, workspace
//! state, extension metadata — every file the workbench touches flows here.
//!
//! Commands (arg = argument array, resources as UriComponents):
//!   stat(resource)                     -> { type, ctime, mtime, size }
//!   readdir(resource)                  -> [[name, FileType]]
//!   readFile(resource, atomicOptions?) -> VSBuffer          (binary tag 3)
//!   writeFile(resource, VSBuffer, opts)-> void
//!   mkdir(resource)                    -> void
//!   delete(resource, opts)             -> void
//!   rename(from, to, opts)             -> void
//!   copy(from, to, opts)               -> void
//!   cloneFile(from, to)                -> void
//!   realpath(resource)                 -> UriComponents
//!   open(resource, opts) / read / write / close  -> fd-based stream IO
//!   watch(session, resource, opts) / unwatch     -> watcher sessions
//!
//! Watchers are registered (stable session ids, correct unwatch) but do not
//! emit `fileChange` events yet — the `notify`-crate integration is Phase 4
//! (ROADMAP.md). Until then the behavior matches "no external changes
//! detected", which is indistinguishable for a freshly-created data dir.
//!
//! Parity note (full trust): Electron's localFilesystem channel deliberately
//! exposes the whole disk to the renderer — that is the desktop contract
//! (`FileService` + `DiskFileSystemProvider`). This implementation matches
//! that contract; no sandboxing is applied beyond rejecting non-`file`
//! schemes, exactly like the Node implementation does.

use serde_json::{json, Value};
use std::fs::{File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::{LazyLock, Mutex};

// FileType (src/vs/platform/files/common/files.ts)
const FILE_TYPE_UNKNOWN: i64 = 0;
const FILE_TYPE_FILE: i64 = 1;
const FILE_TYPE_DIRECTORY: i64 = 2;
const FILE_TYPE_SYMBOLIC_LINK: i64 = 64;

static NEXT_FD: AtomicI64 = AtomicI64::new(1000);
static OPEN_FILES: LazyLock<Mutex<std::collections::HashMap<i64, File>>> =
    LazyLock::new(|| Mutex::new(std::collections::HashMap::new()));
static WATCH_SESSIONS: LazyLock<Mutex<std::collections::HashMap<i64, String>>> =
    LazyLock::new(|| Mutex::new(std::collections::HashMap::new()));

/// Handle one `localFilesystem` channel request.
pub fn handle(command: &str, arg: &Value) -> Result<Value, String> {
    let args = arg.as_array().cloned().unwrap_or_default();
    let arg0 = args.first().cloned().unwrap_or(Value::Null);

    match command {
        "stat" => {
            let path = uri_to_path(&arg0)?;
            let meta = std::fs::symlink_metadata(&path)
                .map_err(|err| fs_error(&path, err))?;
            Ok(json!({
                "type": file_type_of(&meta),
                "ctime": systemtime_ms(meta.created().ok()),
                "mtime": systemtime_ms(meta.modified().ok()),
                "size": meta.len(),
            }))
        }
        "readdir" => {
            let path = uri_to_path(&arg0)?;
            let entries = std::fs::read_dir(&path)
                .map_err(|err| fs_error(&path, err))?;
            let mut out = Vec::new();
            for entry in entries {
                let entry = entry.map_err(|err| fs_error(&path, err))?;
                let file_type = entry.file_type().ok();
                let ty = match file_type {
                    Some(ft) if ft.is_dir() => FILE_TYPE_DIRECTORY,
                    Some(ft) if ft.is_symlink() => FILE_TYPE_SYMBOLIC_LINK,
                    _ => FILE_TYPE_FILE,
                };
                out.push(json!([entry.file_name().to_string_lossy(), ty]));
            }
            Ok(Value::Array(out))
        }
        "readFile" => {
            let path = uri_to_path(&arg0)?;
            let bytes = std::fs::read(&path).map_err(|err| fs_error(&path, err))?;
            Ok(crate::ipc::vsbuffer(&bytes))
        }
        "writeFile" => {
            let path = uri_to_path(&arg0)?;
            let content = args
                .get(1)
                .and_then(crate::ipc::vsbuffer_bytes)
                .ok_or_else(|| "localFilesystem: writeFile expects a VSBuffer".to_string())?;
            if let Some(parent) = path.parent() {
                if args.get(2).and_then(|o| o.get("create")).and_then(Value::as_bool).unwrap_or(true) && !parent.as_os_str().is_empty() {
                    std::fs::create_dir_all(parent)
                        .map_err(|err| fs_error(parent, err))?;
                }
            }
            // Electron writes atomically (temp file + rename) when unlocked.
            let tmp = path.with_extension("vstauri.tmp");
            std::fs::write(&tmp, &content).map_err(|err| fs_error(&tmp, err))?;
            std::fs::rename(&tmp, &path).map_err(|err| fs_error(&path, err))?;
            Ok(Value::Null)
        }
        "mkdir" => {
            let path = uri_to_path(&arg0)?;
            std::fs::create_dir_all(&path).map_err(|err| fs_error(&path, err))?;
            Ok(Value::Null)
        }
        "delete" => {
            let path = uri_to_path(&arg0)?;
            let options = args.get(1).cloned().unwrap_or(Value::Null);
            let recursive = options.get("recursive").and_then(Value::as_bool).unwrap_or(false);
            let use_trash = options.get("useTrash").and_then(Value::as_bool).unwrap_or(false);
            if use_trash {
                return Err("localFilesystem: trash delete not implemented in the shell yet".to_string());
            }
            let meta = std::fs::symlink_metadata(&path)
                .map_err(|err| fs_error(&path, err))?;
            if meta.is_dir() && !meta.is_symlink() {
                if recursive {
                    std::fs::remove_dir_all(&path).map_err(|err| fs_error(&path, err))?;
                } else {
                    std::fs::remove_dir(&path).map_err(|err| fs_error(&path, err))?;
                }
            } else {
                std::fs::remove_file(&path).map_err(|err| fs_error(&path, err))?;
            }
            Ok(Value::Null)
        }
        "rename" => {
            let from = uri_to_path(&arg0)?;
            let to = uri_to_path(args.get(1).unwrap_or(&Value::Null))?;
            let options = args.get(2).cloned().unwrap_or(Value::Null);
            if options.get("create").and_then(Value::as_bool).unwrap_or(true) {
                if let Some(parent) = to.parent() {
                    if !parent.as_os_str().is_empty() {
                        let _ = std::fs::create_dir_all(parent);
                    }
                }
            }
            if options.get("overwrite").and_then(Value::as_bool).unwrap_or(false) && to.exists() {
                if to.is_dir() {
                    std::fs::remove_dir_all(&to).map_err(|err| fs_error(&to, err))?;
                } else {
                    std::fs::remove_file(&to).map_err(|err| fs_error(&to, err))?;
                }
            }
            std::fs::rename(&from, &to).map_err(|err| fs_error(&from, err))?;
            Ok(Value::Null)
        }
        "copy" => {
            let from = uri_to_path(&arg0)?;
            let to = uri_to_path(args.get(1).unwrap_or(&Value::Null))?;
            let options = args.get(2).cloned().unwrap_or(Value::Null);
            let overwrite = options.get("overwrite").and_then(Value::as_bool).unwrap_or(false);
            let meta = std::fs::symlink_metadata(&from)
                .map_err(|err| fs_error(&from, err))?;
            if to.exists() && !overwrite {
                return Err("localFilesystem: target exists and overwrite is false".to_string());
            }
            if let Some(parent) = to.parent() {
                if !parent.as_os_str().is_empty() {
                    let _ = std::fs::create_dir_all(parent);
                }
            }
            if meta.is_dir() && !meta.is_symlink() {
                copy_dir_recursive(&from, &to)?;
            } else {
                std::fs::copy(&from, &to).map_err(|err| fs_error(&from, err))?;
            }
            Ok(Value::Null)
        }
        "cloneFile" => {
            // On the same filesystem a clone is a copy; hard-linking would
            // diverge from COW semantics on refs, so plain copy.
            let from = uri_to_path(&arg0)?;
            let to = uri_to_path(args.get(1).unwrap_or(&Value::Null))?;
            std::fs::copy(&from, &to).map_err(|err| fs_error(&from, err))?;
            Ok(Value::Null)
        }
        "realpath" => {
            let path = uri_to_path(&arg0)?;
            let real = std::fs::canonicalize(&path)
                .map_err(|err| fs_error(&path, err))?;
            Ok(path_to_uri(&real))
        }
        "open" => {
            let path = uri_to_path(&arg0)?;
            let options = args.get(1).cloned().unwrap_or(Value::Null);
            let flag = |name: &str, default: bool| {
                options.get(name).and_then(Value::as_bool).unwrap_or(default)
            };
            let create = flag("create", false);
            let append = flag("append", false);
            let truncate = flag("truncate", false);
            let read = flag("read", true);
            let write = flag("write", true);
            let mut open = OpenOptions::new();
            open.read(read).write(write).append(append).truncate(truncate);
            if create {
                open.create(true);
            }
            let file = open.open(&path).map_err(|err| fs_error(&path, err))?;
            let fd = NEXT_FD.fetch_add(1, Ordering::Relaxed);
            if let Ok(mut guard) = OPEN_FILES.lock() {
                guard.insert(fd, file);
            }
            Ok(json!(fd))
        }
        "close" => {
            let fd = arg0.as_i64().unwrap_or(-1);
            if let Ok(mut guard) = OPEN_FILES.lock() {
                guard.remove(&fd);
            }
            Ok(Value::Null)
        }
        "read" => {
            // read(fd, pos, length) -> VSBuffer; pos < 0 means "current".
            let fd = arg0.as_i64().unwrap_or(-1);
            let pos = args.get(1).and_then(Value::as_i64).unwrap_or(-1);
            let length = args.get(2).and_then(Value::as_i64).unwrap_or(0).max(0) as usize;
            let bytes = {
                let mut guard = OPEN_FILES.lock().map_err(|_| "fs lock poisoned".to_string())?;
                let file = guard
                    .get_mut(&fd)
                    .ok_or_else(|| format!("localFilesystem: invalid file descriptor {}", fd))?;
                let mut buffer = vec![0u8; length];
                if pos >= 0 {
                    file.seek(SeekFrom::Start(pos as u64))
                        .map_err(|err| err.to_string())?;
                }
                let mut read = 0usize;
                while read < length {
                    match file.read(&mut buffer[read..]) {
                        Ok(0) => break,
                        Ok(n) => read += n,
                        Err(err) => return Err(err.to_string()),
                    }
                }
                buffer.truncate(read);
                buffer
            };
            Ok(crate::ipc::vsbuffer(&bytes))
        }
        "write" => {
            // write(fd, pos, VSBuffer, length, append) -> number written
            let fd = arg0.as_i64().unwrap_or(-1);
            let pos = args.get(1).and_then(Value::as_i64).unwrap_or(-1);
            let data = args
                .get(2)
                .and_then(crate::ipc::vsbuffer_bytes)
                .ok_or_else(|| "localFilesystem: write expects a VSBuffer".to_string())?;
            let written = {
                let mut guard = OPEN_FILES.lock().map_err(|_| "fs lock poisoned".to_string())?;
                let file = guard
                    .get_mut(&fd)
                    .ok_or_else(|| format!("localFilesystem: invalid file descriptor {}", fd))?;
                if pos >= 0 {
                    file.seek(SeekFrom::Start(pos as u64))
                        .map_err(|err| err.to_string())?;
                }
                file.write_all(&data).map_err(|err| err.to_string())?;
                data.len()
            };
            Ok(json!(written as i64))
        }
        "watch" => {
            // watch(sessionKey: string, recursive: number, resource, opts)
            let resource = args.get(2).cloned().unwrap_or(Value::Null);
            let path = uri_to_path(&resource)?;
            let session = NEXT_FD.fetch_add(1, Ordering::Relaxed);
            if let Ok(mut guard) = WATCH_SESSIONS.lock() {
                guard.insert(session, path.to_string_lossy().to_string());
            }
            crate::logger::log_app(
                "info",
                &format!("localFilesystem: watch session {} on {:?} (notify integration: Phase 4)", session, path),
            );
            Ok(json!(session))
        }
        "unwatch" => {
            let session = args.get(1).and_then(Value::as_i64).unwrap_or(-1);
            if let Ok(mut guard) = WATCH_SESSIONS.lock() {
                guard.remove(&session);
            }
            Ok(Value::Null)
        }
        other => Err(format!("localFilesystem channel: call not found: {}", other)),
    }
}

// ---------------------------------------------------------------------------
// URI <-> path conversion (URI.file / fsPath semantics)
// ---------------------------------------------------------------------------

fn uri_to_path(uri: &Value) -> Result<PathBuf, String> {
    let scheme = uri.get("scheme").and_then(Value::as_str).unwrap_or("");
    if scheme != "file" {
        return Err(format!(
            "localFilesystem: scheme '{}' is not a file scheme",
            scheme
        ));
    }
    // fsPath semantics: the URI `path` component is percent-decoded; on
    // Windows a leading drive shape ("/C:/x" or "C:/x") becomes "C:\x".
    let raw_path = uri.get("path").and_then(Value::as_str).unwrap_or("");
    let decoded = crate::util::percent_decode(raw_path);
    let mut normalized = decoded.replace('\\', "/");
    if normalized.starts_with('/') {
        normalized = normalized.trim_start_matches('/').to_string();
    }
    // Windows drive letter: keep "C:/..." shape; convert to OS separators.
    let os_path = if cfg!(windows) {
        normalized.replace('/', "\\")
    } else {
        format!("/{}", normalized)
    };
    Ok(PathBuf::from(os_path))
}

fn path_to_uri(path: &Path) -> Value {
    let mut path_str = path.to_string_lossy().replace('\\', "/");
    if cfg!(windows) {
        // fsPath -> URI.file -> "/C:/x"
        path_str = format!("/{}", path_str);
    } else if !path_str.starts_with('/') {
        path_str = format!("/{}", path_str);
    }
    json!({
        "scheme": "file",
        "authority": "",
        "path": crate::util::encode_uri_path(&path_str),
        "query": "",
        "fragment": "",
    })
}

// ---------------------------------------------------------------------------
// helpers
// ---------------------------------------------------------------------------

fn file_type_of(meta: &std::fs::Metadata) -> i64 {
    let meta = meta;
    if meta.file_type().is_symlink() {
        FILE_TYPE_SYMBOLIC_LINK
    } else if meta.is_dir() {
        FILE_TYPE_DIRECTORY
    } else if meta.is_file() {
        FILE_TYPE_FILE
    } else {
        FILE_TYPE_UNKNOWN
    }
}

fn systemtime_ms(time: Option<std::time::SystemTime>) -> i64 {
    time.map(|t| {
        let d = t
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default();
        (d.as_secs() as i64) * 1000 + i64::from(d.subsec_millis())
    })
    .unwrap_or(0)
}

fn fs_error(path: &Path, err: std::io::Error) -> String {
    // FileSystemProviderError shape: message + code name. The renderer maps
    // known codes (FileNotFound etc.) to friendly behavior.
    let code = match err.kind() {
        std::io::ErrorKind::NotFound => "FileNotFound",
        std::io::ErrorKind::PermissionDenied => "NoPermissions",
        std::io::ErrorKind::AlreadyExists => "FileExists",
        _ => "Unknown",
    };
    format!(
        "FileSystemError ({}) for '{}': {} [localFilesystem]",
        code,
        path.to_string_lossy(),
        err
    )
}

fn copy_dir_recursive(from: &Path, to: &Path) -> Result<(), String> {
    std::fs::create_dir_all(to).map_err(|err| fs_error(to, err))?;
    for entry in std::fs::read_dir(from).map_err(|err| fs_error(from, err))? {
        let entry = entry.map_err(|err| fs_error(from, err))?;
        let target = to.join(entry.file_name());
        let file_type = entry.file_type().map_err(|err| fs_error(from, err))?;
        if file_type.is_dir() {
            copy_dir_recursive(&entry.path(), &target)?;
        } else {
            std::fs::copy(entry.path(), &target).map_err(|err| fs_error(&target, err))?;
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn uri(path: &str) -> Value {
        json!({ "scheme": "file", "authority": "", "path": path })
    }

    #[test]
    fn uri_round_trips_through_the_os_path() {
        let p = uri_to_path(&uri("/C:/Users/z/settings.json")).expect("uri");
        assert!(p.to_string_lossy().contains("settings.json"));
        let back = path_to_uri(&p);
        assert_eq!(back.get("scheme").and_then(Value::as_str), Some("file"));
        let round = uri_to_path(&back).expect("back");
        assert_eq!(p, round);
    }

    #[test]
    fn non_file_schemes_reject() {
        let err = uri_to_path(&json!({ "scheme": "vscode-userdata", "path": "/x" }));
        assert!(err.is_err());
    }

    #[test]
    fn stat_readdir_write_read_round_trip() {
        let tmp = std::env::temp_dir().join(format!("vstauri-fs-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();

        let file_uri = uri(&format!(
            "{}/nested/settings.json",
            tmp.to_string_lossy().replace('\\', "/")
        ));

        // writeFile with a binary payload
        let content = b"{ } // hello";
        handle("writeFile", &json!([file_uri, crate::ipc::vsbuffer(content), { "create": true, "overwrite": true }]))
            .expect("writeFile");

        // readFile returns the bytes
        let read = handle("readFile", &json!([file_uri])).expect("readFile");
        assert_eq!(
            crate::ipc::vsbuffer_bytes(&read).expect("bytes"),
            content.to_vec()
        );

        // stat reports a File with the right size
        let stat = handle("stat", &json!([file_uri])).expect("stat");
        assert_eq!(stat.get("type").and_then(Value::as_i64), Some(FILE_TYPE_FILE));
        assert_eq!(stat.get("size").and_then(Value::as_i64), Some(content.len() as i64));

        // readdir of the parent sees the directory
        let parent = uri(&tmp.to_string_lossy().replace('\\', "/"));
        let entries = handle("readdir", &json!([parent])).expect("readdir");
        let names: Vec<&str> = entries
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|e| e.as_array().and_then(|a| a.first()).and_then(Value::as_str))
            .collect();
        assert!(names.contains(&"nested"));

        // delete recursive
        handle("delete", &json!([parent, { "recursive": true }])).expect("delete");
        assert!(!tmp.exists());

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn stat_missing_file_reports_filenotfound_shape() {
        let err = handle("stat", &json!([uri("/Z:/definitely/not/here.json")]))
            .expect_err("stat must fail");
        assert!(err.contains("FileNotFound"), "got: {}", err);
    }

    #[test]
    fn open_read_write_fd_cycle() {
        let tmp = std::env::temp_dir().join(format!("vstauri-fs-fd-{}.bin", std::process::id()));
        let _ = std::fs::remove_file(&tmp);
        let file_uri = uri(&tmp.to_string_lossy().replace('\\', "/"));

        let fd = handle("open", &json!([file_uri, { "create": true }]))
            .expect("open")
            .as_i64()
            .unwrap();
        assert!(fd >= 0);

        handle("write", &json!([fd, 0, crate::ipc::vsbuffer(b"0123456789"), 10, false])).expect("write");

        let read = handle("read", &json!([fd, 2, 4])).expect("read");
        assert_eq!(crate::ipc::vsbuffer_bytes(&read).unwrap(), b"2345".to_vec());

        handle("close", &json!([fd])).expect("close");
        // After close the fd is invalid.
        assert!(handle("read", &json!([fd, 0, 1])).is_err());

        let _ = std::fs::remove_file(&tmp);
    }

    #[test]
    fn watch_sessions_get_ids_and_unwatch() {
        let tmp = std::env::temp_dir().join(format!("vstauri-fs-watch-{}", std::process::id()));
        std::fs::create_dir_all(&tmp).unwrap();
        let session = handle(
            "watch",
            &json!(["", 1, uri(&tmp.to_string_lossy().replace('\\', "/")), { "recursive": true }]),
        )
        .expect("watch")
        .as_i64()
        .unwrap();
        handle("unwatch", &json!(["", session])).expect("unwatch");
        assert!(WATCH_SESSIONS.lock().unwrap().get(&session).is_none());
        let _ = std::fs::remove_dir_all(&tmp);
    }
}
