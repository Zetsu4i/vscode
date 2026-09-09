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
//! File watching (Phase 4) is real: each `watch(sessionId, req, resource,
//! opts)` request opens a `notify` watcher (ReadDirectoryChangesW on
//! Windows, inotify on Linux) on its own thread, coalesces events in a
//! 50 ms window (matching upstream's debounce), resolves each path to
//! ADDED/UPDATED/DELETED semantics and delivers `IFileChange[]` payloads
//! on the per-session `fileChange` event — exactly the contract of
//! diskFileSystemProviderServer.ts / diskFileSystemProviderClient.ts:
//!
//!   listen('fileChange', [sessionId]) -> IFileChange[] | string(error)
//!   watch(sessionId: string, req: string, resource, opts) -> void
//!   unwatch(sessionId: string, req: string) -> void
//!
//! `excludes`/`includes` globs are matched relative to the watched folder
//! with a `**`-aware matcher (no regex dependency).
//!
//! Parity note (full trust): Electron's localFilesystem channel deliberately
//! exposes the whole disk to the renderer — that is the desktop contract
//! (`FileService` + `DiskFileSystemProvider`). This implementation matches
//! that contract; no sandboxing is applied beyond rejecting non-`file`
//! schemes, exactly like the Node implementation does.

use notify::event::{EventKind, ModifyKind, RenameMode};
use notify::Watcher;
use serde_json::{json, Value};
use std::fs::{File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::{mpsc::RecvTimeoutError, LazyLock, Mutex};
use std::time::Duration;

// FileType (src/vs/platform/files/common/files.ts)
const FILE_TYPE_UNKNOWN: i64 = 0;
const FILE_TYPE_FILE: i64 = 1;
const FILE_TYPE_DIRECTORY: i64 = 2;
const FILE_TYPE_SYMBOLIC_LINK: i64 = 64;

// FileChangeType (src/vs/platform/files/common/files.ts)
const FILE_CHANGE_UPDATED: i64 = 0;
const FILE_CHANGE_ADDED: i64 = 1;
const FILE_CHANGE_DELETED: i64 = 2;

/// Upstream debounces watcher events before flushing to the renderer;
/// @parcel/watcher uses a 50 ms window, mirrored here.
const WATCH_DEBOUNCE: Duration = Duration::from_millis(50);

static NEXT_FD: AtomicI64 = AtomicI64::new(1000);
static OPEN_FILES: LazyLock<Mutex<std::collections::HashMap<i64, File>>> =
    LazyLock::new(|| Mutex::new(std::collections::HashMap::new()));

/// Active watch requests, keyed by the renderer's `(sessionId, req)` UUID
/// pair (diskFileSystemProviderClient.ts). Dropping the watcher stops the
/// event thread (the mpsc sender dies with it).
static WATCH_REQUESTS: LazyLock<
    Mutex<std::collections::HashMap<(String, String), notify::RecommendedWatcher>>,
> = LazyLock::new(|| Mutex::new(std::collections::HashMap::new()));

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
            // watch(sessionId: string, req: string, resource, opts) -> void
            // (diskFileSystemProviderServer.ts watch command — note the
            // renderer generates both ids itself; the call resolves void).
            let session_id = arg0
                .as_str()
                .ok_or_else(|| "localFilesystem: watch expects a session id string".to_string())?
                .to_string();
            let req = args
                .get(1)
                .and_then(Value::as_str)
                .ok_or_else(|| "localFilesystem: watch expects a request id string".to_string())?
                .to_string();
            let resource = args.get(2).cloned().unwrap_or(Value::Null);
            let opts = args.get(3).cloned().unwrap_or(Value::Null);
            let path = uri_to_path(&resource)?;
            let recursive = opts.get("recursive").and_then(Value::as_bool).unwrap_or(true);
            let excludes = opt_globs(&opts, "excludes");
            let includes = opt_globs(&opts, "includes");

            let watcher = start_watcher(&session_id, &path, recursive, excludes, includes);
            if let Ok(mut guard) = WATCH_REQUESTS.lock() {
                guard.insert((session_id, req), watcher);
            }
            Ok(Value::Null)
        }
        "unwatch" => {
            // unwatch(sessionId: string, req: string) -> void
            let session_id = arg0.as_str().unwrap_or("").to_string();
            let req = args.get(1).and_then(Value::as_str).unwrap_or("").to_string();
            if let Ok(mut guard) = WATCH_REQUESTS.lock() {
                guard.remove(&(session_id, req)); // drop -> watcher + thread stop
            }
            Ok(Value::Null)
        }
        other => Err(format!("localFilesystem channel: call not found: {}", other)),
    }
}

