//! Window state persistence, hot-exit backups and session restore (Phase 3).
//!
//! Electron electron-main owns three pieces of "state the next boot needs":
//!
//!   1. `windowsState.json` (userDataDir) — every window's last bounds,
//!      maximized/fullscreen state and its workspace/folder, written by
//!      `windowsStateMainService` on window close. On boot,
//!      `windowsMainService` reopens the last active window ("restore the
//!      last session" — the default `window.reopenFolders === 'one'`).
//!   2. `workspaceStorage.json` — which backup folder belongs to which
//!      workspace (the hot-exit registry), so the renderer's BackupTracker
//!      can restore unsaved editors.
//!   3. `Backups/<workspaceId>/` — the actual backup trees the renderer
//!      writes through the localFilesystem channel.
//!
//! The renderer does the heavy lifting itself once the window configuration
//! carries the right `workspace`/`folderUri`/`backupPath` — VS Code's
//! editor-state persistence (opened editors, layout, UI state) lives in the
//! per-workspace storage database (storage channel) and hot exit restores
//! from the backup path. This module is the Mountain-side registry that
//! makes both reachable at boot.

use serde_json::{json, Value};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{LazyLock, Mutex};

/// state file shape (windowsStateMainService IWindowsState parity):
/// { "windows": [ { "windowId", "workspace"?, "folderUri"?,
///                  "backupPath", "uiState": { x,y,width,height,
///                  maximized, fullscreen } } ], "lastActiveWindow": ... }
static LAST_STATE: LazyLock<Mutex<Option<Value>>> = LazyLock::new(|| Mutex::new(None));
/// windowId -> last captured uiState (see update_ui_state).
static LAST_UI_CAPTURED: LazyLock<Mutex<HashMap<i64, Value>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

/// Track the folder/workspace each open window has (updated when the
/// workbench opens things), so closing a window persists it.
static WINDOW_WORKSPACES: LazyLock<Mutex<HashMap<i64, Value>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

// ---------------------------------------------------------------------------
// Init / restore-at-boot
// ---------------------------------------------------------------------------

/// Load `windowsState.json` and remember it for save-on-close. Returns the
/// window config override for the MAIN window when the previous session
/// should be restored (last active window's workspace/folder + backup path).
pub fn init() -> Value {
    let state = read_state();
    let mut override_config = serde_json::Map::new();

    if let Some(last) = last_active_window(&state) {
        // Restore the workspace/folder of the last active window
        // (window.reopenFolders === 'one', the upstream default).
        if let Some(workspace) = last.get("workspace").filter(|w| !w.is_null()) {
            override_config.insert("workspace".to_string(), workspace.clone());
            override_config.insert("folderUri".to_string(), Value::Null);
        } else if let Some(folder) = last.get("folderUri").filter(|f| !f.is_null()) {
            override_config.insert("folderUri".to_string(), folder.clone());
            override_config.insert("workspace".to_string(), Value::Null);
        }
        // Hot exit: point the workbench at the previous backup folder.
        if let Some(backup) = last.get("backupPath").and_then(Value::as_str) {
            if Path::new(backup).is_dir() {
                override_config.insert("backupPath".to_string(), json!(backup));
            }
        }
        // The workbench restores "opened editors" from workspace storage and
        // `filesToOpenOrCreate` is left empty — the renderer's
        // WorkingCopyBackup + Layout restore handle the rest.
    }

    *LAST_STATE.lock().unwrap_or_else(|p| p.into_inner()) = Some(state);
    Value::Object(override_config)
}

/// The set of workspaces with backups on disk (`Backups/workspaces.json`
/// parity) — the renderer's backup path registry, consulted by the
/// hot-exit restore on boot.
pub fn backup_workspaces() -> Value {
    let path = backups_dir().join("workspaces.json");
    std::fs::read_to_string(path)
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_else(|| json!({ "folderWorkspaces": { "folders": {}, "workspaces": {} } }))
}

// ---------------------------------------------------------------------------
// Per-window bookkeeping
// ---------------------------------------------------------------------------

