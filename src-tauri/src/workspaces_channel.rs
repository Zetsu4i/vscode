//! Mountain: `workspaces` protocol channel (IWorkspacesService).
//!
//! Electron-main exposes the workspace management + history surface over the
//! main-process message protocol (src/vs/platform/workspaces/common/
//! workspaces.ts). The workbench calls it during every boot — the welcome
//! page, the "File > Open Recent" menu and `workbench.editor.limit`
//! bookkeeping all read `getRecentlyOpened` within the first seconds, which
//! is why this channel is one of the noisiest "not registered" rejections
//! in the Phase 1 logs.
//!
//! Implementation notes:
//!   * History persists in `<dataRoot>/recent.json`, storing the exact
//!     `IRecentlyOpened` shape (`workspaces`/`files` arrays) so the file can
//!     round-trip through `getRecentlyOpened` without conversion.
//!   * `getWorkspaceIdentifier`/`createUntitledWorkspace` follow upstream's
//!     id derivation exactly (md5 of the lowercased config path off Linux,
//!     see src/vs/platform/workspaces/node/workspaces.ts), because workspace
//!     ids are persisted by the workbench (backup locations, storage keys).
//!   * The untitled workspaces home matches `untitledWorkspacesHome`
//!     (environmentService: `join(userDataPath, 'Workspaces')`).

use serde_json::{json, Value};
use std::path::PathBuf;

static RECENT_FILE: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();
static WORKSPACES_HOME: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();

/// Called from config.rs init once the data root is known.
pub fn init(user_dir: &std::path::Path) {
    let _ = RECENT_FILE.set(crate::config::data_root().join("recent.json"));
    let home = user_dir.join("Workspaces");
    let _ = std::fs::create_dir_all(&home);
    let _ = WORKSPACES_HOME.set(home);
}

/// Handle one `workspaces` channel request. `arg` is the method's first
/// argument (plain ChannelReceiver semantics — no ProxyChannel context).
pub fn handle(command: &str, arg: &Value) -> Result<Value, String> {
    match command {
        // ---- history ----
        "getRecentlyOpened" => Ok(recently_opened()),
        "addRecentlyOpened" => {
            add_recent(arg.as_array().cloned().unwrap_or_default());
            Ok(Value::Null)
        }
        "removeRecentlyOpened" => {
            remove_recent(arg.as_array().cloned().unwrap_or_default());
            Ok(Value::Null)
        }
        "clearRecentlyOpened" => {
            if let Some(path) = RECENT_FILE.get() {
                let _ = std::fs::remove_file(path);
            }
            Ok(Value::Null)
        }

        // ---- management ----
        "getWorkspaceIdentifier" => Ok(workspace_identifier(arg)),
        "createUntitledWorkspace" => create_untitled_workspace(),
        "deleteUntitledWorkspace" => {
            delete_untitled_workspace(arg);
            Ok(Value::Null)
        }
        "enterWorkspace" => Ok(json!({
            "workspace": workspace_identifier(arg),
            "backupPath": null
        })),

        // ---- dirty workspaces (hot-exit registry) ----
        // IWorkspaceIdentifier[] of every workspace/folder with unsaved
        // backups on disk — windowsMainService consults this on boot to
        // offer restoring workspaces that still hold dirty working copies
        // (the Backups/ tree the renderer writes through localFilesystem).
        "getDirtyWorkspaces" => {
            let registered = crate::window_state::backup_workspaces();
            let mut out: Vec<Value> = Vec::new();
            for key in ["folders", "workspaces"] {
                if let Some(map) = registered.get(key).and_then(Value::as_object) {
                    for (id, uri) in map {
                        // Upstream maps workspace ids to identifiers; the
                        // backup registry already stores the config path
                        // URI components under each id.
                        let _ = id;
                        if let Some(config_path) = uri.get("configPath") {
                            out.push(json!({ "id": id, "configPath": config_path }));
                        } else if uri.get("path").is_some() {
                            // A folder entry: return as a recent-folder
                            // identifier (folderUri shape).
                            out.push(json!({ "id": id, "folderUri": uri }));
                        }
                    }
                }
            }
            Ok(json!(out))
        }

        other => Err(format!("workspaces channel: unknown command {}", other)),
    }
}

