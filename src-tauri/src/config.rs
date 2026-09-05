//! Window configuration builder (Phase 1).
//!
//! In Electron, `electron-main` builds an `INativeWindowConfiguration`
//! (src/vs/platform/window/common/window.ts) and hands it to the renderer
//! through the `--vscode-window-config` preload IPC handshake. Here the Rust
//! shell builds the same structure and serves it through the
//! `vscode_window_config` Tauri command; the shim's
//! `window.vscode.context.resolveConfiguration()` consumes it.
//!
//! Required fields were extracted from the source interfaces
//! (ISandboxConfiguration, IWindowConfiguration, INativeWindowConfiguration)
//! rather than guessed. Everything main-process-owned (machineId, profiles,
//! paths, product configuration, NLS messages) is materialized here.

use serde_json::{json, Map, Value};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

static WINDOW_CONFIG: OnceLock<Value> = OnceLock::new();

/// Build and cache the window configuration. Called once from `setup` before
/// the workbench window loads.
pub fn init(app: &tauri::AppHandle) {
    let value = build(app);
    if WINDOW_CONFIG.set(value).is_err() {
        crate::logger::log_app("warn", "window configuration initialized twice");
    }
}

/// Cached configuration (None until `init` ran).
pub fn window_config() -> Option<&'static Value> {
    WINDOW_CONFIG.get()
}

pub fn user_env() -> Value {
    match window_config() {
        Some(config) => config
            .get("userEnv")
            .cloned()
            .unwrap_or_else(|| json!({})),
        None => json!({}),
    }
}

fn build(app: &tauri::AppHandle) -> Value {
    let client_root = crate::protocol::client_root(app);
    let root_str = client_root.to_string_lossy().replace('\\', "/");

    let data_root = data_root();
    if let Err(err) = std::fs::create_dir_all(&data_root) {
        crate::logger::log_app("error", &format!("cannot create data root {:?}: {}", data_root, err));
    }
    let user_dir = data_root.join("User");
    let logs_dir = data_root.join("logs");
    for dir in [&user_dir, &logs_dir] {
        if let Err(err) = std::fs::create_dir_all(dir) {
            crate::logger::log_app("error", &format!("cannot create dir {:?}: {}", dir, err));
        }
    }
    crate::logger::init(&logs_dir);
    crate::ipc::init(&logs_dir);

    let machine_id = persistent_machine_id(&data_root);
    let session_id = crate::util::random_uuid_v4();

    let product = read_json_file(&client_root.join("product.json")).unwrap_or_else(|| {
        crate::logger::log_app("warn", "product.json missing from client bundle; using fallback");
        json!({
            "nameShort": "Visual Studio Code",
            "nameLong": "Visual Studio Code",
            "applicationName": "Visual Studio Code",
            "dataFolderName": ".vstauri",
            "version": "1.138.0"
        })
    });

    let nls_messages = read_json_file(&client_root.join("nls.messages.json"))
        .filter(|value| value.is_array())
        .unwrap_or_else(|| {
            crate::logger::log_app("warn", "nls.messages.json missing/invalid; falling back to built-in English defaults");
            json!([])
        });

    // Dev-parity ESM boot: the workbench's absolute workbench import
    // (`vscode-file://vscode-app/<appRoot>/out/...`) cannot be resolved by
    // WebView2 (wry only routes http(s) WebResourceRequested traffic), so the
    // shim enables the renderer's own documented development path:
    // VSCODE_DEV + _VSCODE_USE_RELATIVE_IMPORTS makes workbench.ts import
    // `../../../workbench/workbench.desktop.main.js` relative to the document,
    // which resolves inside this origin. See shim.js.

    let exec_path = std::env::current_exe()
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_default();
    let cwd = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|dir| dir.to_string_lossy().into_owned()))
        .unwrap_or_else(|| "/".to_string());

    let home_dir = home_dir();
    let tmp_dir = std::env::var("TMPDIR")
        .or_else(|_| std::env::var("TEMP"))
        .or_else(|_| std::env::var("TMP"))
        .unwrap_or_else(|_| "/tmp".to_string());

    let hostname = std::env::var("COMPUTERNAME")
        .or_else(|_| std::env::var("HOSTNAME"))
        .unwrap_or_else(|_| "localhost".to_string());

    let platform = if cfg!(windows) {
        "win32"
    } else if cfg!(target_os = "macos") {
        "darwin"
    } else {
        "linux"
    };
    let arch = if cfg!(target_arch = "x86_64") {
        "x64"
    } else if cfg!(target_arch = "aarch64") {
        "arm64"
    } else {
        "unknown"
    };

    let profile = default_profile(&user_dir, &data_root);

    json!({
        // ISandboxConfiguration
        "windowId": 1,
        "appRoot": root_str,
        "userEnv": Value::Object(os_env_map()),
        "product": product,
        "nls": {
            "messages": nls_messages,
            "language": "en"
        },
        // CSS modules stay EMPTY in this shell: the Electron dev-mode css
        // import map is keyed by `vscode-file://` URLs that can never match
        // this document's origin, so the workbench's setupCSSImportMaps would
        // build a dead map. Instead protocol.rs answers every `import './x.css'`
        // module-graph member with a `_VSCODE_CSS_LOAD` wrapper (see
        // protocol.rs css_module_response) — the server-side equivalent of the
        // blob modules that cssModules produces in Electron dev.
        "cssModules": [],

        // INativeWindowConfiguration
        "mainPid": std::process::id(),
        "machineId": machine_id,
        "sqmId": machine_id,
        "devDeviceId": session_id,
        "isPortable": false,
        "execPath": exec_path,
        "profiles": {
            "home": uri_components(&user_dir.join("profiles")),
            "all": [profile.clone()],
            "profile": profile
        },
        "homeDir": home_dir,
        "tmpDir": tmp_dir,
        "userDataDir": data_root.to_string_lossy().replace('\\', "/"),
        "isInitialStartup": true,
        "logLevel": 2,
        "loggers": [],
        "fullscreen": false,
        "maximized": false,
        "accessibilitySupport": false,
        "colorScheme": { "dark": true, "highContrast": false },
        "autoDetectHighContrast": false,
        "autoDetectColorScheme": true,
        "perfMarks": [],
        "os": {
            "release": "10.0.0",
            "hostname": hostname,
            "arch": arch
        },

        // IWindowConfiguration (workspace / files)
        "workspace": null,
        "folderUri": null,
        "remoteAuthority": null,
        "filesToOpenOrCreate": [],
        "filesToDiff": [],
        "filesToNew": [],
        "userAgent": null,
        "zoomLevel": 0,

        // Shell-private metadata consumed by the preload shim (removed from
        // the contract surface; harmless extra key for the workbench).
        "__vstauri": {
            "platform": platform,
            "arch": arch,
            "execPath": exec_path,
            "cwd": cwd,
            "versions": {
                "node": "22.14.0",
                "v8": "13.0.0",
                "electron": "37.2.0",
                "chrome": "138.0.7204.100"
            }
        }
    })
}