/// Remember which workspace/folder a window currently holds (called from
/// config::apply_window_openables so the state is current at close time).
pub fn track_window_workspace(window_id: i64, config: &Value) {
    let entry = json!({
        "workspace": config.get("workspace").cloned().unwrap_or(Value::Null),
        "folderUri": config.get("folderUri").cloned().unwrap_or(Value::Null),
        "backupPath": config.get("backupPath").cloned().unwrap_or(Value::Null),
    });
    WINDOW_WORKSPACES
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .insert(window_id, entry);
}

/// Compute the backup directory for a window configuration (creating it) —
/// `Backups/<workspaceId or folderId or windowId>`.
pub fn backup_path_for_config(config: &Value) -> Option<String> {
    let id = backup_folder_id(config);
    let dir = backups_dir().join(id);
    std::fs::create_dir_all(&dir).ok()?;
    Some(dir.to_string_lossy().replace('\\', "/"))
}

/// The saved UI state (bounds/maximized/fullscreen) of the last active
/// window — consumed by main.rs when creating the window so the session
/// reopens where it was (windowsStateMainService parity).
pub fn last_window_ui_state() -> Option<Value> {
    let state = read_state();
    last_active_window(&state)
        .and_then(|w| w.get("uiState").cloned())
        .filter(|ui| ui.is_object())
}

/// Capture a window's live bounds (called from the CloseRequested event —
/// the window is still queryable there; Destroyed is too late).
///
/// Electron semantics: when maximized, the RESTORED bounds are what gets
/// persisted, so a maximize->close->reopen still lands on the remembered
/// normal size. We approximate by keeping the last non-maximized bounds.
pub fn update_ui_state(window_id: i64, x: i32, y: i32, width: u32, height: u32, maximized: bool, fullscreen: bool) {
    let mut guard = LAST_UI_CAPTURED.lock().unwrap_or_else(|p| p.into_inner());
    let entry = guard.entry(window_id).or_insert_with(|| {
        json!({ "x": 0, "y": 0, "width": 1280, "height": 800, "maximized": false, "fullscreen": false })
    });
    if let Some(map) = entry.as_object_mut() {
        if !maximized && !fullscreen {
            map.insert("x".to_string(), json!(x));
            map.insert("y".to_string(), json!(y));
            map.insert("width".to_string(), json!(width));
            map.insert("height".to_string(), json!(height));
        }
        map.insert("maximized".to_string(), json!(maximized));
        map.insert("fullscreen".to_string(), json!(fullscreen));
    }
}

/// A window closed: persist its workspace into windowsState.json (the last
/// closed window becomes the restore candidate).
pub fn on_window_closed(label: &str, window_id: i64) {
    let workspaces = WINDOW_WORKSPACES
        .lock()
        .unwrap_or_else(|p| p.into_inner());
    // Fallback chain: the tracked entry (kept current by
    // track_window_workspace), else the window's own configuration — a
    // window always persists its session even when nothing was ever
    // "opened" into it (upstream saves empty windows too).
    let fallback = {
        let config_label = if label.is_empty() { "main" } else { label };
        crate::config::window_config_for(config_label).map(|config| {
            json!({
                "workspace": config.get("workspace").cloned().unwrap_or(Value::Null),
                "folderUri": config.get("folderUri").cloned().unwrap_or(Value::Null),
                "backupPath": config.get("backupPath").cloned().unwrap_or(Value::Null),
            })
        })
    };
    let entry = match workspaces.get(&window_id).cloned().or(fallback) {
        Some(entry) => entry,
        None => {
            let _ = label;
            return;
        }
    };
    let workspace = entry.get("workspace").cloned().unwrap_or(Value::Null);
    let folder = entry.get("folderUri").cloned().unwrap_or(Value::Null);
    let backup = entry.get("backupPath").cloned().unwrap_or(Value::Null);

    let mut state = read_state();
    let windows = state
        .as_object_mut()
        .map(|map| map.entry("windows").or_insert_with(|| json!([])))
        .and_then(Value::as_array_mut);

    if let Some(windows) = windows {
        windows.retain(|w| {
            w.get("windowId").and_then(Value::as_i64) != Some(window_id)
        });
        windows.push(json!({
            "windowId": window_id,
            "workspace": workspace,
            "folderUri": folder,
            "backupPath": backup,
            "uiState": last_ui_state(window_id),
        }));
        if let Some(map) = state.as_object_mut() {
            map.insert("lastActiveWindow".to_string(), json!(window_id));
        }
        write_state(&state);
        *LAST_STATE.lock().unwrap_or_else(|p| p.into_inner()) = Some(state);
    }

    WINDOW_WORKSPACES.lock().unwrap_or_else(|p| p.into_inner()).remove(&window_id);
}