// ---------------------------------------------------------------------------
// File watching (Phase 4 — the `notify` integration)
// ---------------------------------------------------------------------------

/// Read a glob-pattern array (`excludes` / `includes`) from IWatchOptions.
fn opt_globs(opts: &Value, key: &str) -> Vec<String> {
    opts.get(key)
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}

/// Open a notify watcher on `path` and spawn its event thread. The returned
/// watcher must stay alive (WATCH_REQUESTS) or the thread exits.
fn start_watcher(
    session_id: &str,
    path: &Path,
    recursive: bool,
    excludes: Vec<String>,
    includes: Vec<String>,
) -> notify::RecommendedWatcher {
    let (tx, rx) = std::sync::mpsc::channel::<notify::Result<notify::Event>>();
    let mut watcher = notify::recommended_watcher(tx)
        .unwrap_or_else(|err| panic!("cannot create watcher: {}", err));
    let mode = if recursive {
        notify::RecursiveMode::Recursive
    } else {
        notify::RecursiveMode::NonRecursive
    };
    // A missing/unwatchable path is delivered as the string error payload of
    // the fileChange event (upstream: onDidWatchError), not a rejected call.
    if let Err(err) = watcher.watch(path, mode) {
        crate::ipc::fire_event_with_arg(
            "localFilesystem",
            "fileChange",
            &json!([session_id]),
            &json!(format!("VSTauri watcher error on {}: {}", path.display(), err)),
        );
        crate::logger::log_app(
            "warn",
            &format!("localFilesystem: watch {:?} failed: {}", path, err),
        );
    } else {
        crate::logger::log_app(
            "info",
            &format!(
                "localFilesystem: watching {:?} ({} session {})",
                path,
                if recursive { "recursive" } else { "non-recursive" },
                session_id
            ),
        );
    }

    let session_arg = json!([session_id]);
    let root = path.to_path_buf();
    let session = session_id.to_string();
    std::thread::Builder::new()
        .name(format!("vstauri-fswatch-{}", session_id))
        .spawn(move || watch_event_loop(rx, root, excludes, includes, session, session_arg))
        .ok();

    watcher
}

/// Per-path pending state within a debounce window: whether any Create
/// (or rename-To) was seen. Final ADDED/UPDATED/DELETED is resolved by
/// existence at flush time, which reproduces @parcel/watcher semantics
/// on both inotify (Create + Modify bursts) and ReadDirectoryChangesW.
struct PendingChange {
    saw_create: bool,
}

fn watch_event_loop(
    rx: std::sync::mpsc::Receiver<notify::Result<notify::Event>>,
    root: PathBuf,
    excludes: Vec<String>,
    includes: Vec<String>,
    session: String,
    session_arg: Value,
) {
    let mut pending: std::collections::HashMap<PathBuf, PendingChange> =
        std::collections::HashMap::new();
    loop {
        match rx.recv_timeout(WATCH_DEBOUNCE) {
            Ok(Ok(event)) => {
                for (path, change_type) in classify_event(&event) {
                    if !path_starts_with(&path, &root) {
                        continue;
                    }
                    if is_excluded(&root, &path, &excludes, &includes) {
                        continue;
                    }
                    let entry = pending.entry(path).or_insert(PendingChange { saw_create: false });
                    if change_type == FILE_CHANGE_ADDED {
                        entry.saw_create = true;
                    }
                }
            }
            Ok(Err(err)) => {
                // Watcher-level error: upstream surfaces these as the string
                // variant of the fileChange payload.
                crate::ipc::fire_event_with_arg(
                    "localFilesystem",
                    "fileChange",
                    &session_arg,
                    &json!(format!("VSTauri watcher error: {}", err)),
                );
            }
            Err(RecvTimeoutError::Timeout) => {
                flush_pending(&mut pending, &session_arg);
            }
            Err(RecvTimeoutError::Disconnected) => {
                // The watcher was dropped (unwatch / dispose): last flush, exit.
                flush_pending(&mut pending, &session_arg);
                break;
            }
        }
    }
    let _ = session; // kept for thread naming/debug
}