// ---------------------------------------------------------------------------
// History persistence
// ---------------------------------------------------------------------------

const MAX_RECENT_ENTRIES: usize = 100;

fn recently_opened() -> Value {
    let Some(path) = RECENT_FILE.get() else {
        return json!({ "workspaces": [], "files": [] });
    };
    let raw = std::fs::read_to_string(path)
        .ok()
        .and_then(|text| serde_json::from_str::<Value>(&text).ok())
        .filter(|value| value.is_object())
        .unwrap_or_else(|| json!({ "workspaces": [], "files": [] }));
    sanitize_recently_opened(&raw)
}

/// Drop malformed entries before they reach the renderer. The menubar and
/// the welcome page dereference `workspace.configPath` / `folderUri` /
/// `fileUri` unconditionally (`createOpenRecentMenuAction`,
/// `filterRecentlyOpened`) — a single entry missing them crashes the whole
/// menu with `Cannot read properties of undefined`. Entries written by
/// older shells (or partial writes) get filtered here.
fn sanitize_recently_opened(raw: &Value) -> Value {
    let valid_workspace = |entry: &Value| {
        entry
            .get("workspace")
            .and_then(|w| w.get("configPath"))
            .map(|p| !p.is_null())
            .unwrap_or(false)
            || entry
                .get("folderUri")
                .map(|f| !f.is_null() && f.get("path").is_some())
                .unwrap_or(false)
    };
    let valid_file = |entry: &Value| {
        entry
            .get("fileUri")
            .map(|f| !f.is_null() && f.get("path").is_some())
            .unwrap_or(false)
    };

    let workspaces = raw
        .get("workspaces")
        .and_then(Value::as_array)
        .map(|entries| {
            entries
                .iter()
                .filter(|entry| entry.is_object() && valid_workspace(entry))
                .take(MAX_RECENT_ENTRIES)
                .cloned()
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let files = raw
        .get("files")
        .and_then(Value::as_array)
        .map(|entries| {
            entries
                .iter()
                .filter(|entry| entry.is_object() && valid_file(entry))
                .take(MAX_RECENT_ENTRIES)
                .cloned()
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    json!({ "workspaces": workspaces, "files": files })
}

fn write_recently_opened(value: &Value) {
    let Some(path) = RECENT_FILE.get() else { return };
    if let Ok(text) = serde_json::to_string_pretty(value) {
        let _ = std::fs::write(path, text);
    }
}

/// `addRecentlyOpened(recents: IRecent[])` — upstream moves each entry to
/// the front (most recently used first) and drops entries beyond the cap.
fn add_recent(recents: Vec<Value>) {
    if recents.is_empty() {
        return;
    }
    let current = recently_opened();
    let files: Vec<Value> = current
        .get("files")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let workspaces: Vec<Value> = current
        .get("workspaces")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();

    let mut new_files: Vec<Value> = Vec::new();
    let mut new_workspaces: Vec<Value> = Vec::new();
    for recent in recents {
        let entry = with_derived_label(recent);
        if entry.get("fileUri").is_some() {
            new_files.push(entry);
        } else {
            new_workspaces.push(entry);
        }
    }

    // MRU order: new entries first, then the old ones that were not just
    // re-added (matched by their identifying URI).
    let mut merged_files = new_files.clone();
    for old in &files {
        let dup = new_files
            .iter()
            .any(|n| same_uri_components(n.get("fileUri"), old.get("fileUri")));
        if !dup {
            merged_files.push(old.clone());
        }
    }
    let mut merged_workspaces = new_workspaces.clone();
    for old in &workspaces {
        let old_key = old
            .get("workspace")
            .and_then(|w| w.get("configPath"))
            .or_else(|| old.get("folderUri"));
        let dup = new_workspaces.iter().any(|n| {
            let n_key = n
                .get("workspace")
                .and_then(|w| w.get("configPath"))
                .or_else(|| n.get("folderUri"));
            same_uri_components(n_key, old_key)
        });
        if !dup {
            merged_workspaces.push(old.clone());
        }
    }
    merged_files.truncate(MAX_RECENT_ENTRIES);
    merged_workspaces.truncate(MAX_RECENT_ENTRIES);

    write_recently_opened(&json!({ "workspaces": merged_workspaces, "files": merged_files }));
}

/// `removeRecentlyOpened(workspaces: URI[])` — matches by config path,
/// folder URI or file URI.
fn remove_recent(uris: Vec<Value>) {
    if uris.is_empty() {
        return;
    }
    let mut current = recently_opened();
    let mut kept_files = Vec::new();
    let mut kept_workspaces = Vec::new();
    if let Some(files) = current.get_mut("files").and_then(Value::as_array_mut) {
        for file in files.drain(..) {
            if !uris.iter().any(|uri| same_uri_components(Some(uri), file.get("fileUri"))) {
                kept_files.push(file);
            }
        }
    }
    if let Some(workspaces) = current.get_mut("workspaces").and_then(Value::as_array_mut) {
        for workspace in workspaces.drain(..) {
            let config = workspace
                .get("workspace")
                .and_then(|w| w.get("configPath").cloned());
            let folder = workspace.get("folderUri").cloned();
            let matches = uris.iter().any(|uri| {
                config.as_ref().map(|c| same_uri_components(Some(uri), Some(c))).unwrap_or(false)
                    || folder.as_ref().map(|f| same_uri_components(Some(uri), Some(f))).unwrap_or(false)
            });
            if !matches {
                kept_workspaces.push(workspace);
            }
        }
    }
    write_recently_opened(&json!({ "workspaces": kept_workspaces, "files": kept_files }));
}

/// Fill `label` from the entry's URI basename when absent — what
/// `getSingleFolderWorkspaceLabel`/`basename` do for the welcome page.
fn with_derived_label(mut entry: Value) -> Value {
    let uri = entry
        .get("workspace")
        .and_then(|w| w.get("configPath"))
        .or_else(|| entry.get("folderUri"))
        .or_else(|| entry.get("fileUri"))
        .cloned();
    let needs_label = entry
        .get("label")
        .map(|label| label.as_str().map(str::is_empty).unwrap_or(true))
        .unwrap_or(true);
    if needs_label {
        if let Some(uri) = uri {
            if let Some(label) = uri_basename(&uri) {
                if let Some(map) = entry.as_object_mut() {
                    map.insert("label".to_string(), json!(label));
                }
            }
        }
    }
    entry
}

fn uri_basename(uri: &Value) -> Option<String> {
    let path = uri.get("path").and_then(Value::as_str)?;
    let decoded = crate::util::percent_decode(path);
    let trimmed = decoded.trim_end_matches('/');
    let base = trimmed.rsplit(['/', '\\']).next()?;
    if base.is_empty() {
        None
    } else {
        Some(base.to_string())
    }
}

/// Structural URI equality (scheme + path + authority) — URI.isEqual
/// semantics for the revived component objects we persist.
fn same_uri_components(a: Option<&Value>, b: Option<&Value>) -> bool {
    let (Some(a), Some(b)) = (a, b) else {
        return false;
    };
    if !a.is_object() || !b.is_object() {
        return a == b;
    }
    for key in ["scheme", "authority", "path"] {
        if a.get(key) != b.get(key) {
            return false;
        }
    }
    true
}

// ---------------------------------------------------------------------------
// Workspace identifiers
// ---------------------------------------------------------------------------

/// Upstream `getWorkspaceIdentifier(uri)` — md5 of the (lowercased on
/// Windows/macOS) config path, plus the config path URI itself.
fn workspace_identifier(uri: &Value) -> Value {
    let fs_path = uri_fs_path(uri);
    let id_input = if cfg!(target_os = "linux") {
        fs_path.clone()
    } else {
        fs_path.to_lowercase()
    };
    json!({
        "id": crate::util::md5_hex(&id_input),
        "configPath": uri,
    })
}

fn uri_fs_path(uri: &Value) -> String {
    let raw = uri.get("path").and_then(Value::as_str).unwrap_or("");
    let decoded = crate::util::percent_decode(raw);
    if cfg!(windows) {
        decoded.trim_start_matches('/').replace('/', "\\")
    } else {
        decoded
    }
}

fn create_untitled_workspace() -> Result<Value, String> {
    let Some(home) = WORKSPACES_HOME.get() else {
        return Err("workspaces: untitled workspaces home not initialized".to_string());
    };
    for n in 1..1000 {
        let candidate = home.join(format!("Untitled-{}.code-workspace", n));
        if candidate.exists() {
            continue;
        }
        let config_path = crate::config::uri_json(&candidate);
        let identifier = workspace_identifier(&config_path);
        let body = json!({ "folders": [], "settings": {} });
        let body = match serde_json::to_string_pretty(&body) {
            Ok(text) => format!("{}\n", text),
            Err(err) => return Err(format!("workspaces: cannot serialize untitled workspace: {}", err)),
        };
        if let Err(err) = std::fs::write(&candidate, body) {
            return Err(format!("workspaces: cannot write untitled workspace {:?}: {}", candidate, err));
        }
        return Ok(identifier);
    }
    Err("workspaces: too many untitled workspaces".to_string())
}

fn delete_untitled_workspace(identifier: &Value) {
    let config_path = identifier.get("configPath").cloned().unwrap_or(Value::Null);
    let fs_path = uri_fs_path(&config_path);
    if fs_path.is_empty() {
        return;
    }
    // Only delete files inside the untitled home (identifier is persisted
    // state and could point anywhere).
    let Some(home) = WORKSPACES_HOME.get() else { return };
    let target = PathBuf::from(&fs_path);
    if let (Ok(canon_target), Ok(canon_home)) =
        (std::fs::canonicalize(&target), std::fs::canonicalize(home))
    {
        if canon_target.starts_with(&canon_home) {
            let _ = std::fs::remove_file(&target);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn uri(path: &str) -> Value {
        json!({ "scheme": "file", "authority": "", "path": path, "query": "", "fragment": "" })
    }

    #[test]
    fn workspace_identifier_matches_upstream_shape() {
        let id = workspace_identifier(&uri("/c%3A/Users/a/W.code-workspace"));
        assert_eq!(id["id"].as_str().unwrap().len(), 32);
        assert_eq!(id["configPath"]["path"].as_str().unwrap(), "/c%3A/Users/a/W.code-workspace");
    }

    #[test]
    fn label_derived_from_basename() {
        let entry = with_derived_label(json!({ "folderUri": uri("/c%3A/Users/a/MyProject") }));
        assert_eq!(entry["label"].as_str().unwrap(), "MyProject");
        let labeled = with_derived_label(json!({ "folderUri": uri("/x/y"), "label": "Custom" }));
        assert_eq!(labeled["label"].as_str().unwrap(), "Custom");
    }

    #[test]
    fn uri_components_equality() {
        assert!(same_uri_components(
            Some(&uri("/a/b")),
            Some(&uri("/a/b"))
        ));
        assert!(!same_uri_components(
            Some(&uri("/a/b")),
            Some(&uri("/a/c"))
        ));
        assert!(!same_uri_components(None, Some(&uri("/a/b"))));
    }

    #[test]
    fn enter_workspace_returns_result_shape() {
        let result = handle("enterWorkspace", &uri("/c%3A/tmp/w.code-workspace")).unwrap();
        assert!(result["workspace"]["id"].is_string());
        assert!(result["backupPath"].is_null());
    }

    #[test]
    fn get_dirty_workspaces_empty() {
        assert_eq!(handle("getDirtyWorkspaces", &Value::Null).unwrap(), json!([]));
    }
}
