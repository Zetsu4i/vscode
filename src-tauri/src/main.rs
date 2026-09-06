// Prevents an additional console window on Windows in release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
// The window configuration json! literal is deep enough to exhaust the
// default macro recursion limit.
#![recursion_limit = "256"]

//! VSTauri - Tauri v2 shell for Visual Studio Code (Phase 1).
//!
//! Replaces the Electron main process for the boot path only:
//!   * serves the original workbench renderer through the `vscode-file://`
//!     custom protocol (see protocol.rs),
//!   * exposes the preload compatibility shim at document-start (shim.js),
//!   * answers the `vscode_window_config` handshake with a native window
//!     configuration (config.rs),
//!   * routes and logs every ipcRenderer call (ipc.rs) - the raw data for
//!     Phase 2 IPC contract extraction,
//!   * supports a headless `--vstauri-smoke` mode used by CI to verify the
//!     bundled client without a display.
//!
//! The legacy Electron tree stays fully intact and buildable in parallel
//! (AGENTS.md constraint 5).

mod config;
mod fs_channel;
mod ipc;
mod keyboard_channel;
mod logger;
mod logger_channel;
mod native_host;
mod profiles_channel;
mod protocol;
mod shim;
mod storage_channel;
mod terminal_channel;
mod util;

use tauri::Manager;

/// Boot files verified by `--vstauri-smoke`. Keep in sync with the CI
/// assertions in .github/workflows/windows-nsis-release.yml.
const SMOKE_FILES: &[&str] = &[
    "out/vs/code/electron-browser/workbench/workbench.html",
    "out/vs/code/electron-browser/workbench/workbench.js",
    "out/vs/workbench/workbench.desktop.main.js",
    "out/vs/workbench/workbench.desktop.main.css",
    "css-modules.json",
    "out/vs/base/browser/ui/codicons/codicon/codicon.ttf",
    "node_modules/vscode-oniguruma/release/onig.wasm",
    "product.json",
    "nls.messages.json",
];

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.iter().any(|arg| arg == "--vstauri-smoke") {
        std::process::exit(run_smoke_mode());
    }

    let builder = tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_clipboard_manager::init())
        .register_uri_scheme_protocol("vscode-file", |ctx, request| {
            protocol::serve(ctx.app_handle(), request)
        })
        .invoke_handler(tauri::generate_handler![
            vscode_window_config,
            vscode_ipc,
            vscode_set_zoom_level,
            vscode_log
        ])
        .setup(|app| {
            // Build the window configuration, open logs and the IPC call log
            // before the webview starts loading the workbench.
            config::init(app.handle());

            // Same document Electron loads for the desktop workbench, served
            // natively by the vscode-file protocol handler.
            //
            // CRITICAL: the authority MUST be `localhost`. On Windows wry maps
            // custom-scheme navigations onto `http://<scheme>.<authority>` via
            // its WebResourceRequested workaround, so this document ends up at
            // origin `http://vscode-file.localhost` — the only host form Tauri
            // v2's `is_local_url` accepts as LOCAL (`http://<scheme>.localhost`).
            // With any other authority (e.g. `vscode-app`) the origin counts as
            // REMOTE and every invoke() from the preload shim is silently
            // rejected by the IPC ACL ("Command not allowed") — which is exactly
            // what produced the blank white window of the first Phase 1 build.
            let url = tauri::WebviewUrl::External(
                "vscode-file://localhost/out/vs/code/electron-browser/workbench/workbench.html"
                    .parse()
                    .map_err(|err| -> Box<dyn std::error::Error> { Box::new(err) })?,
            );

            let window = tauri::WebviewWindowBuilder::new(app, "main", url)
                .title("Visual Studio Code")
                .inner_size(1280.0, 800.0)
                .min_inner_size(320.0, 240.0)
                .center()
                // Frameless window: VS Code's default Windows experience is the
                // CUSTOM titlebar (window.titleBarStyle = "custom"), where the
                // workbench renders its own titlebar + drag region and reserves
                // `.window-controls-container` space for overlay controls. The
                // shim injects the minimize/maximize/close buttons there and
                // mirrors Electron's -webkit-app-region drag semantics onto
                // `.titlebar-drag-region` via startDragging().
                .decorations(false)
                .initialization_script(shim::SHIM_JS)
                .build()?;

            // Devtools on demand (F12 / Ctrl+Shift+I through the workbench's
            // dev keybindings -> `vscode:toggleDevTools` -> ipc.rs). Invaluable
            // for bring-up on machines without a debugger attached.
            if std::env::var("VSTAURI_DEVTOOLS").map(|v| v == "1").unwrap_or(false) {
                window.open_devtools();
            }

            ipc::init_dispatch(app.handle().clone());

            logger::log_app("info", "main window created; workbench loading via vscode-file://localhost");
            Ok(())
        });

    if let Err(err) = builder.run(tauri::generate_context!()) {
        logger::log_app("error", &format!("failed to run tauri application: {}", err));
        std::process::exit(1);
    }
}