// ---------------------------------------------------------------------------
// helpers
// ---------------------------------------------------------------------------

fn data_root() -> PathBuf {
    crate::config::data_root()
}

fn state_path() -> PathBuf {
    data_root().join("windowsState.json")
}

fn backups_dir() -> PathBuf {
    data_root().join("Backups")
}

/// Stable backup folder name for a window config: the workspace id, the
/// folder fsPath hash, or the plain window id (electron-main
/// toBackupWorkspaceBackupPath parity, simplified).
fn backup_folder_id(config: &Value) -> String {
    if let Some(workspace) = config.get("workspace").and_then(|w| w.get("id")).and_then(Value::as_str) {
        if !workspace.is_empty() {
            return workspace.to_string();
        }
    }
    if let Some(folder) = config.get("folderUri").and_then(|f| f.get("path")).and_then(Value::as_str) {
        if !folder.is_empty() {
            return crate::util::md5_hex(&folder.to_lowercase());
        }
    }
    let window_id = config.get("windowId").and_then(Value::as_i64).unwrap_or(1);
    format!("window{}", window_id)
}

fn last_ui_state(window_id: i64) -> Value {
    // Live-captured bounds (CloseRequested) when available; defaults when
    // the window was killed before a clean close.
    LAST_UI_CAPTURED
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .get(&window_id)
        .cloned()
        .unwrap_or_else(|| {
            json!({ "x": 0, "y": 0, "width": 1280, "height": 800, "maximized": false, "fullscreen": false })
        })
}

fn read_state() -> Value {
    std::fs::read_to_string(state_path())
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_else(|| json!({ "windows": [], "lastActiveWindow": 1 }))
}

fn write_state(state: &Value) {
    let _ = std::fs::write(state_path(), serde_json::to_string_pretty(state).unwrap_or_default());
}

fn last_active_window(state: &Value) -> Option<Value> {
    let last_id = state.get("lastActiveWindow").and_then(Value::as_i64)?;
    let windows = state.get("windows")?.as_array()?;
    let found = windows
        .iter()
        .rev()
        .find(|w| w.get("windowId").and_then(Value::as_i64) == Some(last_id))
        .cloned();
    found.or_else(|| windows.last().cloned())
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ui_state_capture_keeps_normal_bounds_when_maximized() {
        update_ui_state(41, 10, 20, 900, 600, false, false);
        update_ui_state(41, 999, 999, 100, 100, true, false); // maximize: bounds unchanged
        let ui = last_ui_state(41);
        assert_eq!(ui["x"], json!(10));
        assert_eq!(ui["width"], json!(900));
        assert_eq!(ui["maximized"], json!(true));
    }

    #[test]
    fn backup_folder_id_prefers_workspace() {
        let config = json!({
            "windowId": 7,
            "workspace": { "id": "abc123", "configPath": "file:///c:/x.code-workspace" },
            "folderUri": null
        });
        assert_eq!(backup_folder_id(&config), "abc123");
    }

    #[test]
    fn backup_folder_id_falls_back_to_folder_hash() {
        let config = json!({
            "windowId": 8,
            "workspace": null,
            "folderUri": { "path": "/c%3A/Users/Younes/project" }
        });
        assert!(!backup_folder_id(&config).is_empty());
        assert_ne!(backup_folder_id(&config), "8");
    }

    #[test]
    fn backup_folder_id_window_fallback() {
        let config = json!({ "windowId": 9, "workspace": null, "folderUri": null });
        assert_eq!(backup_folder_id(&config), "window9");
    }
}
