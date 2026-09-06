//! Mountain: `storage` protocol channel.
//!
//! Implements the `StorageDatabaseChannel` command surface
//! (src/vs/platform/storage/electron-main/storageIpc.ts) natively in Rust:
//!
//!   getItems  -> Item[] ([key, value] pairs)
//!   getValue  -> string | undefined
//!   updateItems (insert: [[k,v]], delete: [k])
//!   compareAndSwap -> { swapped: boolean, currentValue: string | undefined }
//!   optimize  -> void (the JSON store is rewritten on every update)
//!   isUsed    -> boolean (is the given state path owned by a workspace)
//!   getFallbackApplicationStorageItems -> Item[]
//!   event: onDidChangeStorage ({ changed?, deleted? })
//!
//! Arg shape (ISerializableRequest, an OBJECT — not an array):
//!   { profile?: UriDto<IUserDataProfile>, workspace?: IAnyWorkspaceIdentifier,
//!     applicationShared?: boolean, insert?: [[k,v]], delete?: [k], ... }
//!
//! Storage layout under the data root (this shell owns the layout — fresh
//! data dir, no migration needed — only the wire protocol must match):
//!   application            User/globalStorage/storage.json
//!   application-shared     User/globalStorage/storage.shared.json
//!   default profile        User/globalStorage/storage.json (same as application)
//!   named profile          User/profiles/<id>/globalStorage/storage.json
//!   workspace              User/workspaceStorage/<hash>/state.json
//!
//! Every write is atomic (temp file + rename) so a crash can never corrupt
//! state — the property SQLite gives Electron's storage for free.

use serde_json::{json, Map, Value};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{LazyLock, Mutex, OnceLock};

static USER_DIR: OnceLock<PathBuf> = OnceLock::new();
static STORES: LazyLock<Mutex<BTreeMap<String, BTreeMap<String, String>>>> =
    LazyLock::new(|| Mutex::new(BTreeMap::new()));

/// Called from config::build once the data root exists.
pub fn init(user_dir: &Path) {
    let _ = USER_DIR.set(user_dir.to_path_buf());
}

/// Handle one `storage` channel request. `arg` is the ISerializableRequest
/// object; `command` is the channel command name.
pub fn handle(command: &str, arg: &Value) -> Result<Value, String> {
    let scope_key = scope_key(arg);

    match command {
        "getItems" => {
            let store = load_scope(&scope_key);
            Ok(json!(store
                .iter()
                .map(|(k, v)| json!([k, v]))
                .collect::<Vec<Value>>()))
        }
        "getValue" => {
            let key = arg.get("key").and_then(Value::as_str).unwrap_or("");
            let store = load_scope(&scope_key);
            Ok(match store.get(key) {
                Some(value) => json!(value),
                None => Value::Null,
            })
        }
        "updateItems" => {
            let mut stores = STORES.lock().map_err(|_| "storage lock poisoned".to_string())?;
            let store = stores.entry(scope_key.clone()).or_default();
            let mut changed: Vec<(String, String)> = Vec::new();
            let mut deleted: Vec<String> = Vec::new();

            if let Some(insert) = arg.get("insert").and_then(Value::as_array) {
                for pair in insert {
                    if let (Some(key), Some(value)) = (
                        pair.get(0).and_then(Value::as_str),
                        pair.get(1).and_then(Value::as_str),
                    ) {
                        store.insert(key.to_string(), value.to_string());
                        changed.push((key.to_string(), value.to_string()));
                    }
                }
            }
            if let Some(delete) = arg.get("delete").and_then(Value::as_array) {
                for key in delete {
                    if let Some(key) = key.as_str() {
                        if store.remove(key).is_some() {
                            deleted.push(key.to_string());
                        }
                    }
                }
            }

            persist_scope(&scope_key, &store)?;
            drop(stores);

            // Fire the onDidChangeStorage event to registered renderer
            // listeners (same debounced shape Electron delivers).
            if !changed.is_empty() || !deleted.is_empty() {
                let mut event = Map::new();
                if !changed.is_empty() {
                    event.insert(
                        "changed".to_string(),
                        json!(changed.iter().map(|(k, v)| json!([k, v])).collect::<Vec<Value>>()),
                    );
                }
                if !deleted.is_empty() {
                    event.insert("deleted".to_string(), json!(deleted));
                }
                crate::ipc::fire_event("storage", "onDidChangeStorage", &Value::Object(event));
            }
            Ok(Value::Null)
        }
        "compareAndSwap" => {
            let key = arg.get("key").and_then(Value::as_str).unwrap_or("");
            let expected = arg.get("expectedValue").and_then(Value::as_str);
            let new_value = arg.get("newValue").and_then(Value::as_str);

            let mut stores = STORES.lock().map_err(|_| "storage lock poisoned".to_string())?;
            let store = stores.entry(scope_key.clone()).or_default();
            let current = store.get(key).cloned();
            let swapped = current.as_deref() == expected;
            if swapped {
                if let Some(new_value) = new_value {
                    store.insert(key.to_string(), new_value.to_string());
                } else {
                    store.remove(key);
                }
                persist_scope(&scope_key, &store)?;
            }
            Ok(json!({
                "swapped": swapped,
                "currentValue": current,
            }))
        }
        "optimize" => {
            // The JSON store is compacted on every persist; nothing to do.
            Ok(Value::Null)
        }
        "isUsed" => {
            // Arg: { payload: <path> } — whether the state.vscdb path is in
            // use. Workspace stores live under workspaceStorage/<hash>; a
            // payload outside our layout can never be in use.
            let payload = arg.get("payload").and_then(Value::as_str).unwrap_or("");
            let used = workspace_state_files().iter().any(|p| {
                p.to_string_lossy().eq_ignore_ascii_case(payload)
            });
            Ok(json!(used))
        }
        "getFallbackApplicationStorageItems" => {
            // Application-shared fallback migration (fresh layout: empty).
            Ok(json!([]))
        }
        other => Err(format!("storage channel: call not found: {}", other)),
    }
}

