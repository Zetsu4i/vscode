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
mod encryption_channel;
mod fs_channel;
mod ipc;
mod keyboard_channel;
mod logger;
mod logger_channel;
mod native_host;
mod profiles_channel;
mod protocol;
mod shim;
mod sidecar_channel;
mod storage_channel;
mod terminal_channel;
mod util;
mod window_state;
mod windows;
mod workspaces_channel;

use serde_json::Value;

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
    // Built-in (system) extensions: theme-defaults is pure data and gives the
    // workbench its default themes/grammars through the localFilesystem
    // channel — without it the workbench renders unthemed.
    "extensions/theme-defaults/package.json",
    "extensions/theme-defaults/themes/dark_plus.json",
    // Terminal shell-integration script (injected into pwsh/bash launches,
    // see terminal_channel::shell_integration_injection).
    "out/vs/workbench/contrib/terminal/common/scripts/shellIntegration.ps1",
    // Integrated terminal renderer deps: xterm addons dynamically imported
    // as node_modules.asar/@xterm/addon-*/lib/*.js — a missing one 404s and
    // kills the terminal panel (observed in the first Windows runtime log).
    "node_modules/@xterm/addon-webgl/lib/addon-webgl.js",
    "node_modules/@xterm/addon-unicode11/lib/addon-unicode11.js",
    "node_modules/@xterm/xterm/lib/xterm.js",
    // Copilot extension (AI provider bring-up): its dist is the entry the
    // extension host loads; the BYOK providers live inside the bundle.
    "extensions/copilot/dist/extension.js",
    "extensions/copilot/package.json",
    // The Node sidecar wrapper (extension host / pty host / watcher).
    "vstauri-sidecar.mjs",
];

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.iter().any(|arg| arg == "--vstauri-smoke") {
        std::process::exit(run_smoke_mode());
    }

    let builder = tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_clipboard_manager::init())
        .on_window_event(|window, event| {
            // Kill window-bound sidecars (extension hosts, utility workers)
            // when their owning window goes away — Electron's
            // windowLifecycleBound semantics.
            if let tauri::WindowEvent::Destroyed = event {
                let label = window.label();
                if label != "main" {
                    crate::sidecar_channel::kill_window_processes(label);
                    crate::windows::unregister_window(label);
                }
            }
        })
        .register_uri_scheme_protocol("vscode-file", |ctx, request| {
            protocol::serve(ctx.app_handle(), request)
        })
        .invoke_handler(tauri::generate_handler![
            vscode_window_config,
            vscode_ipc,
            vscode_set_zoom_level,
            vscode_log,
            vscode_message_port_send,
            vscode_message_port_close
        ])
        .setup(|app| {
            // Build the window configuration (including the restored
            // session workspace + hot-exit backup path), open logs and the
            // IPC call log before the webview starts loading.
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

            // Session bounds restore (windowsState.json uiState — the same
            // data electron-main's windowsMainService reopens windows with).
            // First boot or missing state falls back to the centered default.
            let ui_state = crate::window_state::last_window_ui_state();
            let saved_bounds = ui_state.as_ref().and_then(|ui| {
                let x = ui.get("x").and_then(Value::as_i64)?;
                let y = ui.get("y").and_then(Value::as_i64)?;
                let width = ui.get("width").and_then(Value::as_u64)?;
                let height = ui.get("height").and_then(Value::as_u64)?;
                if width >= 200 && height >= 150 {
                    Some((x as f64, y as f64, width as f64, height as f64))
                } else {
                    None
                }
            });
            let maximized = ui_state
                .as_ref()
                .and_then(|ui| ui.get("maximized"))
                .and_then(Value::as_bool)
                .unwrap_or(false);

            let window = tauri::WebviewWindowBuilder::new(app, "main", url)
                .title("VSTauri")
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
                .initialization_script(format!(
                    "window.__VSTAURI_BOOT__={};window.__VSTAURI_BOOT_READY__&&window.__VSTAURI_BOOT_READY__();",
                    serde_json::to_string(&config::boot_json_for("main"))
                        .unwrap_or_else(|_| "{}".to_string())
                ))
                .build()?;

            // Saved-session bounds: applied via set_position/set_size after
            // build (the builder's position() would fight the .center() call
            // ordering above; direct APIs are unambiguous). A maximized
            // session reopens maximized.
            if let Some((x, y, width, height)) = saved_bounds {
                let _ = window.set_position(tauri::PhysicalPosition::new(
                    x.round() as i32,
                    y.round() as i32,
                ));
                let _ = window.set_size(tauri::PhysicalSize::new(
                    width.round() as u32,
                    height.round() as u32,
                ));
            }
            if maximized {
                let _ = window.maximize();
            }

            // Multi-window: window ids, NewWindowRequested (window.open) and
            // per-window lifecycle bookkeeping.
            windows::register_main_window("main");
            windows::attach_new_window_handler(app.handle(), &window);
            windows::watch_window_lifecycle_main(app.handle());

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

    let run_result = builder.run(tauri::generate_context!());
    // Kill every sidecar (extension host, pty host, workers) on the way out.
    sidecar_channel::kill_all();
    if let Err(err) = run_result {
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
    // The sidecar Node.js runtime ships next to the client tree, not in it
    // (tauri resources: resources/node/node.exe).
    let node_runtime = std::env::var("VSTAURI_CLIENT_DIR")
        .ok()
        .map(|dir| std::path::PathBuf::from(dir))
        .and_then(|client| client.parent().map(|p| p.to_path_buf()))
        .or_else(|| {
            std::env::current_exe().ok().and_then(|exe| {
                exe.parent().map(|dir| dir.join("resources").to_path_buf())
            })
        })
        .map(|resources| resources.join("node").join("node.exe"));
    match node_runtime {
        Some(path) if path.is_file() => {
            report.push_str(&format!("ok node runtime {}\n", path.display()));
        }
        other => {
            report.push_str(&format!(
                "MISSING node runtime {}\n",
                other.map(|p| p.display().to_string()).unwrap_or_default()
            ));
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
/// Returns the configuration of the CALLING window (multi-window aware).
#[tauri::command]
fn vscode_window_config(window: tauri::WebviewWindow) -> Result<serde_json::Value, String> {
    let label = window.label();
    match config::window_config_for(label) {
        Some(value) => Ok(value),
        None => match config::window_config_for("main") {
            Some(value) => Ok(value),
            None => Err("window configuration not initialized".to_string()),
        },
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
///
/// The `window` parameter is injected by Tauri as the CALLING webview —
/// every response/event routes back to that window (multi-window).
#[tauri::command]
async fn vscode_ipc(
    app: tauri::AppHandle,
    window: tauri::WebviewWindow,
    channel: String,
    args: Vec<serde_json::Value>,
    kind: String,
) -> Result<serde_json::Value, String> {
    let label = window.label().to_string();
    tauri::async_runtime::spawn_blocking(move || {
        ipc::route(&app, &label, &channel, &args, &kind)
    })
    .await
    .map_err(|err| format!("ipc task failed: {}", err))?
}

/// webFrame.setZoomLevel -> WebView2 zoom (scale = 1.2^level, identical to
/// zoomLevelToZoomFactor in src/vs/platform/window/common/window.ts).
/// Zoom applies to the CALLING window (aux windows inherit).
#[tauri::command]
fn vscode_set_zoom_level(window: tauri::WebviewWindow, level: f64) -> Result<(), String> {
    let clamped = level.clamp(-10.0, 10.0);
    let factor = 1.2f64.powf(clamped);
    window.set_zoom(factor).map_err(|err| err.to_string())
}

/// Renderer log forwarding (console.error / onerror / unhandledrejection).
#[tauri::command]
fn vscode_log(level: String, message: String) {
    logger::log_renderer(&level, &message);
}

// ---------------------------------------------------------------------------
// Virtual MessagePort plumbing (sidecar_channel)
// ---------------------------------------------------------------------------

/// The shim's fake MessagePort posted a message (base64 VSBuffer bytes) on
/// virtual port `port_id` — route it into the owning Node sidecar.
#[tauri::command]
fn vscode_message_port_send(port_id: u64, data: String) -> Result<(), String> {
    let bytes = crate::ipc::base64_decode_public(&data)
        .ok_or_else(|| "invalid base64 port message".to_string())?;
    sidecar_channel::port_message_from_renderer(port_id, &bytes);
    Ok(())
}

/// The shim closed a virtual port (port.close()).
#[tauri::command]
fn vscode_message_port_close(port_id: u64) {
    sidecar_channel::port_closed_from_renderer(port_id);
}
