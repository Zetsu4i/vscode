//! Mountain: `userDataProfiles` protocol channel.
//!
//! Implements the renderer-visible surface of the user data profile service:
//! the channel registered in electron-main via
//! `ProxyChannel.fromService(IUserDataProfilesMainService)` and consumed by
//! `UserDataProfilesService` (userDataProfileIpc.ts). Commands arrive as
//! argument arrays:
//!
//!   createNamedProfile([name, options?, workspaceIdentifier?]) -> profile
//!   createProfile([id, name, options?, workspaceIdentifier?])     -> profile
//!   createTransientProfile([workspaceIdentifier?])                -> profile
//!   setProfileForWorkspace([workspaceIdentifier, profile])        -> void
//!   removeProfile([profile])                                      -> void
//!   updateProfile([profile, updateOptions])                       -> profile
//!   resetWorkspaces()                                             -> void
//!   cleanUp() / cleanUpTransientProfiles()                        -> void
//!   events: onDidChangeProfiles ({all, added, removed, updated}),
//!           onDidResetWorkspaces
//!
//! Profiles persist in `User/profiles.json`; the default profile is created
//! from the same field-for-field shape config.rs serves in the window
//! configuration (id `__default__profile__`, location = User roamed home).

use serde_json::{json, Map, Value};
use std::path::{Path, PathBuf};
use std::sync::{LazyLock, Mutex, OnceLock};

static USER_DIR: OnceLock<PathBuf> = OnceLock::new();
static STATE: LazyLock<Mutex<ProfilesState>> = LazyLock::new(|| Mutex::new(ProfilesState::default()));

#[derive(Default)]
struct ProfilesState {
    /// Persistent + transient profiles, default first.
    profiles: Vec<Value>,
    /// workspace identifier key -> profile id.
    workspace_associations: std::collections::BTreeMap<String, String>,
    dirty: bool,
}

/// Called from config::build once the User dir exists.
pub fn init(user_dir: &Path) {
    let _ = USER_DIR.set(user_dir.to_path_buf());
    if let Ok(mut state) = STATE.lock() {
        state.reload(user_dir);
    }
}

impl ProfilesState {
    fn reload(&mut self, user_dir: &Path) {
        self.profiles.clear();
        self.workspace_associations.clear();

        let file = user_dir.join("profiles.json");
        if let Ok(content) = std::fs::read_to_string(&file) {
            if let Ok(Value::Object(map)) = serde_json::from_str::<Value>(&content) {
                if let Some(Value::Array(profiles)) = map.get("profiles") {
                    self.profiles = profiles.clone();
                }
                if let Some(Value::Object(assoc)) = map.get("workspaces") {
                    for (key, value) in assoc {
                        if let Some(id) = value.as_str() {
                            self.workspace_associations.insert(key.clone(), id.to_string());
                        }
                    }
                }
            }
        }

        // The default profile always exists and always comes first.
        if self.profiles.is_empty() {
            self.profiles.push(crate::config::default_profile_json(user_dir));
            self.dirty = true;
        } else if !self
            .profiles
            .first()
            .and_then(|p| p.get("isDefault"))
            .and_then(Value::as_bool)
            .unwrap_or(false)
        {
            self.profiles.insert(0, crate::config::default_profile_json(user_dir));
            self.dirty = true;
        }
        if self.dirty {
            let _ = self.persist(user_dir);
            self.dirty = false;
        }
    }

    fn persist(&self, user_dir: &Path) -> Result<(), String> {
        let mut assoc = Map::new();
        for (key, value) in &self.workspace_associations {
            assoc.insert(key.clone(), json!(value));
        }
        let body = json!({
            "profiles": self.profiles,
            "workspaces": Value::Object(assoc),
        });
        let file = user_dir.join("profiles.json");
        let tmp = user_dir.join("profiles.json.tmp");
        std::fs::write(&tmp, serde_json::to_string(&body).map_err(|e| e.to_string())?.as_bytes())
            .map_err(|err| format!("profiles: write {:?}: {}", tmp, err))?;
        std::fs::rename(&tmp, &file)
            .map_err(|err| format!("profiles: rename {:?}: {}", file, err))?;
        Ok(())
    }
}

fn user_dir() -> PathBuf {
    USER_DIR.get().cloned().unwrap_or_else(|| PathBuf::from("."))
}