/// Resolve pending paths into `IFileChange[]` and fire the per-session
/// `fileChange` event.
fn flush_pending(
    pending: &mut std::collections::HashMap<PathBuf, PendingChange>,
    session_arg: &Value,
) {
    if pending.is_empty() {
        return;
    }
    let mut events = Vec::with_capacity(pending.len());
    for (path, state) in pending.drain() {
        // Final type by existence: gone -> DELETED; new -> ADDED; else UPDATED.
        let change_type = if !path.exists() {
            FILE_CHANGE_DELETED
        } else if state.saw_create {
            FILE_CHANGE_ADDED
        } else {
            FILE_CHANGE_UPDATED
        };
        events.push(json!({
            "resource": path_to_uri(&path),
            "type": change_type,
        }));
    }
    crate::ipc::fire_event_with_arg(
        "localFilesystem",
        "fileChange",
        session_arg,
        &Value::Array(events),
    );
}

/// Map one notify event to `(path, FileChangeType)` pairs. Access events
/// (open/close/read) are dropped — upstream does not report them either.
fn classify_event(event: &notify::Event) -> Vec<(PathBuf, i64)> {
    let kind = &event.kind;
    match kind {
        EventKind::Create(_) => event.paths.iter().map(|p| (p.clone(), FILE_CHANGE_ADDED)).collect(),
        EventKind::Remove(_) => {
            event.paths.iter().map(|p| (p.clone(), FILE_CHANGE_DELETED)).collect()
        }
        EventKind::Modify(ModifyKind::Name(rename_mode)) => match rename_mode {
            RenameMode::From => {
                event.paths.iter().map(|p| (p.clone(), FILE_CHANGE_DELETED)).collect()
            }
            RenameMode::To => {
                event.paths.iter().map(|p| (p.clone(), FILE_CHANGE_ADDED)).collect()
            }
            RenameMode::Both => {
                // paths = [old, new]: old removed, new added.
                let mut out = Vec::with_capacity(2);
                if let Some(old) = event.paths.first() {
                    out.push((old.clone(), FILE_CHANGE_DELETED));
                }
                if let Some(new) = event.paths.get(1) {
                    out.push((new.clone(), FILE_CHANGE_ADDED));
                }
                out
            }
            _ => event.paths.iter().map(|p| (p.clone(), FILE_CHANGE_UPDATED)).collect(),
        },
        EventKind::Modify(_) => {
            event.paths.iter().map(|p| (p.clone(), FILE_CHANGE_UPDATED)).collect()
        }
        EventKind::Access(_) | EventKind::Other | EventKind::Any => Vec::new(),
    }
}

/// `path.starts_with(root)` that also tolerates prefix mismatch on
/// case-insensitive systems — notify always reports watched-root-prefixed
/// paths, this is just a guard against backend quirks.
fn path_starts_with(path: &Path, root: &Path) -> bool {
    path.starts_with(root)
        || path
            .to_string_lossy()
            .to_lowercase()
            .starts_with(&root.to_string_lossy().to_lowercase())
}

/// Apply `excludes` (skip match) and `includes` (allow-list) to one path.
/// Globs are matched relative to the watched root; absolute patterns fall
/// back to matching the full path.
fn is_excluded(root: &Path, path: &Path, excludes: &[String], includes: &[String]) -> bool {
    if excludes.is_empty() && includes.is_empty() {
        return false;
    }
    let full = path.to_string_lossy().replace('\\', "/");
    let rel = path
        .strip_prefix(root)
        .map(|r| r.to_string_lossy().replace('\\', "/"))
        .unwrap_or_else(|_| full.clone());
    for pattern in excludes {
        let pat = pattern.replace('\\', "/");
        if glob_match(&pat, &rel) || glob_match(&pat, &full) {
            return true;
        }
    }
    if !includes.is_empty() {
        let allowed = includes.iter().any(|pattern| {
            let pat = pattern.replace('\\', "/");
            glob_match(&pat, &rel) || glob_match(&pat, &full)
        });
        if !allowed {
            return true;
        }
    }
    false
}

