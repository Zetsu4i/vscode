//! File system service: JSON-RPC backed operations + notify-based watcher.

use base64::Engine;
use notify::Watcher;
use notify_debouncer_full::{new_debouncer, DebounceEventResult, Debouncer, FileIdMap};
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use crate::server::SharedState;
use crate::state::AppState;

// ---------------------------------------------------------------------------
// arg helpers (positional JSON args)
// ---------------------------------------------------------------------------

fn arg_str(args: &[Value], i: usize) -> Result<String, String> {
    args.get(i)
        .and_then(Value::as_str)
        .map(str::to_string)
        .ok_or_else(|| format!("missing string arg #{i}"))
}

fn arg_opt_bool(args: &[Value], i: usize, default: bool) -> bool {
    args.get(i).and_then(Value::as_bool).unwrap_or(default)
}

// ---------------------------------------------------------------------------
// rpc operations
// ---------------------------------------------------------------------------

pub async fn stat(args: &[Value]) -> Result<Value, String> {
    let path = arg_str(args, 0)?;
    tokio::task::spawn_blocking(move || stat_sync(Path::new(&path)))
        .await
        .map_err(|e| e.to_string())?
}

fn stat_sync(path: &Path) -> Result<Value, String> {
    let meta = std::fs::metadata(path).map_err(|e| e.to_string())?;
    let ft = meta.file_type();
    let ftype = if ft.is_dir() {
        "dir"
    } else if ft.is_symlink() {
        "symlink"
    } else {
        "file"
    };
    let mtime = meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);
    let ctime = meta
        .created()
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_millis() as u64)
        .unwrap_or(mtime);
    Ok(serde_json::json!({
        "type": ftype,
        "ctime": ctime,
        "mtime": mtime,
        "size": meta.len(),
        "readonly": meta.permissions().readonly(),
    }))
}

pub async fn mkdir(args: &[Value]) -> Result<Value, String> {
    let path = arg_str(args, 0)?;
    tokio::fs::create_dir_all(&path)
        .await
        .map_err(|e| e.to_string())?;
    Ok(Value::Null)
}

pub async fn readdir(args: &[Value]) -> Result<Value, String> {
    let path = arg_str(args, 0)?;
    let mut read_dir = tokio::fs::read_dir(&path)
        .await
        .map_err(|e| e.to_string())?;
    let mut out: Vec<Value> = Vec::new();
    while let Some(entry) = read_dir.next_entry().await.map_err(|e| e.to_string())? {
        let ft = entry.file_type().await.map_err(|e| e.to_string())?;
        let ftype = if ft.is_dir() {
            "dir"
        } else if ft.is_symlink() {
            "symlink"
        } else {
            "file"
        };
        out.push(serde_json::json!({ "name": entry.file_name().to_string_lossy(), "type": ftype }));
    }
    Ok(Value::Array(out))
}

pub async fn delete(args: &[Value]) -> Result<Value, String> {
    let path = arg_str(args, 0)?;
    let recursive = arg_opt_bool(args, 1, false);
    let use_trash = arg_opt_bool(args, 2, false);
    tokio::task::spawn_blocking(move || {
        let p = Path::new(&path);
        if use_trash {
            // best effort trash; fall back to permanent delete
            if trash::delete(p).is_ok() {
                return Ok(Value::Null);
            }
        }
        if p.is_dir() {
            if recursive {
                std::fs::remove_dir_all(p).map_err(|e| e.to_string())?;
            } else {
                std::fs::remove_dir(p).map_err(|e| e.to_string())?;
            }
        } else {
            std::fs::remove_file(p).map_err(|e| e.to_string())?;
        }
        Ok(Value::Null)
    })
    .await
    .map_err(|e| e.to_string())?
}

pub async fn rename(args: &[Value]) -> Result<Value, String> {
    let from = arg_str(args, 0)?;
    let to = arg_str(args, 1)?;
    let overwrite = arg_opt_bool(args, 2, false);
    tokio::fs::rename(&from, &to).await.map_err(|e| {
        if !overwrite && Path::new(&to).exists() {
            "EEXIST: file already exists".to_string()
        } else {
            e.to_string()
        }
    })?;
    Ok(Value::Null)
}