/// Handle one `userDataProfiles` channel request (arg = argument array).
pub fn handle(command: &str, arg: &Value) -> Result<Value, String> {
    let args = arg.as_array().cloned().unwrap_or_default();

    match command {
        "createProfile" => {
            let id = args.first().and_then(Value::as_str).unwrap_or_default().to_string();
            let name = args.get(1).and_then(Value::as_str).unwrap_or_default().to_string();
            Ok(create_profile(id, name, args.get(2), false)?)
        }
        "createNamedProfile" => {
            let name = args.first().and_then(Value::as_str).unwrap_or_default().to_string();
            let id = sanitize_profile_id(&name);
            Ok(create_profile(id, name, args.get(1), false)?)
        }
        "createTransientProfile" => {
            let id = format!("tmp-{}", &crate::util::random_uuid_v4()[..8]);
            let name = format!("Temporary Profile");
            Ok(create_profile(id, name, args.first().and_then(|_| args.get(0)), true)?)
        }
        "setProfileForWorkspace" => {
            let workspace = args.first().cloned().unwrap_or(Value::Null);
            let profile = args.get(1).cloned().unwrap_or(Value::Null);
            let profile_id = profile.get("id").and_then(Value::as_str).unwrap_or_default().to_string();
            if let Ok(mut state) = STATE.lock() {
                state
                    .workspace_associations
                    .insert(workspace_key(&workspace), profile_id);
                let _ = state.persist(&user_dir());
            }
            Ok(Value::Null)
        }
        "removeProfile" => {
            let profile = args.first().cloned().unwrap_or(Value::Null);
            let id = profile.get("id").and_then(Value::as_str).unwrap_or_default().to_string();
            let removed = if let Ok(mut state) = STATE.lock() {
                let before = state.profiles.len();
                state.profiles.retain(|p| {
                    p.get("id").and_then(Value::as_str).unwrap_or_default() != id
                        || p.get("isDefault").and_then(Value::as_bool).unwrap_or(false)
                });
                let removed = state.profiles.len() < before;
                if removed {
                    let _ = state.persist(&user_dir());
                }
                removed
            } else {
                false
            };
            if removed {
                crate::ipc::fire_event("userDataProfiles", "onDidChangeProfiles", &change_event(&[], &[id.as_str()], &[]));
            }
            Ok(Value::Null)
        }
        "updateProfile" => {
            let profile = args.first().cloned().unwrap_or(Value::Null);
            let update = args.get(1).cloned().unwrap_or(Value::Null);
            let id = profile.get("id").and_then(Value::as_str).unwrap_or_default().to_string();
            let updated = if let Ok(mut state) = STATE.lock() {
                let mut updated = None;
                for existing in state.profiles.iter_mut() {
                    if existing.get("id").and_then(Value::as_str) == Some(id.as_str()) {
                        if let Some(name) = update.get("name").and_then(Value::as_str) {
                            existing["name"] = json!(name);
                        }
                        if let Some(icon) = update.get("icon") {
                            existing["icon"] = icon.clone();
                        }
                        if let Some(use_flags) = update.get("useFlags") {
                            existing["useFlags"] = use_flags.clone();
                        }
                        updated = Some(existing.clone());
                    }
                }
                if updated.is_some() {
                    let _ = state.persist(&user_dir());
                }
                updated
            } else {
                None
            };
            match updated {
                Some(profile) => {
                    crate::ipc::fire_event("userDataProfiles", "onDidChangeProfiles", &change_event(&[], &[], &[id.as_str()]));
                    Ok(profile)
                }
                None => Err(format!("profiles channel: profile {} not found", id)),
            }
        }
        "resetWorkspaces" => {
            if let Ok(mut state) = STATE.lock() {
                state.workspace_associations.clear();
                let _ = state.persist(&user_dir());
            }
            crate::ipc::fire_event("userDataProfiles", "onDidResetWorkspaces", &Value::Null);
            Ok(Value::Null)
        }
        "cleanUp" | "cleanUpTransientProfiles" => {
            if let Ok(mut state) = STATE.lock() {
                let before = state.profiles.len();
                state.profiles.retain(|p| {
                    p.get("isTransient").and_then(Value::as_bool).unwrap_or(false) == false
                });
                if state.profiles.len() < before {
                    let _ = state.persist(&user_dir());
                }
            }
            Ok(Value::Null)
        }
        other => Err(format!("profiles channel: call not found: {}", other)),
    }
}

