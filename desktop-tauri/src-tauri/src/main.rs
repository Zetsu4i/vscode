/*---------------------------------------------------------------------------------------------
 *  Copyright (c) Microsoft Corporation. All rights reserved.
 *  Licensed under the MIT License. See License.txt in the project root for license information.
 *--------------------------------------------------------------------------------------------*/

//! Code - OSS desktop shell built on Tauri.
//!
//! This binary replaces the Electron main process (`src/vs/code/electron-main`):
//! it creates the native window via the OS webview, manages the VS Code server
//! sidecar (which provides the Node.js extension host, terminals and filesystem
//! access) and enforces single-instance behaviour. The workbench UI itself is
//! served unmodified by the sidecar, so every VS Code feature and marketplace
//! extension keeps working.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod server;

use std::path::PathBuf;
use std::sync::Mutex;

use tauri::{AppHandle, Manager, RunEvent};

/// Holds the sidecar handle so we can terminate it when the app exits.
struct ServerState(Mutex<Option<server::ServerHandle>>);

fn main() {
	env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

	// A folder passed on the command line is opened in the workbench
	// (equivalent to `code <folder>` with the Electron build).
	let folder_arg = std::env::args().nth(1).and_then(|arg| {
		let path = PathBuf::from(&arg);
		if path.is_dir() {
			path.canonicalize().ok()
		} else {
			None
		}
	});

	tauri::Builder::default()
		.plugin(tauri_plugin_single_instance::init(|app, _argv, _cwd| {
			// Second launch: focus the existing window instead of starting
			// another server (mirrors Electron's requestSingleInstanceLock).
			if let Some(window) = app.get_webview_window("main") {
				let _ = window.unminimize();
				let _ = window.set_focus();
			}
		}))
		.manage(ServerState(Mutex::new(None)))
		.setup(move |app| {
			let handle = app.handle().clone();
			let folder = folder_arg.clone();

			// Boot the sidecar off the main thread; the splash screen
			// (bundled frontend) is visible in the meantime.
			std::thread::spawn(move || match server::start(&handle) {
				Ok((server_handle, mut url)) => {
					*handle.state::<ServerState>().0.lock().unwrap() = Some(server_handle);

					if let Some(folder) = folder {
						url.query_pairs_mut()
							.append_pair("folder", &folder.to_string_lossy());
					}

					log::info!("workbench ready at {url}");
					if let Some(mut window) = handle.get_webview_window("main") {
						if let Err(e) = window.navigate(url) {
							log::error!("failed to navigate to workbench: {e}");
							show_error(&handle, &format!("Failed to open the workbench: {e}"));
						}
					}
				}
				Err(message) => {
					log::error!("{message}");
					show_error(&handle, &message);
				}
			});

			Ok(())
		})
		.build(tauri::generate_context!())
		.expect("failed to build the Tauri application")
		.run(|app, event| {
			if let RunEvent::Exit = event {
				// Tear down the server and everything it spawned.
				if let Some(mut server_handle) =
					app.state::<ServerState>().0.lock().unwrap().take()
				{
					server_handle.shutdown();
				}
			}
		});
}

/// Surface a startup error on the splash screen (which defines `showError`).
fn show_error(app: &AppHandle, message: &str) {
	if let Some(window) = app.get_webview_window("main") {
		let payload = serde_json::to_string(message).unwrap_or_else(|_| "\"unknown error\"".into());
		let _ = window.eval(&format!(
			"window.showError ? window.showError({payload}) : alert({payload})"
		));
	}
}