// ---------------------------------------------------------------------------
// Scope resolution
// ---------------------------------------------------------------------------

fn scope_key(arg: &Value) -> String {
    let user_dir = USER_DIR
        .get()
        .cloned()
        .unwrap_or_else(|| PathBuf::from("."));
    let user = user_dir.to_string_lossy().replace('\\', "/");

    // Workspace storage: keyed by a stable hash of the identifier.
    if let Some(workspace) = arg.get("workspace").filter(|w| !w.is_null()) {
        let id = workspace_identifier_key(workspace);
        let hash = fnv1a_hex(&id);
        return format!("{}/User/workspaceStorage/{}/state.json", user, hash);
    }

    // Profile storage.
    if let Some(profile) = arg.get("profile").filter(|p| !p.is_null()) {
        let is_default = profile.get("isDefault").and_then(Value::as_bool).unwrap_or(false);
        let id = profile.get("id").and_then(Value::as_str).unwrap_or("unknown");
        if is_default {
            // The default profile's global storage IS the application storage.
            return format!("{}/User/globalStorage/storage.json", user);
        }
        return format!("{}/User/profiles/{}/globalStorage/storage.json", user, id);
    }

    // Application-shared storage.
    if arg.get("applicationShared").and_then(Value::as_bool).unwrap_or(false) {
        return format!("{}/User/globalStorage/storage.shared.json", user);
    }

    // Plain application storage.
    format!("{}/User/globalStorage/storage.json", user)
}

/// Canonical key of an IAnyWorkspaceIdentifier (empty-workspace aware).
fn workspace_identifier_key(workspace: &Value) -> String {
    if let Some(id) = workspace.get("id").and_then(Value::as_str) {
        if let Some(uri) = workspace.get("uri") {
            let path = uri.get("path").and_then(Value::as_str).unwrap_or("");
            let scheme = uri.get("scheme").and_then(Value::as_str).unwrap_or("");
            return format!("{}:{}", id, format!("{}{}", scheme, path));
        }
        return id.to_string();
    }
    "empty-window".to_string()
}

// ---------------------------------------------------------------------------
// Persistence (atomic temp-file + rename)
// ---------------------------------------------------------------------------

fn load_scope(scope_key: &str) -> BTreeMap<String, String> {
    if let Ok(guard) = STORES.lock() {
        if let Some(store) = guard.get(scope_key) {
            return store.clone();
        }
    }
    let path = PathBuf::from(scope_key);
    match std::fs::read_to_string(&path) {
        Ok(content) => match serde_json::from_str::<Value>(&content) {
            Ok(Value::Object(map)) => {
                let mut store = BTreeMap::new();
                for (key, value) in map {
                    if let Value::String(value) = value {
                        store.insert(key, value);
                    }
                }
                store
            }
            _ => BTreeMap::new(),
        },
        Err(_) => BTreeMap::new(),
    }
}

