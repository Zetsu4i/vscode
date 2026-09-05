// Hide the console window on Windows release builds, matching Electron.
#![cfg_attr(all(not(debug_assertions), target_os = "windows"), windows_subsystem = "windows")]

//! VS Code Tauri shell — process entry point.
//!
//! Replaces `src/vs/code/electron-main/main.ts` + `app.ts` for the shell
//! concerns only. The renderer it hosts is the untouched VS Code desktop
//! workbench bundle.

use std::path::PathBuf;
use std::sync::Arc;

use serde_json::Value;
use tauri::{Manager, State, WebviewUrl, WebviewWindowBuilder};

use vscode_shell::{ipc, protocol, self_check, shell_env, ShellState};

struct AppState {
	shell: Arc<ShellState>,
}

#[derive(Default)]
struct Cli {
	self_check: bool,
	workbench_dir: Option<PathBuf>,
	folder: Option<String>,
}

fn parse_cli() -> Cli {
	let mut cli = Cli::default();
	let mut args = std::env::args().skip(1);
	while let Some(arg) = args.next() {
		match arg.as_str() {
			"--self-check" => cli.self_check = true,
			"--workbench-dir" => cli.workbench_dir = args.next().map(PathBuf::from),
			"--folder" => cli.folder = args.next(),
			other if other.starts_with("--workbench-dir=") => {
				cli.workbench_dir = other.split_once('=').map(|(_, v)| PathBuf::from(v));
			}
			other if other.starts_with("--folder=") => {
				cli.folder = other.split_once('=').map(|(_, v)| v.to_string());
			}
			other if !other.starts_with('-') && cli.folder.is_none() => {
				cli.folder = Some(other.to_string());
			}
			_ => {}
		}
	}
	cli
}

// ---------------------------------------------------------------------------
// Tauri commands consumed by src-tauri/preload/shim.js
// ---------------------------------------------------------------------------

#[tauri::command]
fn resolve_window_configuration(state: State<'_, AppState>) -> Value {
	state.shell.window_configuration(1)
}

#[tauri::command]
fn ipc_send(state: State<'_, AppState>, channel: String, args: Vec<Value>) -> Result<(), ipc::IpcError> {
	state.shell.router.send(&channel, &args)
}

#[tauri::command]
fn ipc_invoke(state: State<'_, AppState>, channel: String, args: Vec<Value>) -> Result<Value, ipc::IpcError> {
	state.shell.router.invoke(&channel, &args)
}

#[tauri::command]
fn ipc_subscribe(channel: String) -> Result<(), ipc::IpcError> {
	ipc::validate(&channel)?;
	log::debug!("[ipc] renderer subscribed to {channel}");
	Ok(())
}

#[tauri::command]
fn fetch_shell_env() -> std::collections::BTreeMap<String, String> {
	shell_env::resolve()
}

#[tauri::command]
fn set_zoom_level(level: f64) {
	log::debug!("[window] zoom level {level}");
}

#[tauri::command]
fn get_process_memory_info() -> Value {
	serde_json::json!({ "residentSet": 0, "private": 0, "shared": 0 })
}

fn main() {
	env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();
	let cli = parse_cli();

	if cli.self_check {
		match self_check(cli.workbench_dir) {
			Ok(()) => std::process::exit(0),
			Err(err) => {
				eprintln!("self-check failed: {err:#}");
				std::process::exit(1);
			}
		}
	}

	let shell = match ShellState::new(cli.workbench_dir, cli.folder) {
		Ok(state) => Arc::new(state),
		Err(err) => {
			eprintln!("fatal: {err:#}");
			std::process::exit(1);
		}
	};

	log::info!("app root  : {}", shell.paths.app_root.display());
	log::info!("workbench : {}", shell.paths.out_dir.display());

	let asset_roots = shell.asset_roots();
	let shim = include_str!("../preload/shim.js");
	let entry = shell.paths.workbench_html();
	let entry_url = format!(
		"{}://{}{}",
		protocol::SCHEME,
		protocol::AUTHORITY,
		vscode_shell::to_uri_path(&entry)
	);
	log::info!("entry     : {entry_url}");

	let builder_state = AppState { shell: shell.clone() };

	tauri::Builder::default()
		.manage(builder_state)
		.plugin(tauri_plugin_dialog::init())
		.plugin(tauri_plugin_clipboard_manager::init())
		.register_uri_scheme_protocol(protocol::SCHEME, move |_app, request| {
			protocol::handle(&request, &asset_roots)
		})
		.invoke_handler(tauri::generate_handler![
			resolve_window_configuration,
			ipc_send,
			ipc_invoke,
			ipc_subscribe,
			fetch_shell_env,
			set_zoom_level,
			get_process_memory_info
		])
		.setup(move |app| {
			let url = entry_url
				.parse()
				.map_err(|e| format!("bad entry url {entry_url}: {e}"))?;

			let window = WebviewWindowBuilder::new(app, "main", WebviewUrl::External(url))
				.title(
					shell
						.product
						.get("nameLong")
						.and_then(Value::as_str)
						.unwrap_or("Code - OSS"),
				)
				.inner_size(1200.0, 800.0)
				.min_inner_size(400.0, 270.0)
				.decorations(false) // VS Code draws its own title bar
				.background_color(tauri::webview::Color(30, 30, 30, 255))
				.initialization_script(shim)
				.build()?;

			let _ = window.show();
			Ok(())
		})
		.run(tauri::generate_context!())
		.unwrap_or_else(|err| {
			eprintln!("fatal: could not start the Tauri shell: {err:#}");
			std::process::exit(1);
		});
}