/// `*` / `?` / `**` glob matcher on `/`-separated paths (no regex crate).
/// A `**` segment matches zero or more whole segments (so `**/x/**` matches
/// the `x` directory itself — required for exclude-based pruning — and
/// everything under it); `*` and `?` stay within one segment. Empty
/// segments and `.` are dropped on both sides, so absolute patterns
/// (`/C:/x/**`) match normalized absolute paths.
fn glob_match(pattern: &str, path: &str) -> bool {
    // Windows callers pass backslash-separated paths/patterns — normalize
    // both sides so matching is separator-agnostic.
    let pattern = pattern.replace('\\', "/");
    let path = path.replace('\\', "/");
    let pattern_segs: Vec<&str> = pattern
        .split('/')
        .filter(|seg| !seg.is_empty() && *seg != ".")
        .collect();
    let path_segs: Vec<&str> = path
        .split('/')
        .filter(|seg| !seg.is_empty() && *seg != ".")
        .collect();
    match_segments(&pattern_segs, &path_segs)
}

/// Segment-level matching with `**` backtracking.
fn match_segments(pattern: &[&str], path: &[&str]) -> bool {
    if pattern.is_empty() {
        return path.is_empty();
    }
    if pattern[0] == "**" {
        // "**" consumes zero or more whole segments.
        for skip in 0..=path.len() {
            if match_segments(&pattern[1..], &path[skip..]) {
                return true;
            }
        }
        return false;
    }
    if path.is_empty() {
        return false;
    }
    if !segment_match(pattern[0].as_bytes(), path[0].as_bytes()) {
        return false;
    }
    match_segments(&pattern[1..], &path[1..])
}