fn persist_scope(scope_key: &str, store: &BTreeMap<String, String>) -> Result<(), String> {
    let path = PathBuf::from(scope_key);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|err| format!("storage: cannot create {:?}: {}", parent, err))?;
    }
    let mut object = Map::new();
    for (key, value) in store {
        object.insert(key.clone(), json!(value));
    }
    let body = serde_json::to_string(&Value::Object(object))
        .map_err(|err| format!("storage: serialize: {}", err))?;
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, body.as_bytes())
        .map_err(|err| format!("storage: write {:?}: {}", tmp, err))?;
    std::fs::rename(&tmp, &path)
        .map_err(|err| format!("storage: rename {:?}: {}", path, err))?;
    Ok(())
}

fn workspace_state_files() -> Vec<PathBuf> {
    let user_dir = USER_DIR.get().cloned().unwrap_or_else(|| PathBuf::from("."));
    let root = user_dir.join("workspaceStorage");
    let mut out = Vec::new();
    if let Ok(entries) = std::fs::read_dir(&root) {
        for entry in entries.flatten() {
            let state = entry.path().join("state.json");
            if state.is_file() {
                out.push(state);
            }
        }
    }
    out
}

fn fnv1a_hex(input: &str) -> String {
    let mut hash: u64 = 0xcbf29ce484222325;
    for byte in input.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    format!("{:016x}", hash)
}

// ---------------------------------------------------------------------------
// Tests (contract shape mirrors storageIpc.ts round-trips)
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn app_request(profile: Option<Value>, workspace: Option<Value>) -> Value {
        json!({
            "profile": profile,
            "workspace": workspace,
            "applicationShared": false,
        })
    }

    #[test]
    fn application_scope_is_stable() {
        let a = scope_key(&app_request(None, None));
        assert!(a.ends_with("User/globalStorage/storage.json"), "got {}", a);
        let b = scope_key(&app_request(None, None));
        assert_eq!(a, b);
    }

    #[test]
    fn default_profile_maps_to_application_storage() {
        let a = scope_key(&app_request(None, None));
        let b = scope_key(&app_request(Some(json!({"id": "__default__profile__", "isDefault": true})), None));
        assert_eq!(a, b);
    }

    #[test]
    fn named_profile_has_its_own_store() {
        let a = scope_key(&app_request(Some(json!({"id": "abc", "isDefault": false})), None));
        assert!(a.ends_with("User/profiles/abc/globalStorage/storage.json"), "got {}", a);
    }

    #[test]
    fn workspace_scopes_are_hashed_and_stable() {
        let w = json!({"id": "folder", "uri": {"scheme": "file", "path": "/C:/proj"}});
        let a = scope_key(&app_request(None, Some(w.clone())));
        let b = scope_key(&app_request(None, Some(w)));
        assert_eq!(a, b);
        assert!(a.contains("/workspaceStorage/"), "got {}", a);
    }

    #[test]
    fn application_shared_is_separate() {
        let req = json!({ "applicationShared": true });
        let a = scope_key(&req);
        assert!(a.ends_with("User/globalStorage/storage.shared.json"), "got {}", a);
    }

    #[test]
    fn fnv_hash_is_deterministic() {
        assert_eq!(fnv1a_hex("folder:fileC:/proj"), fnv1a_hex("folder:fileC:/proj"));
        assert_ne!(fnv1a_hex("folder:fileC:/a"), fnv1a_hex("folder:fileC:/b"));
    }

    #[test]
    fn update_and_read_back_round_trip() {
        // Use a throwaway scope via VSTAURI test env: point USER_DIR at a
        // temp dir before init. handle() persists to disk for real.
        let tmp = std::env::temp_dir().join(format!("vstauri-storage-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        init(&tmp);

        let req = json!({
            "profile": null, "workspace": null, "applicationShared": false,
            "insert": [["layout.key", "1"], ["theme", "dark"]],
        });
        handle("updateItems", &req).expect("updateItems");

        let read = json!({ "profile": null, "workspace": null, "applicationShared": false });
        let items = handle("getItems", &read).expect("getItems");
        let items = items.as_array().expect("items array");
        assert_eq!(items.len(), 2);

        let value = handle(
            "getValue",
            &json!({ "profile": null, "workspace": null, "applicationShared": false, "key": "theme" }),
        )
        .expect("getValue");
        assert_eq!(value, json!("dark"));

        let _ = std::fs::remove_dir_all(&tmp);
    }
}
