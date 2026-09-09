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
//!
//! Multi-window (Phase 3): every workbench window gets its own
//! configuration (`WINDOW_CONFIGS`, label-keyed); `fresh_window_config`
//! derives one for a new window. The boot JSON (`boot_json_for`) feeds the
//! shim's pre-paint: the DETECTED OS theme (registry
//! AppsUseLightTheme/SystemUsesLightTheme + high contrast), the splash
//! colors and the product branding ("VSTauri" by Mouri Younes).

use serde_json::{json, Map, Value};
use std::path::{Path, PathBuf};
use std::sync::RwLock;

static WINDOW_CONFIGS: RwLock<Option<serde_json::Map<String, Value>>> = RwLock::new(None);

/// Build and cache the main window configuration. Called once from `setup`
/// before the workbench window loads. Restores the previous session's
/// workspace + hot-exit backup path (window_state).
pub fn init(app: &tauri::AppHandle) {
    let mut value = build(app);

    // Session restore: the last active window's workspace/folder + backup
    // path land in the main window's configuration (windowsMainService
    // parity — `window.reopenFolders` defaults to 'one').
    let restored = crate::window_state::init();
    if let (Some(target), Some(source)) = (value.as_object_mut(), restored.as_object()) {
        for key in ["workspace", "folderUri", "backupPath"] {
            if let Some(v) = source.get(key) {
                target.insert(key.to_string(), v.clone());
            }
        }
        if restored.as_object().map(|o| !o.is_empty()).unwrap_or(false) {
            crate::logger::log_app("info", "config: restoring previous session workspace (hot exit)");
        }
    }

    // Hot exit: EVERY boot gets a backup path for its workspace — the
    // renderer's BackupTracker writes unsaved working copies under it and
    // restores them on the next boot ("backups on exit" — the original
    // VS Code behavior of recovering editors with unsaved content). Without
    // this the main window of a fresh session has nowhere to spill to.
    if let Some(config) = value.as_object_mut() {
        let whole = Value::Object(config.clone());
        if let Some(backup) = crate::window_state::backup_path_for_config(&whole) {
            config.insert("backupPath".to_string(), json!(backup));
        }
    }

    // Session tracking: close-time persistence reads this (window_state::
    // on_window_closed) — without it an empty-window session would never
    // be written and "restore last session" would do nothing.
    if let Some(config) = window_config_for("main") {
        crate::window_state::track_window_workspace(1, &config);
    }

    let mut guard = WINDOW_CONFIGS.write().unwrap_or_else(|poisoned| poisoned.into_inner());
    let map = guard.get_or_insert_with(Map::new);
    map.insert("main".to_string(), value);
}

/// Cached configuration for a window (the main window's for unknown labels
/// — auxiliary windows share the main config; their ids are assigned
/// through `vscode:registerAuxiliaryWindow`).
pub fn window_config_for(label: &str) -> Option<Value> {
    let guard = WINDOW_CONFIGS.read().ok()?;
    let map = guard.as_ref()?;
    if let Some(config) = map.get(label) {
        return Some(config.clone());
    }
    map.get("main").cloned()
}

/// Legacy accessor (the main window).
pub fn window_config() -> Option<Value> {
    window_config_for("main")
}

/// Store/replace the configuration of a specific window.
pub fn set_window_config(label: &str, config: Value) {
    let mut guard = WINDOW_CONFIGS.write().unwrap_or_else(|poisoned| poisoned.into_inner());
    let map = guard.get_or_insert_with(Map::new);
    map.insert(label.to_string(), config);
}