/// In-segment wildcard matching (`*`, `?`); segments never contain
/// separators so `*` may consume any run of characters.
fn segment_match(mut pat: &[u8], mut name: &[u8]) -> bool {
    while !pat.is_empty() {
        match pat[0] {
            b'*' => {
                let rest = &pat[1..];
                let mut i = 0usize;
                while i <= name.len() {
                    if segment_match(rest, &name[i..]) {
                        return true;
                    }
                    i += 1;
                }
                return false;
            }
            b'?' => {
                if name.is_empty() {
                    return false;
                }
                pat = &pat[1..];
                name = &name[1..];
            }
            c => {
                if name.is_empty() || name[0] != c {
                    return false;
                }
                pat = &pat[1..];
                name = &name[1..];
            }
        }
    }
    name.is_empty()
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
    fn watch_emits_added_updated_and_deleted_per_session() {
        // Note: no clear_test_listeners() here — tests run in parallel and
        // share the listener registry; each test uses unique listener ids
        // and filters frames by them.
        let tmp = std::env::temp_dir().join(format!("vstauri-fs-watch-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();

        // Renderer-side session ids (UUID strings upstream; plain strings here).
        let session = "sess-watch-test";
        // Two sessions to prove events are partitioned per listen arg.
        crate::ipc::register_test_listener(11, "localFilesystem", "fileChange", json!([session]));
        crate::ipc::register_test_listener(12, "localFilesystem", "fileChange", json!(["other"]));

        let dir_uri = uri(&tmp.to_string_lossy().replace('\\', "/"));
        handle(
            "watch",
            &json!([session, "req-1", dir_uri, { "recursive": true, "excludes": [] }]),
        )
        .expect("watch");

        // File created -> ADDED (1)
        std::fs::write(tmp.join("created.txt"), b"hello").unwrap();
        // File created then deleted within the window -> DELETED (2)
        std::fs::write(tmp.join("ephemeral.txt"), b"x").unwrap();
        std::fs::remove_file(tmp.join("ephemeral.txt")).unwrap();
        // Existing file modified -> UPDATED (0)
        std::fs::write(tmp.join("modified.txt"), b"1").unwrap();
        std::thread::sleep(Duration::from_millis(120));
        std::fs::write(tmp.join("modified.txt"), b"2").unwrap();

        // Wait past the debounce window.
        std::thread::sleep(Duration::from_millis(400));

        handle("unwatch", &json!([session, "req-1"])).expect("unwatch");

        let frames = crate::ipc::TEST_FRAMES
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let mut saw_created = false;
        let mut saw_deleted = false;
        let mut saw_updated = false;
        let marker = "vstauri-fs-watch";
        for (id, payload) in frames.iter() {
            if *id == 12 {
                // The "other" session must never see this session's files.
                assert!(
                    !payload.to_string().contains(marker),
                    "events leaked across sessions: {}",
                    payload
                );
            }
            if *id != 11 {
                continue; // frames from tests running in parallel
            }
            let Some(events) = payload.as_array() else { continue };
            for event in events {
                let path = event["resource"]["path"].as_str().unwrap_or_default();
                let change_type = event["type"].as_i64().unwrap_or(-1);
                if path.ends_with("created.txt") && change_type == FILE_CHANGE_ADDED {
                    saw_created = true;
                }
                if path.ends_with("ephemeral.txt") && change_type == FILE_CHANGE_DELETED {
                    saw_deleted = true;
                }
                if path.ends_with("modified.txt") && change_type == FILE_CHANGE_UPDATED {
                    saw_updated = true;
                }
            }
        }
        assert!(saw_created, "ADDED for created.txt missing: {:?}", *frames);
        assert!(saw_deleted, "DELETED for ephemeral.txt missing: {:?}", *frames);
        assert!(saw_updated, "UPDATED for modified.txt missing: {:?}", *frames);
        drop(frames);
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn watch_excludes_are_glob_matched() {
        let tmp = std::env::temp_dir().join(format!("vstauri-fs-excl-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(tmp.join("node_modules/pkg")).unwrap();

        let session = "sess-excl";
        crate::ipc::register_test_listener(21, "localFilesystem", "fileChange", json!([session]));
        let dir_uri = uri(&tmp.to_string_lossy().replace('\\', "/"));
        handle(
            "watch",
            &json!([session, "req-1", dir_uri, {
                "recursive": true,
                "excludes": ["**/node_modules/**"]
            }]),
        )
        .expect("watch");

        std::fs::write(tmp.join("node_modules/pkg/junk.js"), b"").unwrap();
        std::fs::write(tmp.join("visible.js"), b"").unwrap();
        std::thread::sleep(Duration::from_millis(400));
        handle("unwatch", &json!([session, "req-1"])).expect("unwatch");

        let frames = crate::ipc::TEST_FRAMES
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let mut all_paths = String::new();
        for (id, payload) in frames.iter() {
            if *id != 21 {
                continue; // frames from tests running in parallel
            }
            all_paths.push_str(&payload.to_string());
        }
        assert!(!all_paths.contains("junk.js"), "excluded file leaked: {}", all_paths);
        assert!(all_paths.contains("visible.js"), "visible file missing: {}", all_paths);
        drop(frames);
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn glob_matcher_covers_watcher_patterns() {
        assert!(glob_match("**/node_modules/**", "node_modules"));
        assert!(glob_match("**/node_modules/**", "node_modules/foo"));
        assert!(glob_match("**/node_modules/**", "a/b/node_modules/c/d.js"));
        assert!(!glob_match("**/node_modules/**", "a/b/c.js"));
        assert!(glob_match("**/.git/objects/**", ".git/objects/pack/x.pack"));
        assert!(glob_match("**/*.log", "debug.log"));
        assert!(glob_match("**/*.log", "nested/deeper/error.log"));
        assert!(!glob_match("**/*.log", "nested/deeper/error.txt"));
        assert!(glob_match("*.log", "debug.log"));
        assert!(!glob_match("*.log", "nested/debug.log"));
        assert!(glob_match("src/**", "src"));
        assert!(glob_match("src/**", "src/main.rs"));
        assert!(!glob_match("src/**", "lib/main.rs"));
        assert!(glob_match("a?c.js", "abc.js"));
        assert!(!glob_match("a?c.js", "abcd.js"));
        assert!(glob_match("**", "any/thing/at/all"));
        assert!(glob_match("exact.txt", "exact.txt"));
        assert!(!glob_match("exact.txt", "other.txt"));
        // Windows separators are normalized before matching.
        assert!(glob_match("**/node_modules/**", "node_modules\\foo"));
        // '**/x' also matches a bare 'x' (leading zero segments).
        assert!(glob_match("**/dist", "dist"));
        assert!(glob_match("**/dist", "build/dist"));
        assert!(!glob_match("**/dist", "build/dist/x"));
        // Absolute patterns (leading slash) match absolute paths.
        assert!(glob_match("/C:/proj/**", "/C:/proj/src/main.rs"));
        assert!(!glob_match("/C:/proj/**", "/C:/other/main.rs"));
    }
}
