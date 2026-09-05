// Prevents an extra console window on Windows in release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod server;
mod state;

use state::AppState;
use tauri::{Manager, WebviewUrl, WebviewWindowBuilder};

/// Boot files the workbench page references on startup. If any of these is
/// missing from the client directory the app boots into a blank page — fail
/// here with a precise message (and a log line) instead.
const BOOT_FILES: &[&str] = &[
    "out/vs/code/browser/workbench/workbench.js",
    "out/vs/code/browser/workbench/workbench.css",
    "out/nls.messages.js",
    "product.json",
];

fn main() {
    // Headless smoke mode (CI / manual diagnostics): start ONLY the backbone,
    // print the port + token, and park. No window, no Tauri runtime.
    if std::env::args().any(|a| a == "--vstauri-smoke") {
        smoke_run();
        return;
    }

    tauri::Builder::default()
        .setup(|app| {
            // Release builds have no console — everything goes to the log.
            if let Ok(dir) = app.path().app_data_dir() {
                state::init_log(dir.join("vstauri.log"));
                state::init_last_folder(dir.join("last_folder.txt"));
            }

            // Resolve the vscode-web client directory before anything else.
            let client_dir = server::resolve_client_dir(app.handle()).ok_or_else(|| {
                "vscode-web client not found. Run scripts/build-client.sh (dev) or reinstall the app."
                    .to_string()
            })?;

            for f in BOOT_FILES {
                if !client_dir.join(f).is_file() {
                    state::log(&format!("boot: MISSING {f} in {}", client_dir.display()));
                    return Err(format!("client file missing: {f}").into());
                }
            }
            state::log(&format!("boot: client_dir={}", client_dir.display()));

            let state = AppState::new(client_dir)?;

            // Bind the backbone server on a random loopback port.
            let port = server::start(state.clone())?;
            state::log(&format!("boot: backbone bound on 127.0.0.1:{port}"));

            // The workbench is served by the backbone and talks to it over the
            // local HTTP/WS bridge. Tauri provides the native window shell.
            let url = tauri::Url::parse(&format!("http://127.0.0.1:{port}/"))
                .map_err(|e| e.to_string())?;

            let window = WebviewWindowBuilder::new(app, "main", WebviewUrl::External(url))
                .title("Code")
                .inner_size(1400.0, 900.0)
                .min_inner_size(860.0, 520.0)
                .resizable(true)
                .center()
                .build()
                .map_err(|e| e.to_string())?;

            let _ = window.set_focus();

            app.manage(state);
            Ok(())
        })
        .on_window_event(|window, event| {
            // When the last window closes, tear down terminals and exit.
            if let tauri::WindowEvent::Destroyed = event {
                if window.app_handle().webview_windows().len() == 0 {
                    if let Some(state) = window.app_handle().try_state::<AppState>() {
                        state.shutdown();
                    }
                    window.app_handle().exit(0);
                }
            }
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

/// `--vstauri-smoke`: run the backbone headless and print a readiness line.
/// Used by CI to assert (HTTP-level) that the packaged app serves the
/// workbench correctly — content types, injected config, boot files — without
/// opening a window. Exit codes: 0 = ready (killed by the caller), 2 = setup
/// failure.
fn smoke_run() {
    state::init_log(std::env::temp_dir().join("vstauri-smoke.log"));

    let client_dir = std::env::var("VSTAURI_CLIENT_DIR")
        .ok()
        .map(std::path::PathBuf::from)
        .filter(|p| p.join(server::WORKBENCH_HTML).is_file())
        .or_else(server::resolve_client_dir_generic);

    let Some(client_dir) = client_dir else {
        eprintln!("VSTAURI_SMOKE_FAIL: no client dir (set VSTAURI_CLIENT_DIR)");
        std::process::exit(2);
    };

    for f in BOOT_FILES {
        if !client_dir.join(f).is_file() {
            eprintln!("VSTAURI_SMOKE_FAIL: client file missing: {f}");
            std::process::exit(2);
        }
    }

    let state = match AppState::new(client_dir) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("VSTAURI_SMOKE_FAIL: {e}");
            std::process::exit(2);
        }
    };

    let port = match server::start(state.clone()) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("VSTAURI_SMOKE_FAIL: {e}");
            std::process::exit(2);
        }
    };

    println!("VSTAURI_SMOKE_READY port={port} token={}", state.token);
    use std::io::Write as _;
    let _ = std::io::stdout().flush();

    // Park until the caller kills us.
    loop {
        std::thread::sleep(std::time::Duration::from_secs(3600));
    }
}
