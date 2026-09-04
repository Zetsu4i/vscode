//! Generic JSON configuration storage for user & workspace scopes.
//!
//! Mirrors VSCode's layout:
//! - user scope:      `<config_dir>/vstauri/<name>`      (settings.json, keybindings.json, …)
//! - workspace scope: `<root>/.vstauri/<name>`           (same file names, per project)
//!
//! The frontend owns merging & validation; the backend only does durable
//! read/write so both scopes behave identically on Linux and Windows.

use serde_json::Value;
use std::fs;
use std::path::PathBuf;

/// `<config_dir>/vstauri` — created on demand.
fn user_dir() -> Result<PathBuf, String> {
    let base =
        dirs::config_dir().ok_or_else(|| "cannot resolve user config directory".to_string())?;
    let dir = base.join("vstauri");
    fs::create_dir_all(&dir).map_err(|e| format!("cannot create {}: {}", dir.display(), e))?;
    Ok(dir)
}

/// `<root>/.vstauri` — created on demand.
fn workspace_dir(root: &str) -> Result<PathBuf, String> {
    let dir = PathBuf::from(root).join(".vstauri");
    fs::create_dir_all(&dir).map_err(|e| format!("cannot create {}: {}", dir.display(), e))?;
    Ok(dir)
}

fn resolve(scope: &str, root: Option<&str>, name: &str) -> Result<PathBuf, String> {
    // Basic name sanitation — we only ever store flat file names.
    if name.contains('/') || name.contains('\\') || name.contains("..") {
        return Err(format!("invalid config file name '{}'", name));
    }
    match scope {
        "user" => user_dir().map(|d| d.join(name)),
        "workspace" => {
            let root = root.ok_or_else(|| "no folder open".to_string())?;
            workspace_dir(root).map(|d| d.join(name))
        }
        other => Err(format!("unknown config scope '{}'", other)),
    }
}

/// Read a config file. Returns `Ok(None)` when it simply does not exist yet,
/// so callers can fall back to defaults without treating that as an error.
#[tauri::command]
pub fn config_read(
    scope: String,
    root: Option<String>,
    name: String,
) -> Result<Option<Value>, String> {
    let path = resolve(&scope, root.as_deref(), &name)?;
    match fs::read_to_string(&path) {
        Ok(text) => {
            if text.trim().is_empty() {
                return Ok(None);
            }
            serde_json::from_str(&text)
                .map(Some)
                .map_err(|e| format!("invalid JSON in {}: {}", path.display(), e))
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(format!("cannot read {}: {}", path.display(), e)),
    }
}

/// Write a config file (pretty-printed, trailing newline) atomically-ish:
/// serialize first, then write, so a serialization failure never truncates.
#[tauri::command]
pub fn config_write(
    scope: String,
    root: Option<String>,
    name: String,
    value: Value,
) -> Result<(), String> {
    let path = resolve(&scope, root.as_deref(), &name)?;
    let mut text = serde_json::to_string_pretty(&value)
        .map_err(|e| format!("cannot serialize {}: {}", name, e))?;
    text.push('\n');
    fs::write(&path, text).map_err(|e| format!("cannot write {}: {}", path.display(), e))
}

/// The on-disk location of a config file (null when the scope is unavailable,
/// e.g. workspace scope with no folder open) — used by UI to show the path.
#[tauri::command]
pub fn config_path(
    scope: String,
    root: Option<String>,
    name: String,
) -> Result<Option<String>, String> {
    match resolve(&scope, root.as_deref(), &name) {
        Ok(p) => Ok(Some(p.to_string_lossy().to_string())),
        Err(_) => Ok(None),
    }
}