fn copy_dir_recursive(from: &Path, to: &Path) -> Result<(), String> {
    std::fs::create_dir_all(to).map_err(|e| e.to_string())?;
    for entry in std::fs::read_dir(from).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        let src = entry.path();
        let dst = to.join(entry.file_name());
        if src.is_dir() {
            copy_dir_recursive(&src, &dst)?;
        } else {
            std::fs::copy(&src, &dst).map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}

pub async fn copy(args: &[Value]) -> Result<Value, String> {
    let from = arg_str(args, 0)?;
    let to = arg_str(args, 1)?;
    let overwrite = arg_opt_bool(args, 2, false);
    tokio::task::spawn_blocking(move || {
        if !overwrite && Path::new(&to).exists() {
            return Err("EEXIST: file already exists".to_string());
        }
        let src = Path::new(&from);
        if src.is_dir() {
            copy_dir_recursive(src, Path::new(&to)).map(|_| Value::Null)
        } else {
            std::fs::copy(src, &to)
                .map_err(|e| e.to_string())
                .map(|_| Value::Null)
        }
    })
    .await
    .map_err(|e| e.to_string())?
}

pub async fn read_file(args: &[Value]) -> Result<Value, String> {
    let path = arg_str(args, 0)?;
    let bytes = tokio::fs::read(&path).await.map_err(|e| e.to_string())?;
    let b64 = base64::engine::general_purpose::STANDARD.encode(bytes);
    Ok(Value::String(b64))
}

pub async fn write_file(args: &[Value]) -> Result<Value, String> {
    let path = arg_str(args, 0)?;
    let b64 = arg_str(args, 1)?;
    let create = arg_opt_bool(args, 2, true);
    let overwrite = arg_opt_bool(args, 3, true);
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(b64.as_bytes())
        .map_err(|e| format!("bad base64: {e}"))?;

    let exists = Path::new(&path).exists();
    if exists && !overwrite {
        return Err("EEXIST: file already exists".to_string());
    }
    if !exists && !create {
        return Err("ENOENT: file does not exist".to_string());
    }
    tokio::fs::write(&path, bytes)
        .await
        .map_err(|e| e.to_string())?;
    Ok(Value::Null)
}

// ---------------------------------------------------------------------------
// watcher
// ---------------------------------------------------------------------------

pub struct WatcherEntry {
    // kept alive for as long as the watch exists; dropping it stops the watcher
    pub _debouncer: Debouncer<notify::RecommendedWatcher, FileIdMap>,
}

fn event_kind(kind: &notify::EventKind) -> Option<&'static str> {
    use notify::EventKind;
    match kind {
        EventKind::Create(_) => Some("added"),
        EventKind::Modify(_) => Some("updated"),
        EventKind::Remove(_) => Some("deleted"),
        _ => None,
    }
}

pub async fn watch(state: SharedState, args: &[Value]) -> Result<Value, String> {
    let path = arg_str(args, 0)?;
    let recursive = arg_opt_bool(args, 1, true);
    let _excludes: Vec<String> = args
        .get(2)
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(Value::as_str)
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default();

    if !Path::new(&path).is_dir() {
        return Err(format!("ENOENT: not a directory: {path}"));
    }

    let id = state.next_watch_id();
    let (tx, rx) = std::sync::mpsc::channel::<DebounceEventResult>();

    let mut debouncer = new_debouncer(
        std::time::Duration::from_millis(300),
        None,
        move |events: DebounceEventResult| {
            let _ = tx.send(events);
        },
    )
    .map_err(|e| e.to_string())?;

    debouncer
        .watcher()
        .watch(
            Path::new(&path),
            if recursive {
                notify::RecursiveMode::Recursive
            } else {
                notify::RecursiveMode::NonRecursive
            },
        )
        .map_err(|e| e.to_string())?;

    let root = PathBuf::from(&path);
    let bstate = state.clone();
    std::thread::spawn(move || {
        for events in rx {
            match events {
                Ok(events) => {
                    let mut changes: Vec<Value> = Vec::new();
                    for ev in events {
                        if let Some(kind) = event_kind(&ev.event.kind) {
                            for p in &ev.event.paths {
                                let abs = if p.is_absolute() {
                                    p.clone()
                                } else {
                                    root.join(p)
                                };
                                changes.push(serde_json::json!({
                                    "type": kind,
                                    "path": abs.to_string_lossy(),
                                }));
                            }
                        }
                    }
                    if !changes.is_empty() {
                        bstate.broadcast("fs.change", serde_json::json!({ "changes": changes }));
                    }
                }
                Err(_) => break,
            }
        }
        bstate.broadcast("fs.watch-end", serde_json::json!({ "watchId": id }));
    });

    state
        .watchers
        .lock()
        .map_err(|_| "watcher lock poisoned")?
        .insert(
            id,
            WatcherEntry {
                _debouncer: debouncer,
            },
        );

    Ok(Value::Number(id.into()))
}

pub async fn unwatch(state: &AppState, args: &[Value]) -> Result<Value, String> {
    let id = args
        .first()
        .and_then(Value::as_u64)
        .ok_or_else(|| "missing watch id".to_string())? as u32;
    if let Ok(mut watchers) = state.watchers.lock() {
        watchers.remove(&id); // Drop stops the watcher and its thread
    }
    Ok(Value::Null)
}