/// A fresh configuration copy for a new workbench window (workspace/folder
/// applied by the caller).
pub fn fresh_window_config() -> Value {
    window_config_for("main").unwrap_or_else(|| json!({}))
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
/// the MAIN window configuration so the next workbench boot opens them: the
/// LAST folder/workspace wins (upstream semantics — one workspace container
/// per window), files accumulate into `filesToOpenOrCreate`. Returns true
/// when the configuration changed and the window should reload.
pub fn apply_window_openables(openables: &[Value]) -> bool {
    let changed = apply_window_openables_to("main", openables);
    if changed {
        // Keep the hot-exit/session-restore tracking current.
        if let Some(mut config) = window_config_for("main") {
            if let Some(backup) = crate::window_state::backup_path_for_config(&config) {
                if let Some(target) = config.as_object_mut() {
                    target.insert("backupPath".to_string(), json!(backup));
                }
                set_window_config("main", config.clone());
            }
            crate::window_state::track_window_workspace(1, &config);
        }
    }
    changed
}

/// The label-addressed variant used for multi-window.
pub fn apply_window_openables_to(label: &str, openables: &[Value]) -> bool {
    let Some(config) = window_config_for(label) else {
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
        set_window_config(label, Value::Object(map));
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

    let mut product = read_json_file(&client_root.join("product.json")).unwrap_or_else(|| {
        crate::logger::log_app("warn", "product.json missing from client bundle; using fallback");
        json!({
            "nameShort": "Visual Studio Code",
            "nameLong": "Visual Studio Code",
            "applicationName": "Visual Studio Code",
            "dataFolderName": ".vstauri",
            "version": "1.138.0"
        })
    });
    apply_branding(&mut product);

    // OS theme detection — the workbench's autoDetectColorScheme path and
    // the pre-paint both consume this (electron-main nativeTheme parity).
    let theme = detect_theme();
    if theme.dark {
        crate::logger::log_app("info", "config: OS theme detected: dark");
    } else {
        crate::logger::log_app("info", "config: OS theme detected: light");
    }
    if theme.high_contrast {
        crate::logger::log_app("info", "config: high contrast active");
    }

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
        "colorScheme": { "dark": theme.dark, "highContrast": theme.high_contrast },
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
        "partsSplash": if theme.dark { default_parts_splash() } else { light_parts_splash() },

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

// ---------------------------------------------------------------------------
// OS theme detection (electron-main nativeTheme parity)
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug)]
pub struct DetectedTheme {
    pub dark: bool,
    pub high_contrast: bool,
}

/// Read the Windows personalization state: `AppsUseLightTheme` /
/// `SystemUsesLightTheme` (dark when 0) under
/// `HKCU\Software\Microsoft\Windows\CurrentVersion\Themes\Personalize`,
/// plus the high-contrast flag via SystemParametersInfo(SPI_GETHIGHCONTRAST).
/// Non-Windows builds fall back to dark (VS Code's historical default).
pub fn detect_theme() -> DetectedTheme {
    #[cfg(windows)]
    {
        type Hkey = *mut core::ffi::c_void;
        const HKEY_CURRENT_USER: isize = 0x8000_0001;
        const RRF_RT_REG_DWORD: u32 = 0x0000_0002;

        fn reg_dword(subkey: &str, value: &str) -> Option<u32> {
            use std::os::raw::c_void;
            unsafe {
                let mut hkey: Hkey = std::ptr::null_mut();
                let mut ty: u32 = 0;
                let mut data: u32 = 0;
                let mut size: u32 = 4;
                let subkey_w: Vec<u16> = subkey.encode_utf16().chain(std::iter::once(0)).collect();
                let value_w: Vec<u16> = value.encode_utf16().chain(std::iter::once(0)).collect();
                extern "system" {
                    fn RegOpenKeyExW(
                        hkey: isize,
                        lpsubkey: *const u16,
                        reserved: u32,
                        access: u32,
                        phkresult: *mut *mut c_void,
                    ) -> i32;
                    fn RegGetValueW(
                        hkey: isize,
                        lpsubkey: *const u16,
                        lpvalue: *const u16,
                        dwflags: u32,
                        pdwtype: *mut u32,
                        pvdata: *mut core::ffi::c_void,
                        pcbdata: *mut u32,
                    ) -> i32;
                    fn RegCloseKey(hkey: *mut c_void) -> i32;
                }
                let status = RegOpenKeyExW(
                    HKEY_CURRENT_USER,
                    subkey_w.as_ptr(),
                    0,
                    0x2001F, // KEY_READ
                    &mut hkey as *mut *mut c_void,
                );
                if status != 0 {
                    return None;
                }
                let status = RegGetValueW(
                    HKEY_CURRENT_USER,
                    std::ptr::null(),
                    value_w.as_ptr(),
                    RRF_RT_REG_DWORD,
                    &mut ty,
                    &mut data as *mut u32 as *mut core::ffi::c_void,
                    &mut size,
                );
                let _ = RegCloseKey(hkey);
                if status == 0 && ty == 4 {
                    Some(data)
                } else {
                    None
                }
            }
        }

        const SUBKEY: &str = "Software\\Microsoft\\Windows\\CurrentVersion\\Themes\\Personalize";
        let apps_light = reg_dword(SUBKEY, "AppsUseLightTheme");
        let system_light = reg_dword(SUBKEY, "SystemUsesLightTheme");
        // Electron nativeTheme.shouldUseDarkColors: dark when AppsUseLightTheme=0;
        // missing values (older Windows / server) keep the dark default.
        let dark = match (apps_light, system_light) {
            (Some(apps), _) => apps == 0,
            (None, Some(system)) => system == 0,
            (None, None) => true,
        };

        // High contrast: SystemParametersInfoW(SPI_GETHIGHCONTRAST).
        let high_contrast = unsafe {
            #[repr(C)]
            struct HighContrast {
                size: u32,
                flags: u32,
                scheme: *mut u16,
            }
            extern "system" {
                fn SystemParametersInfoW(action: u32, param: u32, pvparam: *mut core::ffi::c_void, winini: u32) -> i32;
            }
            let mut hc = HighContrast { size: std::mem::size_of::<HighContrast>() as u32, flags: 0, scheme: std::ptr::null_mut() };
            const SPI_GETHIGHCONTRAST: u32 = 0x0042;
            let ok = SystemParametersInfoW(
                SPI_GETHIGHCONTRAST,
                0,
                &mut hc as *mut HighContrast as *mut core::ffi::c_void,
                0,
            );
            ok != 0 && (hc.flags & 0x0001) != 0 // HCF_HIGHCONTRASTON — SPIF_UPDATEINIFILE unused here
        };

        return DetectedTheme { dark, high_contrast };
    }
    #[cfg(not(windows))]
    DetectedTheme { dark: true, high_contrast: false }
}

/// OS color scheme (nativeHost.getOSColorScheme) — mirrors the detected
/// theme so the workbench's auto theme switching follows the OS.
pub fn color_scheme() -> Value {
    let theme = detect_theme();
    json!({ "dark": theme.dark, "highContrast": theme.high_contrast })
}

// ---------------------------------------------------------------------------
// Branding: "VSTauri" by Mouri Younes
// ---------------------------------------------------------------------------

/// The product identity override applied to the bundled product.json so the
/// whole workbench (window title, About dialog, telemetry app name, data
/// paths) presents VSTauri instead of the upstream product identity.
fn apply_branding(product: &mut Value) {
    const NAME: &str = "VSTauri";
    const AUTHOR: &str = "Mouri Younes";
    let Some(map) = product.as_object_mut() else { return };
    map.insert("nameShort".to_string(), json!(NAME));
    map.insert("nameLong".to_string(), json!(NAME));
    map.insert("applicationName".to_string(), json!(NAME));
    map.insert("publisher".to_string(), json!(AUTHOR));
    map.insert("author".to_string(), json!(AUTHOR));
    map.insert("win32AppUserModelId".to_string(), json!("VSTauri"));
    map.insert("win32MutexName".to_string(), json!("vstauri"));
    map.insert("win32DirName".to_string(), json!("VSTauri"));
    map.insert("win32AppName".to_string(), json!("VSTauri"));
    map.insert("win32ShellNameShort".to_string(), json!("VSTauri"));
    map.insert("dataFolderName".to_string(), json!("VSTauri"));
    map.insert("reportIssueUrl".to_string(), json!("https://github.com/Zetsu4i/vscode/issues"));
    map.insert("extensionsGallery".to_string(), json!({
        "serviceUrl": "https://open-vsx.org/vscode/gallery",
        "itemUrl": "https://open-vsx.org/vscode/item",
        "resourceUrlTemplate": "https://open-vsx.org/vscode/unpkg/{publisher}/{name}/{version}",
        "controlUrl": "",
        "recommendationUrl": ""
    }));
    crate::logger::log_app(
        "info",
        "config: branded product identity (VSTauri by Mouri Younes)",
    );
}

/// The pre-paint boot payload injected as a second initialization script
/// per window: theme colors for the document-start paint + loading screen
/// data (editor name, version, attribution, window label).
pub fn boot_json_for(label: &str) -> Value {
    let theme = detect_theme();
    let product = window_config_for(label)
        .and_then(|config| config.get("product").cloned())
        .unwrap_or_else(|| json!({}));
    let version = product
        .get("version")
        .and_then(Value::as_str)
        .unwrap_or(env!("CARGO_PKG_VERSION"))
        .to_string();
    let quality = product.get("quality").and_then(Value::as_str);
    let dark = if theme.high_contrast { true } else { theme.dark };
    let (background, foreground, accent) = if dark {
        ("#1e1e1e", "#cccccc", "#007acc")
    } else {
        ("#ffffff", "#3b3b3b", "#007acc")
    };
    let mut quality_note = String::new();
    if let Some(q) = quality {
        quality_note = q.to_string();
    }
    json!({
        "windowLabel": label,
        "theme": {
            "dark": dark,
            "highContrast": theme.high_contrast,
            "background": background,
            "foreground": foreground,
            "accent": accent,
        },
        "product": {
            "nameShort": "VSTauri",
            "nameLong": "VSTauri",
            "version": version,
            "quality": quality_note,
            "by": "Mouri Younes",
        }
    })
}

/// The agents window profile (userDataProfilesMainService
/// createAgentsWindowProfile parity: a distinct profile flagged
/// isAgentsWindowProfile with its own storage under the data root).
pub fn agents_profile_json() -> Value {
    let user_dir = data_root().join("User");
    let location = user_dir.join("profiles").join("agents");
    let cache_home = data_root()
        .join("Cache")
        .join("CachedProfilesData")
        .join("agents");
    json!({
        "id": "agents-window-profile",
        "name": "Agents",
        "isDefault": false,
        "location": uri_components(&location),
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
        "isAgentsWindowProfile": true
    })
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

/// Dark_plus parts splash (IPartsSplash in themeService.ts) — colors only.
/// workbench.js paints the `initialShellColors` body background from
/// colorInfo synchronously while the module graph streams in; the branded
/// "Starting…" overlay (shim.js, from __VSTAURI_BOOT__) covers the loading
/// period and is removed when the real titlebar renders. layoutInfo is
/// deliberately omitted so no fake workbench skeleton is drawn.
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
        // NO layoutInfo: workbench.ts only draws the fake shell skeleton
        // (titlebar/activitybar/sidebar/... strips — the "fake VS Code
        // layout") when partsSplash carries layoutInfo. Shipping colors
        // only makes it apply the initialShellColors background style, so
        // the first thing the user sees is the shim's branded "Starting…"
        // splash over the correctly-themed background instead of a fake
        // layout flash.
    })
}

/// Light-mode parts splash (Default Light Modern palette — the light twin
/// of default_parts_splash, colors only). Used when the OS theme is
/// detected light so the boot background and the shim's splash match the
/// final theme instead of flashing dark.
fn light_parts_splash() -> Value {
    json!({
        "baseTheme": "vs",
        "colorInfo": {
            "background": "#ffffff",
            "foreground": "#3b3b3b",
            "editorBackground": "#ffffff",
            "editorForeground": "#3b3b3b",
            "activityBarBackground": "#2c2c2c",
            "activityBarForeground": "#ffffff",
            "titleBarBackground": "#f8f8f8",
            "titleBarForeground": "#3b3b3b",
            "statusBarBackground": "#007acc",
            "statusBarForeground": "#ffffff",
            "sideBarBackground": "#f3f3f3",
            "sideBarForeground": "#3b3b3b",
            "editorGroupBorder": "#e7e7e7",
            "editorGroupHeaderTabsBorder": "#f8f8f8",
            "panelBackground": "#ffffff"
        },
    })
}
