/*---------------------------------------------------------------------------------------------
 *  Tauri v2 shell for the VS Code workbench (Phase 1 prototype).
 *
 *  Boot flow implemented here:
 *    1. Discover the VS Code build output (`out/`, produced by `npm run compile`).
 *    2. Serve it on a loopback-only static HTTP server (WebView2 loads workbench.html
 *       from it; mirrors how Electron loads it from disk in development).
 *    3. Create the main window with an initialization script (`shim.js`) that reproduces
 *       the globals Electron's preload would expose (`window.vscode`, `process`, ...).
 *    4. Answer the handful of IPC channels needed to reach first render.
 *
 *  Everything else in the original Electron/Node tree is untouched.
 *--------------------------------------------------------------------------------------------*/

mod ipc;
mod server;
mod shim;

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use tauri::{Manager, WebviewUrl, WebviewWindowBuilder};

/// Shared shell state available to all Tauri IPC commands.
pub struct ShellState {
	/// Repository root, i.e. the parent of `out/` (VS Code's `appRoot`).
	pub app_root: PathBuf,
	/// The `out/` build directory, if it was found.
	pub out_dir: Option<PathBuf>,
	/// Port of the loopback static server.
	pub port: u16,
	/// IPC channels we have already logged as unimplemented (log-once bookkeeping).
	pub ipc_seen: Mutex<HashSet<String>>,
}

/// Where to look for the VS Code compile output.
fn discover_out_dir() -> Option<PathBuf> {
	if let Ok(from_env) = std::env::var("VSCODE_TAURI_OUT_DIR") {
		let p = PathBuf::from(from_env);
		if has_workbench(&p) {
			return Some(p);
		}
		log::warn!("[shell] VSCODE_TAURI_OUT_DIR does not contain the workbench build, ignoring it");
	}

	// `cargo run` executes with cwd = src-tauri, so the repo root is one level up.
	let cwd = std::env::current_dir().ok();
	let exe_ancestor = std::env::current_exe()
		.ok()
		.and_then(|exe| exe.parent())
		.and_then(Path::parent);
	let candidates: [Option<&Path>; 3] = [
		cwd.as_deref().and_then(Path::parent),
		cwd.as_deref(),
		exe_ancestor,
	];

	for candidate in candidates.into_iter().flatten() {
		let out = candidate.join("out");
		if has_workbench(&out) {
			return Some(out);
		}
	}

	None
}

/// The compile output is usable when the workbench entry page exists in it.
fn has_workbench(out_dir: &Path) -> bool {
	out_dir
		.join("vs/code/electron-browser/workbench/workbench.html")
		.is_file()
}

#[tauri::command]
async fn ipc_invoke(
	state: tauri::State<'_, ShellState>,
	channel: String,
	args: Option<serde_json::Value>,
) -> Result<serde_json::Value, String> {
	ipc::handle_invoke(&state, &channel, args.as_ref())
}

#[tauri::command]
async fn ipc_send(
	state: tauri::State<'_, ShellState>,
	channel: String,
	args: Option<serde_json::Value>,
) -> Result<(), String> {
	ipc::handle_send(&state, &channel, args.as_ref())
}

#[tauri::command]
fn native_set_zoom_level(window: tauri::WebviewWindow, level: f64) -> Result<(), String> {
	// Mirrors `zoomLevelToZoomFactor` from src/vs/platform/window/common/window.ts.
	let factor = 1.2_f64.powf(level);
	window.set_zoom(factor).map_err(|e| e.to_string())
}

#[tauri::command]
fn renderer_log(level: String, message: String) {
	// Keep log lines bounded; renderer messages must never dump env/token data.
	let bounded: String = message.chars().take(2_000).collect();
	match level.as_str() {
		"error" => log::error!("[renderer] {bounded}"),
		"warn" => log::warn!("[renderer] {bounded}"),
		_ => log::info!("[renderer] {bounded}"),
	}
}

pub fn run() {
	env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

	let out_dir = discover_out_dir();
	if out_dir.is_none() {
		log::error!(
			"[shell] no VS Code build output found - run `npm ci && npm run compile` in the repo root first; the window will show a diagnostic page"
		);
	}
	let app_root = out_dir
		.as_ref()
		.and_then(|p| p.parent())
		.map(Path::to_path_buf)
		.unwrap_or_else(|| PathBuf::from("."));

	// The server also serves the diagnostic page when `out/` is missing, so it must
	// always be running before the window loads.
	let server = match server::start(out_dir.as_deref()) {
		Ok(server) => server,
		Err(error) => {
			log::error!("[shell] failed to start static server: {error}");
			return;
		}
	};

	log::info!("[shell] serving workbench on http://127.0.0.1:{} (loopback only)", server.port);

	let state = ShellState {
		app_root,
		out_dir,
		port: server.port,
		ipc_seen: Mutex::new(HashSet::new()),
	};

	let window_url = format!("http://127.0.0.1:{}/", state.port);
	let shim_script = shim::render(&state);

	let run_result = tauri::Builder::default()
		.setup(move |app| {
			let url = window_url
				.parse()
				.map_err(|error| format!("invalid workbench URL `{window_url}`: {error}"))?;

			WebviewWindowBuilder::new(app, "main", WebviewUrl::External(url))
				.title("Visual Studio Code")
				.inner_size(1280.0, 800.0)
				.min_inner_size(640.0, 480.0)
				.initialization_script(shim_script.as_str())
				.build()
				.map_err(|error| format!("failed to create main window: {error}"))?;

			app.manage(state);
			log::info!("[shell] main window created");
			Ok(())
		})
		.invoke_handler(tauri::generate_handler![
			ipc_invoke,
			ipc_send,
			native_set_zoom_level,
			renderer_log
		])
		.run(tauri::generate_context!());

	if let Err(error) = run_result {
		log::error!("[shell] fatal: {error}");
		eprintln!("vscode-tauri: {error}");
		std::process::exit(1);
	}
}