fn create_profile(id: String, name: String, options: Option<&Value>, transient: bool) -> Result<Value, String> {
    let dir = user_dir();
    let location = dir.join("profiles").join(&id);
    std::fs::create_dir_all(&location)
        .map_err(|err| format!("profiles: cannot create {:?}: {}", location, err))?;

    let mut profile = Map::new();
    profile.insert("id".to_string(), json!(id));
    profile.insert("name".to_string(), json!(name));
    profile.insert("isDefault".to_string(), json!(false));
    profile.insert("isTransient".to_string(), json!(transient));
    profile.insert("location".to_string(), crate::config::uri_json(&location));
    profile.insert("globalStorageHome".to_string(), crate::config::uri_json(&location.join("globalStorage")));
    profile.insert("settingsResource".to_string(), crate::config::uri_json(&location.join("settings.json")));
    profile.insert("keybindingsResource".to_string(), crate::config::uri_json(&location.join("keybindings.json")));
    profile.insert("tasksResource".to_string(), crate::config::uri_json(&location.join("tasks.json")));
    profile.insert("snippetsHome".to_string(), crate::config::uri_json(&location.join("snippets")));
    profile.insert("promptsHome".to_string(), crate::config::uri_json(&location.join("prompts")));
    profile.insert("extensionsResource".to_string(), crate::config::uri_json(&location.join("extensions.json")));
    profile.insert("mcpResource".to_string(), crate::config::uri_json(&location.join("mcp.json")));
    profile.insert("languageModelsResource".to_string(), crate::config::uri_json(&location.join("chatLanguageModels.json")));
    profile.insert("agentPluginsHome".to_string(), crate::config::uri_json(&location.join("agent-plugins")));
    profile.insert("cacheHome".to_string(), crate::config::uri_json(&dir.join("Cache").join("CachedProfilesData").join(&id)));
    profile.insert("isAgentsWindowProfile".to_string(), json!(false));
    if let Some(options) = options {
        if let Some(icon) = options.get("icon") {
            profile.insert("icon".to_string(), icon.clone());
        }
    }
    let profile = Value::Object(profile);

    if let Ok(mut state) = STATE.lock() {
        state.profiles.push(profile.clone());
        if !transient {
            let _ = state.persist(&dir);
        }
        crate::ipc::fire_event("userDataProfiles", "onDidChangeProfiles", &change_event(&[&profile], &[], &[]));
    }
    Ok(profile)
}

fn sanitize_profile_id(name: &str) -> String {
    let cleaned: String = name
        .to_lowercase()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect();
    let trimmed = cleaned.trim_matches('-').to_string();
    if trimmed.is_empty() {
        format!("profile-{}", &crate::util::random_uuid_v4()[..8])
    } else {
        trimmed
    }
}

fn workspace_key(workspace: &Value) -> String {
    if let Some(id) = workspace.get("id").and_then(Value::as_str) {
        if let Some(uri) = workspace.get("uri") {
            let path = uri.get("path").and_then(Value::as_str).unwrap_or("");
            return format!("{}:{}", id, path);
        }
        return id.to_string();
    }
    "empty-window".to_string()
}

fn change_event(added: &[&Value], removed: &[&str], updated: &[&str]) -> Value {
    let all = if let Ok(state) = STATE.lock() {
        state.profiles.clone()
    } else {
        Vec::new()
    };
    let removed_profiles: Vec<Value> = removed
        .iter()
        .map(|id| json!({ "id": id, "name": id }))
        .collect();
    let updated_profiles: Vec<Value> = all
        .iter()
        .filter(|p| {
            updated.contains(&p.get("id").and_then(Value::as_str).unwrap_or_default())
        })
        .cloned()
        .collect();
    json!({
        "all": all,
        "added": added.iter().map(|a| (*a).clone()).collect::<Vec<Value>>(),
        "removed": removed_profiles,
        "updated": updated_profiles,
    })
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn ids_are_sanitized() {
        assert_eq!(sanitize_profile_id("My Cool Profile"), "my-cool-profile");
        assert!(sanitize_profile_id("///").starts_with("profile-"));
    }

    #[test]
    fn default_profile_is_first_and_immutable() {
        let tmp = std::env::temp_dir().join(format!("vstauri-profiles-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();
        init(&tmp);

        // Attempting to remove the default profile is a no-op.
        let default = json!({ "id": "__default__profile__", "isDefault": true });
        handle("removeProfile", &json!([default])).expect("removeProfile");
        let state = STATE.lock().unwrap();
        assert!(state
            .profiles
            .first()
            .and_then(|p| p.get("id"))
            .and_then(Value::as_str)
            .unwrap_or("")
            .contains("__default__profile__"));
        drop(state);

        // Creating and reading back a named profile round-trips.
        let created = handle("createNamedProfile", &json!(["Work"])).expect("createNamedProfile");
        assert_eq!(created.get("name").and_then(Value::as_str), Some("Work"));
        assert!(created.get("id").and_then(Value::as_str).unwrap().contains("work"));
        assert!(tmp.join("profiles.json").is_file());

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn workspace_associations_round_trip() {
        let w = json!({"id": "folder", "uri": {"scheme": "file", "path": "/C:/x"}});
        assert_eq!(workspace_key(&w), "folder:/C:/x");
    }
}
