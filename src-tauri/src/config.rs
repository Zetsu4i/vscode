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
use std::sync::RwLock;

static WINDOW_CONFIG: RwLock<Option<Value>> = RwLock::new(None);

/// Build and cache the window configuration. Called once from `setup` before
/// the workbench window loads.
pub fn init(app: &tauri::AppHandle) {
    let value = build(app);
    let mut guard = WINDOW_CONFIG.write().unwrap_or_else(|poisoned| poisoned.into_inner());
    if guard.is_some() {
        crate::logger::log_app("warn", "window configuration initialized twice");
    } else {
        *guard = Some(value);
    }
}

/// Cached configuration (None until `init` ran). Cloned out because the
/// configuration became mutable: `openWindow` / the pick*AndOpen dialogs
/// rewrite the workspace/file fields and reload the window.
pub fn window_config() -> Option<Value> {
    WINDOW_CONFIG
        .read()
        .ok()
        .and_then(|guard| guard.clone())
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

/// Apply `IWindowOpenable[]` (nativeHost `openWindow` / pick*AndOpen) to
/// the configuration so the next workbench boot opens them: the LAST
/// folder/workspace wins (upstream semantics — one workspace container per
/// window), files accumulate into `filesToOpenOrCreate`. Returns true when
/// the configuration changed and the window should reload.
pub fn apply_window_openables(openables: &[Value]) -> bool {
    let Some(config) = window_config() else {
        return false;
    };
    let mut map = match config {
        Value::Object(map) => map,
        _ => return false,
    };

    let mut changed = false;
    for openable in openables {
        if let Some(file_uri) = openable.get("fileUri") {
            // IPathToOpen: { fileUri, line?, column? }
            let mut entry = Map::new();
            entry.insert("fileUri".to_string(), file_uri.clone());
            if let Some(line) = openable.get("line") {
                entry.insert("line".to_string(), line.clone());
            }
            if let Some(column) = openable.get("column") {
                entry.insert("column".to_string(), column.clone());
            }
            let files = map
                .entry("filesToOpenOrCreate")
                .or_insert_with(|| json!([]));
            if let Some(list) = files.as_array_mut() {
                list.push(Value::Object(entry));
                changed = true;
            }
        } else if let Some(folder_uri) = openable.get("folderUri") {
            map.insert("folderUri".to_string(), folder_uri.clone());
            map.insert("workspace".to_string(), Value::Null);
            changed = true;
        } else if let Some(workspace_uri) = openable.get("workspaceUri") {
            // IWorkspaceIdentifier: id = md5(lowercased fsPath off Linux),
            // configPath = the workspace .code-workspace URI.
            let fs_path = uri_fs_path(workspace_uri);
            let id_input = if cfg!(target_os = "linux") {
                fs_path.clone()
            } else {
                fs_path.to_lowercase()
            };
            map.insert(
                "workspace".to_string(),
                json!({
                    "id": crate::util::md5_hex(&id_input),
                    "configPath": workspace_uri.clone(),
                }),
            );
            map.insert("folderUri".to_string(), Value::Null);
            changed = true;
        }
    }

    if changed {
        if let Ok(mut guard) = WINDOW_CONFIG.write() {
            *guard = Some(Value::Object(map));
        }
    }
    changed
}

/// UriComponents -> filesystem path (fsPath semantics, enough for hashing
/// and dialog default paths).
fn uri_fs_path(uri: &Value) -> String {
    let raw = uri.get("path").and_then(Value::as_str).unwrap_or("");
    let decoded = crate::util::percent_decode(raw);
    if cfg!(windows) {
        decoded.trim_start_matches('/').replace('/', "\\")
    } else {
        decoded
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
    // First-boot noise reduction: the workbench probes a fixed set of
    // profile sub-dirs during startup (user extensions, snippets, prompts,
    // globalStorage, per-window logs). A missing dir is handled, but it
    // surfaces as FileSystemError(FileNotFound) noise in the renderer log
    // and in `vscode-file 404` traces. Electron's first boot creates the
    // same tree; match it. `logs/<session>/window1` mirrors what
    // `logsHome`/`windowLogsPath` (environmentService.ts) derive from the
    // `logsPath` we put into the configuration below.
    let session_stamp = chrono_like_stamp(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0),
    );
    let session_logs_dir = logs_dir.join(&session_stamp);
    for dir in [
        user_dir.join("globalStorage"),
        user_dir.join("snippets"),
        user_dir.join("prompts"),
        user_dir.join("extensions"),
        user_dir.join("History"),
        session_logs_dir.join("window1"),
    ] {
        let _ = std::fs::create_dir_all(dir);
    }
    crate::logger::init(&logs_dir);
    crate::ipc::init(&logs_dir);
    crate::logger_channel::init(&logs_dir);
    crate::storage_channel::init(&user_dir);
    crate::profiles_channel::init(&user_dir);
    crate::workspaces_channel::init(&user_dir);
    crate::keyboard_channel::init();
    // Resolve the shell-integration scripts dir + product quality before
    // any localPty createProcess arrives.
    crate::terminal_channel::init(app);

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

        // Electron-main parity fields the renderer derives through
        // environmentService (args = the configuration object itself):
        //   * `builtin-extensions-dir` — builtinExtensionsPath. Without it,
        //     the scanner falls back to FileAccess.asFileUri('').fsPath,
        //     which under the Wind shim's document-origin file root resolves
        //     to a RELATIVE "extensions" path and the whole system-extension
        //     scan fails (FileNotFound for 'extensions').
        //   * `logsPath` — logsHome; lets renderer.log / output channels
        //     land in the per-session directory the shell just created.
        "builtin-extensions-dir": client_root.join("extensions").to_string_lossy().replace('\\', "/"),
        "logsPath": session_logs_dir.to_string_lossy().replace('\\', "/"),

        // Parts splash: Electron main passes the persisted theme splash so
        // workbench.js paints the shell skeleton synchronously (before the
        // module graph finishes loading) — this is what makes real VS Code
        // feel instant on cold boot. No persisted state exists yet, so ship
        // the default dark_plus-shaped splash; the workbench swaps it for
        // the real layout as soon as it renders (showSplash removes
        // #monaco-parts-splash when the layout is ready).
        "partsSplash": default_parts_splash(),

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
pub fn default_profile_json(user_dir: &Path) -> Value {
    default_profile(user_dir, &data_root())
}

/// VS Code `UriComponents` (object form accepted by `URI.revive`).
pub fn uri_json(path: &Path) -> Value {
    uri_components(path)
}
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

/// OS color scheme (nativeHost.getOSColorScheme). The workbench drives the
/// theme from this; dark matches the compiled-in default workbench theme.
pub fn color_scheme() -> Value {
    json!({ "dark": true, "highContrast": false })
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

/// Data root (`%APPDATA%/VSTauri` on Windows) shared by config, storage,
/// profiles and logs. Public for the Mountain channel services.
pub fn data_root() -> PathBuf {
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

/// `YYYYMMDDTHHMMSS` session stamp for the logs directory — the same shape
/// electron-main's `logs/<date>` sessions use (toLocalISOString compacted).
fn chrono_like_stamp(secs: u64) -> String {
    let days = (secs / 86_400) as i64;
    let secs_of_day = secs % 86_400;
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = if month <= 2 { y + 1 } else { y };
    format!(
        "{:04}{:02}{:02}T{:02}{:02}{:02}",
        year,
        month,
        day,
        secs_of_day / 3600,
        (secs_of_day % 3600) / 60,
        secs_of_day % 60
    )
}

/// Default dark_plus parts splash (IPartsSplash in themeService.ts) so
/// workbench.js paints the classic shell skeleton — titlebar strip, activity
/// bar, sidebar, editor region, statusbar — synchronously while the module
/// graph streams in. Sizes match the workbench's classic-layout defaults;
/// every touched color has the dark_plus value. The real workbench replaces
/// this skeleton the moment the layout service renders.
fn default_parts_splash() -> Value {
    json!({
        "baseTheme": "vs-dark",
        "colorInfo": {
            "background": "#1e1e1e",
            "foreground": "#cccccc",
            "editorBackground": "#1e1e1e",
            "editorForeground": "#cccccc",
            "activityBarBackground": "#333333",
            "activityBarForeground": "#cccccc",
            "titleBarBackground": "#3c3c3c",
            "titleBarForeground": "#cccccc",
            "statusBarBackground": "#007acc",
            "statusBarForeground": "#ffffff",
            "sideBarBackground": "#252526",
            "sideBarForeground": "#cccccc",
            "editorGroupBorder": "#444444",
            "editorGroupHeaderTabsBorder": "#252526",
            "panelBackground": "#1e1e1e"
        },
        "layoutInfo": {
            "sideBarWidth": 300,
            "sideBarSide": "left",
            "titleBarHeight": 35,
            "activityBarWidth": 48,
            "auxiliaryBarWidth": 0,
            "auxiliaryBarSide": "right",
            "statusBarHeight": 22,
            "editorPartMinWidth": 220,
            "modernUI": false,
            "modernUICompact": false
        }
    })
}