/// The default profile, field-for-field like electron-main's
/// `createDefaultProfile()` (src/vs/platform/userDataProfile/common/userDataProfile.ts):
/// id `__default__profile__`, location = userRoamingDataHome, every resource
/// joined off the location, cacheHome under CachedProfilesData.
fn default_profile(user_dir: &Path, data_root: &Path) -> Value {
    let location = user_dir;
    let cache_home = data_root
        .join("Cache")
        .join("CachedProfilesData")
        .join("__default__profile__");
    json!({
        "id": "__default__profile__",
        "name": "Default",
        "isDefault": true,
        "location": uri_components(location),
        "globalStorageHome": uri_components(&location.join("globalStorage")),
        "settingsResource": uri_components(&location.join("settings.json")),
        "keybindingsResource": uri_components(&location.join("keybindings.json")),
        "tasksResource": uri_components(&location.join("tasks.json")),
        "snippetsHome": uri_components(&location.join("snippets")),
        "promptsHome": uri_components(&location.join("prompts")),
        "extensionsResource": uri_components(&location.join("extensions.json")),
        "mcpResource": uri_components(&location.join("mcp.json")),
        "languageModelsResource": uri_components(&location.join("chatLanguageModels.json")),
        "agentPluginsHome": uri_components(&location.join("agent-plugins")),
        "cacheHome": uri_components(&cache_home),
        "isTransient": false,
        "isAgentsWindowProfile": false
    })
}

/// Cache home URI for nativeHost.getCacheHome.
pub fn cache_home_uri() -> Value {
    uri_components(&data_root().join("Cache"))
}

fn read_json_file(path: &Path) -> Option<Value> {
    match std::fs::read_to_string(path) {
        Ok(content) => match serde_json::from_str(&content) {
            Ok(value) => Some(value),
            Err(err) => {
                crate::logger::log_app("error", &format!("cannot parse {:?}: {}", path, err));
                None
            }
        },
        Err(err) => {
            crate::logger::log_app("warn", &format!("cannot read {:?}: {}", path, err));
            None
        }
    }
}

fn data_root() -> PathBuf {
    if let Ok(appdata) = std::env::var("APPDATA") {
        if !appdata.is_empty() {
            return PathBuf::from(appdata).join("VSTauri");
        }
    }
    if let Ok(home) = std::env::var("USERPROFILE") {
        if !home.is_empty() {
            return PathBuf::from(home).join(".vstauri");
        }
    }
    if let Ok(home) = std::env::var("HOME") {
        if !home.is_empty() {
            return PathBuf::from(home).join(".vstauri");
        }
    }
    PathBuf::from(".vstauri")
}

fn home_dir() -> String {
    std::env::var("USERPROFILE")
        .or_else(|_| std::env::var("HOME"))
        .map(|value| value.replace('\\', "/"))
        .unwrap_or_else(|_| "/".to_string())
}

fn persistent_machine_id(data_root: &Path) -> String {
    let file = data_root.join("machineid");
    if let Ok(content) = std::fs::read_to_string(&file) {
        let trimmed = content.trim();
        if trimmed.len() == 36 {
            return trimmed.to_string();
        }
    }
    let id = crate::util::random_uuid_v4();
    let _ = std::fs::write(&file, &id);
    id
}

fn os_env_map() -> Map<String, Value> {
    let mut map = Map::new();
    for (key, value) in std::env::vars_os() {
        map.insert(
            key.to_string_lossy().into_owned(),
            Value::String(value.to_string_lossy().into_owned()),
        );
    }
    map
}

/// VS Code `UriComponents` (object form accepted by `URI.revive`).
fn uri_components(path: &Path) -> Value {
    let mut normalized = path.to_string_lossy().replace('\\', "/");
    if !normalized.starts_with('/') {
        normalized = format!("/{}", normalized);
    }
    json!({
        "scheme": "file",
        "authority": "",
        "path": crate::util::encode_uri_path(&normalized),
        "query": "",
        "fragment": ""
    })
}
