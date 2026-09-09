//! Multi-window management (Phase 3 window service + auxiliary windows).
//!
//! Electron's window model:
//!   * **Workbench windows** (`windowsMainService.open`) — full VS Code
//!     windows with their own `INativeWindowConfiguration` (workspace /
//!     folder / files / profile), opened by `openWindow(forceNewWindow)`,
//!     the agents window and (later) CLI paths.
//!   * **Auxiliary windows** — `window.open('about:blank', features)`
//!     popups the workbench creates for editors/chat drag-out. Electron's
//!     main process intercepts them via `setWindowOpenHandler` and returns a
//!     real `BrowserWindow`, giving the renderer a genuine same-origin
//!     cross-window DOM reference.
//!
//! The Tauri equivalents:
//!   * Workbench windows: `WebviewWindowBuilder` with the workbench URL, the
//!     preload shim as initialization script and a per-window window
//!     configuration (see config::set_window_config).
//!   * Auxiliary windows: WebView2's `NewWindowRequested` COM event. The
//!     handler marks the request handled, creates a Tauri window, and feeds
//!     its `ICoreWebView2` core back through `SetNewWindow` — the popup then
//!     IS our window, with the shim injected and real `window.opener` /
//!     cross-window DOM semantics (the closest possible match to Electron).
//!
//! Window ids: VS Code identifies windows numerically (`windowId`, starting
//! at 1). `windowsMainService` allocates them; the registry here does the
//! same, mapping ids <-> Tauri labels and routing
//! `vscode:registerAuxiliaryWindow`.

use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::{LazyLock, Mutex};

use tauri::Manager;

/// Tauri label -> window registration (registry bookkeeping; the id/aux/
/// parent fields are read by the window service in later phases).
#[allow(dead_code)]
struct WindowInfo {
    window_id: i64,
    is_aux: bool,
    /// Parent window id for auxiliary windows (0 for top-level).
    parent: i64,
}

static WINDOW_REGISTRY: LazyLock<Mutex<HashMap<String, WindowInfo>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));
static NEXT_WINDOW_ID: AtomicI64 = AtomicI64::new(2); // 1 = main
static NEXT_AUX: AtomicI64 = AtomicI64::new(1);
static NEXT_WIN: AtomicI64 = AtomicI64::new(1);

// ---------------------------------------------------------------------------
// Registry
// ---------------------------------------------------------------------------

/// Register the main window at setup (window id 1).
pub fn register_main_window(label: &str) {
    let mut guard = WINDOW_REGISTRY.lock().unwrap_or_else(|p| p.into_inner());
    guard.insert(
        label.to_string(),
        WindowInfo { window_id: 1, is_aux: false, parent: 0 },
    );
}

/// An auxiliary window checked in through
/// `vscode:registerAuxiliaryWindow` — returns its window id.
pub fn register_auxiliary_window(label: &str, parent_window_id: i64) -> i64 {
    let id = NEXT_WINDOW_ID.fetch_add(1, Ordering::SeqCst);
    let mut guard = WINDOW_REGISTRY.lock().unwrap_or_else(|p| p.into_inner());
    guard.insert(
        label.to_string(),
        WindowInfo { window_id: id, is_aux: true, parent: parent_window_id },
    );
    crate::logger::log_app(
        "info",
        &format!("windows: auxiliary window '{}' registered as window id {} (parent {})", label, id, parent_window_id),
    );
    id
}

/// VS Code numeric window id for a Tauri label (1 when unknown).
pub fn window_id_for(label: &str) -> i64 {
    let label = if label.is_empty() { "main" } else { label };
    WINDOW_REGISTRY
        .lock()
        .map(|guard| {
            guard
                .get(label)
                .map(|info| info.window_id)
                .unwrap_or(1)
        })
        .unwrap_or(1)
}

/// The Tauri label for a VS Code numeric window id.
pub fn label_for_window_id(window_id: i64) -> String {
    if window_id <= 1 {
        return "main".to_string();
    }
    WINDOW_REGISTRY
        .lock()
        .map(|guard| {
            guard
                .iter()
                .find(|(_, info)| info.window_id == window_id)
                .map(|(label, _)| label.clone())
                .unwrap_or_else(|| "main".to_string())
        })
        .unwrap_or_else(|_| "main".to_string())
}

