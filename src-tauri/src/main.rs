// Prevents an extra console window on Windows in release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod server;
mod state;

use state::AppState;
use tauri::{Manager, WebviewUrl, WebviewWindowBuilder};

fn main() {
    tauri::Builder::default()
        .setup(|app| {
            // Resolve the vscode-web client directory before anything else.
            let client_dir = server::resolve_client_dir(app.handle()).ok_or_else(|| {
                "vscode-web client not found. Run scripts/build-client.sh (dev) or reinstall the app."
                    .to_string()
            })?;

            let state = AppState::new(client_dir)?;

            // Bind the backbone server on a random loopback port.
            let port = server::start(state.clone())?;

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