/// Headless verification mode: checks that the bundled client contains every
/// boot file and writes a `vstauri-smoke.out` report next to the current
/// working directory. Returns the process exit code.
fn run_smoke_mode() -> i32 {
    let client_root = std::env::var("VSTAURI_CLIENT_DIR")
        .ok()
        .filter(|dir| !dir.is_empty())
        .map(std::path::PathBuf::from)
        .or_else(|| {
            std::env::current_exe().ok().and_then(|exe| {
                exe.parent()
                    .map(|dir| dir.join("resources").join("client"))
            })
        })
        .unwrap_or_else(|| std::path::PathBuf::from("."));

    let mut report = String::from("VSTAURI_SMOKE_READY\n");
    let mut all_ok = true;
    for file in SMOKE_FILES {
        if client_root.join(file).is_file() {
            report.push_str(&format!("ok {}\n", file));
        } else {
            report.push_str(&format!("MISSING {}\n", file));
            all_ok = false;
        }
    }
    report.push_str(if all_ok { "SMOKE OK\n" } else { "SMOKE FAILED\n" });

    let marker = std::env::current_dir()
        .unwrap_or_else(|_| std::path::PathBuf::from("."))
        .join("vstauri-smoke.out");
    match std::fs::write(&marker, &report) {
        Ok(()) => {
            if all_ok {
                0
            } else {
                1
            }
        }
        Err(_) => 2,
    }
}

// ---------------------------------------------------------------------------
// Tauri commands (invoked from shim.js through window.__TAURI_INTERNALS__)
// ---------------------------------------------------------------------------

/// The `--vscode-window-config` IPC handshake replacement: everything the
/// workbench needs to boot (product config, NLS, paths, environment).
#[tauri::command]
fn vscode_window_config() -> Result<serde_json::Value, String> {
    match config::window_config() {
        Some(value) => Ok(value),
        None => Err("window configuration not initialized".to_string()),
    }
}

/// ipcRenderer.send / ipcRenderer.invoke routing with contract logging. The
/// `vscode:message` channel carries a base64-encoded binary protocol frame
/// in `args[0]` (see ipc.rs — the main-process message protocol).
///
/// Async + `spawn_blocking`: the routed commands may open NATIVE MODAL
/// dialogs (nativeHost showSaveDialog / showOpenDialog / pick*AndOpen via
/// tauri-plugin-dialog's blocking API). Those must never run on the main
/// thread (deadlock with the Windows message loop) nor block an async
/// runtime worker — the blocking pool is the right home for them.
#[tauri::command]
async fn vscode_ipc(
    app: tauri::AppHandle,
    channel: String,
    args: Vec<serde_json::Value>,
    kind: String,
) -> Result<serde_json::Value, String> {
    tauri::async_runtime::spawn_blocking(move || ipc::route(&app, &channel, &args, &kind))
        .await
        .map_err(|err| format!("ipc task failed: {}", err))?
}

/// webFrame.setZoomLevel -> WebView2 zoom (scale = 1.2^level, identical to
/// zoomLevelToZoomFactor in src/vs/platform/window/common/window.ts).
#[tauri::command]
fn vscode_set_zoom_level(app: tauri::AppHandle, level: f64) -> Result<(), String> {
    let clamped = level.clamp(-10.0, 10.0);
    let factor = 1.2f64.powf(clamped);
    match app.get_webview_window("main") {
        Some(window) => window.set_zoom(factor).map_err(|err| err.to_string()),
        None => Ok(()),
    }
}

/// Renderer log forwarding (console.error / onerror / unhandledrejection).
#[tauri::command]
fn vscode_log(level: String, message: String) {
    logger::log_renderer(&level, &message);
}