/// Deregister a window (close).
pub fn unregister_window(label: &str) {
    let mut guard = WINDOW_REGISTRY.lock().unwrap_or_else(|p| p.into_inner());
    guard.remove(label);
}

// ---------------------------------------------------------------------------
// Workbench window creation (openWindow forceNewWindow / agents window)
// ---------------------------------------------------------------------------

/// Open a new full workbench window.
///
/// `openables`: IWindowOpenable[] (folderUri / workspaceUri / fileUri) —
/// the LAST folder/workspace wins, files accumulate.
/// `agents`: marks the window as the agents/sessions window
/// (isSessionsWindow + agents profile + the agent-sessions workspace).
pub fn open_workbench_window(
    app: &tauri::AppHandle,
    openables: &[Value],
    agents: bool,
) -> Result<String, String> {
    let window_id = NEXT_WINDOW_ID.fetch_add(1, Ordering::SeqCst);
    let label = if agents {
        format!("agents-{}", NEXT_AUX.fetch_add(1, Ordering::SeqCst))
    } else {
        format!("win-{}", NEXT_WIN.fetch_add(1, Ordering::SeqCst))
    };

    // Per-window boot configuration, derived from the main template.
    let mut config = crate::config::fresh_window_config();

    if agents {
        // The agents window: the shared agent-sessions workspace + the
        // dedicated agents window profile (windowsMainService parity).
        let workspace_path = crate::config::data_root().join("agent-sessions.code-workspace");
        if !workspace_path.is_file() {
            let _ = std::fs::write(&workspace_path, "{\n\t\"folders\": []\n}\n");
        }
        let workspace_uri = crate::config::uri_json(&workspace_path);
        let fs_path = workspace_path.to_string_lossy().to_lowercase();
        config["workspace"] = json!({
            "id": crate::util::md5_hex(&fs_path),
            "configPath": workspace_uri,
        });
        config["folderUri"] = Value::Null;
        config["isSessionsWindow"] = json!(true);
        // Agents profile (isAgentsWindowProfile parity).
        config["profiles"]["profile"] = crate::config::agents_profile_json();
    } else {
        // Apply the openables to the fresh config (last folder/workspace
        // wins, files accumulate — apply_window_openables semantics).
        let mut map = match config {
            Value::Object(map) => map,
            _ => return Err("window configuration is not an object".to_string()),
        };
        let mut changed = false;
        for openable in openables {
            if let Some(file_uri) = openable.get("fileUri") {
                let files = map
                    .entry("filesToOpenOrCreate")
                    .or_insert_with(|| json!([]));
                if let Some(list) = files.as_array_mut() {
                    let mut entry = serde_json::Map::new();
                    entry.insert("fileUri".to_string(), file_uri.clone());
                    if let Some(line) = openable.get("line") {
                        entry.insert("line".to_string(), line.clone());
                    }
                    if let Some(column) = openable.get("column") {
                        entry.insert("column".to_string(), column.clone());
                    }
                    list.push(Value::Object(entry));
                    changed = true;
                }
            } else if let Some(folder_uri) = openable.get("folderUri") {
                map.insert("folderUri".to_string(), folder_uri.clone());
                map.insert("workspace".to_string(), Value::Null);
                changed = true;
            } else if let Some(workspace_uri) = openable.get("workspaceUri") {
                let fs_path = crate::util::percent_decode(
                    workspace_uri.get("path").and_then(Value::as_str).unwrap_or(""),
                );
                let fs_path = if cfg!(windows) {
                    fs_path.trim_start_matches('/').replace('/', "\\")
                } else {
                    fs_path
                };
                let id_input = if cfg!(target_os = "linux") { fs_path.clone() } else { fs_path.to_lowercase() };
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
        let _ = changed;
        config = Value::Object(map);
    }

    // Per-window fields.
    config["windowId"] = json!(window_id);
    // A restored (not initial) startup for secondary windows.
    config["isInitialStartup"] = json!(false);
    // Hot-exit support: a backup path per window/workspace.
    if let Some(backup) = crate::window_state::backup_path_for_config(&config) {
        config["backupPath"] = json!(backup);
    }

    crate::config::set_window_config(&label, config);
    // Close-time persistence (windowsState.json) reads this entry.
    if let Some(config) = crate::config::window_config_for(&label) {
        crate::window_state::track_window_workspace(window_id, &config);
    }

    let url = match "vscode-file://localhost/out/vs/code/electron-browser/workbench/workbench.html"
        .parse::<tauri::Url>()
    {
        Ok(parsed) => tauri::WebviewUrl::External(parsed),
        Err(err) => return Err(err.to_string()),
    };

    let boot = crate::config::boot_json_for(&label);
    let window = tauri::WebviewWindowBuilder::new(app, label.clone(), url)
        .title(if agents { "VSTauri — Agents" } else { "VSTauri" })
        .inner_size(1280.0, 800.0)
        .min_inner_size(320.0, 240.0)
        .center()
        .decorations(false)
        .initialization_script(crate::shim::SHIM_JS)
        .initialization_script(format!(
            "window.__VSTAURI_BOOT__={};window.__VSTAURI_BOOT_READY__&&window.__VSTAURI_BOOT_READY__();",
            serde_json::to_string(&boot).unwrap_or_else(|_| "{}".to_string())
        ))
        .build()
        .map_err(|err| err.to_string())?;

    if std::env::var("VSTAURI_DEVTOOLS").map(|v| v == "1").unwrap_or(false) {
        window.open_devtools();
    }

    attach_new_window_handler(app, &window);
    watch_window_lifecycle(app, &label, window_id);

    crate::logger::log_app(
        "info",
        &format!("windows: opened workbench window '{}' (id {})", label, window_id),
    );
    Ok(label)
}

/// Window lifecycle bookkeeping: sidecar cleanup + window-state persistence.
fn watch_window_lifecycle(app: &tauri::AppHandle, label: &str, window_id: i64) {
    let label_owned = label.to_string();
    let app = app.clone();
    if let Some(window) = app.get_webview_window(label) {
        let window_handle = window.clone();
        window.on_window_event(move |event| {
            match event {
                // Bounds are captured while the window still exists
                // (Destroyed is too late to query position/size).
                tauri::WindowEvent::CloseRequested { .. } => {
                    let x = window_handle.outer_position().map(|p| p.x).unwrap_or(0);
                    let y = window_handle.outer_position().map(|p| p.y).unwrap_or(0);
                    let size = window_handle.inner_size().unwrap_or_default();
                    let maximized = window_handle.is_maximized().unwrap_or(false);
                    let fullscreen = window_handle.is_fullscreen().unwrap_or(false);
                    crate::window_state::update_ui_state(
                        window_id,
                        x,
                        y,
                        size.width,
                        size.height,
                        maximized,
                        fullscreen,
                    );
                }
                tauri::WindowEvent::Destroyed => {
                    crate::sidecar_channel::kill_window_processes(&label_owned);
                    crate::window_state::on_window_closed(&label_owned, window_id);
                    unregister_window(&label_owned);
                }
                _ => {}
            }
        });
    }
}

// ---------------------------------------------------------------------------
// Auxiliary windows (window.open -> WebView2 NewWindowRequested)
// ---------------------------------------------------------------------------

/// Attach the NewWindowRequested handler to a window so `window.open`
/// popups created by the workbench become real Tauri windows with the shim
/// (Electron `setWindowOpenHandler` parity).
pub fn attach_new_window_handler(app: &tauri::AppHandle, window: &tauri::WebviewWindow) {
    #[cfg(windows)]
    {
        let app = app.clone();
        let label = window.label().to_string();
        let _ = window.with_webview(move |webview| {
            unsafe {
                let controller: webview2_com::Microsoft::Web::WebView2::Win32::ICoreWebView2Controller =
                    webview.controller();
                let Ok(core) = controller.CoreWebView2() else {
                    crate::logger::log_app("warn", "windows: no CoreWebView2 for NewWindowRequested");
                    return;
                };
                use webview2_com::Microsoft::Web::WebView2::Win32::{
                    ICoreWebView2NewWindowRequestedEventHandler,
                };

                let handler: ICoreWebView2NewWindowRequestedEventHandler =
                    NewWindowHandler { app: app.clone(), opener_label: label.clone() }.into();
                // webview2-com 0.38: the registration token is a plain i64
                // (no EventRegistrationToken type in windows 0.61).
                let mut token: i64 = 0;
                let result = core.add_NewWindowRequested(&handler, &mut token);
                match result {
                    Ok(()) => {
                        crate::logger::log_app(
                            "info",
                            &format!("windows: NewWindowRequested handler attached to '{}'", label),
                        );
                    }
                    Err(err) => {
                        crate::logger::log_app(
                            "warn",
                            &format!("windows: NewWindowRequested attach failed: {}", err),
                        );
                    }
                }
            }
        });
    }
    #[cfg(not(windows))]
    {
        let _ = (app, window);
    }
}

#[cfg(windows)]
mod aux_com {
    use webview2_com::Microsoft::Web::WebView2::Win32::{
        ICoreWebView2, ICoreWebView2Controller, ICoreWebView2NewWindowRequestedEventArgs,
        ICoreWebView2NewWindowRequestedEventHandler,
        ICoreWebView2NewWindowRequestedEventHandler_Impl,
    };
    use windows::core::implement;
    use super::unregister_window;

    /// The NewWindowRequested COM callback. Marks the request handled, then
    /// (through the deferral API, since Tauri window creation is async)
    /// creates the popup window and hands its CoreWebView2 to the args.
    #[implement(ICoreWebView2NewWindowRequestedEventHandler)]
    pub struct NewWindowHandler {
        pub app: tauri::AppHandle,
        pub opener_label: String,
    }

    #[allow(non_snake_case)]
    impl ICoreWebView2NewWindowRequestedEventHandler_Impl for NewWindowHandler_Impl {
        fn Invoke(
            &self,
            _sender: windows_core::Ref<'_, ICoreWebView2>,
            args: windows_core::Ref<'_, ICoreWebView2NewWindowRequestedEventArgs>,
        ) -> windows::core::Result<()> {
            let Some(args) = args.as_ref() else {
                return Ok(());
            };
            let app = self.app.clone();

            unsafe {
                // 1. Take over the request before WebView2 routes it to the
                //    default browser.
                args.SetHandled(true)?;

                // 2. Deferral: we create the Tauri window asynchronously.
                //    The COM pointers below only TRAVEL through the worker
                //    thread — every actual COM call happens inside
                //    with_webview, which Tauri dispatches onto the main/UI
                //    thread (the COM apartment the interfaces belong to).
                //    SendCom asserts exactly that single-threaded handoff.
                struct SendCom<T>(T);
                unsafe impl<T> Send for SendCom<T> {}
                impl<T> SendCom<T> {
                    // Method (not field) access: closures calling this capture
                    // the whole SendCom (RFC 2229 disjoint captures would
                    // otherwise capture the raw non-Send interface field).
                    fn com(&self) -> &T {
                        &self.0
                    }
                }

                let deferral = args.GetDeferral()?;

                // 3. Bounds from the features (popup=yes,left,top,width,height).
                //    WindowFeatures() lives on the base args interface in
                //    webview2-com 0.38 and its getters write through out
                //    params (BOOL/u32 pointers).
                let mut bounds: Option<(i32, i32, u32, u32)> = None;
                if let Ok(features) = args.WindowFeatures() {
                    let mut has_pos = windows_core::BOOL::default();
                    let mut has_size = windows_core::BOOL::default();
                    let pos_ok = features.HasPosition(&mut has_pos).is_ok();
                    let size_ok = features.HasSize(&mut has_size).is_ok();
                    if (!pos_ok || has_pos.as_bool()) && (!size_ok || has_size.as_bool()) {
                        let mut left: u32 = 100;
                        let mut top: u32 = 100;
                        let mut width: u32 = 800;
                        let mut height: u32 = 600;
                        let _ = features.Left(&mut left);
                        let _ = features.Top(&mut top);
                        let _ = features.Width(&mut width);
                        let _ = features.Height(&mut height);
                        bounds = Some((
                            left as i32,
                            top as i32,
                            width.max(120),
                            height.max(90),
                        ));
                    }
                }

                let deferral = SendCom(deferral);
                let args = SendCom(args.clone());

                // 4. Create the window on a worker thread (Tauri's builder
                //    dispatches creation onto the main loop; the deferral
                //    keeps WebView2 waiting until we hand over the core).
                std::thread::spawn(move || {
                    let n = NEXT_AUX_LABEL.with(|counter| {
                        let n = counter.get() + 1;
                        counter.set(n);
                        n
                    });
                    let label = format!("aux-{}", n);
                    let url = tauri::WebviewUrl::External(
                        "vscode-file://localhost/vstauri-aux"
                            .parse()
                            .expect("static aux url parses"),
                    );
                    let boot = crate::config::boot_json_for(&label);
                    let builder = tauri::WebviewWindowBuilder::new(
                        &app,
                        label.clone(),
                        url,
                    )
                        .title("VSTauri")
                        .decorations(false)
                        .initialization_script(crate::shim::SHIM_JS)
                        .initialization_script(format!(
                            "window.__VSTAURI_BOOT__={};window.__VSTAURI_BOOT_READY__&&window.__VSTAURI_BOOT_READY__();",
                            serde_json::to_string(&boot).unwrap_or_else(|_| "{}".to_string())
                        ));
                    let builder = match bounds {
                        Some((x, y, w, h)) => builder
                            .position(x as f64, y as f64)
                            .inner_size(w as f64, h as f64),
                        None => builder.inner_size(800.0, 600.0),
                    };

                    match builder.build() {
                        Ok(popup) => {
                            // 5. Give the popup's core to WebView2 and close
                            //    out the deferral on the main thread.
                            let label_for_log = label.clone();
                            let _ = popup.with_webview(move |webview| {
                                let controller: ICoreWebView2Controller = webview.controller();
                                if let Ok(core) = controller.CoreWebView2() {
                                    let _ = args.com().SetNewWindow(&core);
                                }
                                let _ = deferral.com().Complete();
                                crate::logger::log_app(
                                    "info",
                                    &format!("windows: auxiliary window '{}' opened", label_for_log),
                                );
                            });
                            // Lifecycle bookkeeping for the popup.
                            let app_clone = app.clone();
                            let label_clone = label.clone();
                            std::thread::spawn(move || {
                                // register_auxiliary_window is invoked by the
                                // shim via vscode:registerAuxiliaryWindow; here
                                // we only watch for destroy -> sidecar cleanup.
                                if let Some(window) = app_clone.get_webview_window(&label_clone) {
                                    let label_clone2 = label_clone.clone();
                                    window.on_window_event(move |event| {
                                        if let tauri::WindowEvent::Destroyed = event {
                                            crate::sidecar_channel::kill_window_processes(
                                                &label_clone2,
                                            );
                                            unregister_window(&label_clone2);
                                        }
                                    });
                                }
                            });
                        }
                        Err(err) => {
                            crate::logger::log_app(
                                "error",
                                &format!("windows: auxiliary window creation failed: {}", err),
                            );
                            // Best effort: unblock the deferred request (the
                            // WebView2 popup then gets a canceled navigation,
                            // which the workbench tolerates).
                            let _ = deferral.com().Complete();
                        }
                    }
                });
            }
            Ok(())
        }
    }

    use std::cell::Cell;
    use tauri::Manager;
    thread_local! {
        static NEXT_AUX_LABEL: Cell<i64> = const { Cell::new(0) };
    }
}

#[cfg(windows)]
use aux_com::NewWindowHandler;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_roundtrip() {
        register_main_window("main");
        assert_eq!(window_id_for("main"), 1);
        let id = register_auxiliary_window("aux-1", 1);
        assert_eq!(label_for_window_id(id), "aux-1");
        assert!(id > 1);
        unregister_window("aux-1");
        assert_eq!(window_id_for("aux-1"), 1);
    }
}

/// Main-window lifecycle bookkeeping (close -> persist session state).
pub fn watch_window_lifecycle_main(app: &tauri::AppHandle) {
    let label = "main".to_string();
    let app = app.clone();
    if let Some(window) = app.get_webview_window("main") {
        let window_handle = window.clone();
        window.on_window_event(move |event| {
            match event {
                tauri::WindowEvent::CloseRequested { .. } => {
                    let x = window_handle.outer_position().map(|p| p.x).unwrap_or(0);
                    let y = window_handle.outer_position().map(|p| p.y).unwrap_or(0);
                    let size = window_handle.inner_size().unwrap_or_default();
                    let maximized = window_handle.is_maximized().unwrap_or(false);
                    let fullscreen = window_handle.is_fullscreen().unwrap_or(false);
                    crate::window_state::update_ui_state(
                        1,
                        x,
                        y,
                        size.width,
                        size.height,
                        maximized,
                        fullscreen,
                    );
                }
                tauri::WindowEvent::Destroyed => {
                    crate::sidecar_channel::kill_window_processes(&label);
                    crate::window_state::on_window_closed(&label, 1);
                }
                _ => {}
            }
        });
    }
}
