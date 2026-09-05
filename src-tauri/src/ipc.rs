/*---------------------------------------------------------------------------------------------
 *  IPC dispatch for the Phase 1 prototype.
 *
 *  The renderer shim (shim.js) forwards `vscode:*` channels here via Tauri IPC.
 *  Only the small set needed to reach first render is implemented; every unknown
 *  channel is logged once and catalogued for ROADMAP Phase 2 (IPC contract extraction).
 *
 *  Channel semantics mirror src/vs/base/parts/sandbox/electron-browser/preload.ts and
 *  src/vs/code/electron-main/app.ts in the original Electron implementation.
 *--------------------------------------------------------------------------------------------*/

use std::collections::HashMap;

use serde_json::{json, Value};

use crate::ShellState;

pub fn handle_invoke(
	state: &ShellState,
	channel: &str,
	_args: Option<&Value>,
) -> Result<Value, String> {
	// The window-config channel is dynamic in Electron (a UUID suffix); recognize by prefix.
	if channel.starts_with("vscode:") && channel.contains("window-config") {
		log::info!("[ipc] invoke: window configuration requested");
		return build_window_config(state);
	}

	let first_time = state
		.ipc_seen
		.lock()
		.map_err(|error| format!("ipc state poisoned: {error}"))?
		.insert(format!("invoke:{channel}"));

	match channel {
		"vscode:fetchShellEnv" => {
			log::info!("[ipc] invoke: fetchShellEnv");
			// Phase 1: the shell already inherits the environment the app was started from,
			// so the merged shell environment is simply the current environment.
			Ok(env_object())
		}
		_ => {
			if first_time {
				log::warn!("[ipc] UNIMPLEMENTED invoke channel `{channel}` (catalogue in Phase 2)");
			}
			// Mirrors Electron's invoke contract: JSON value back to the renderer.
			Ok(Value::Null)
		}
	}
}

pub fn handle_send(state: &ShellState, channel: &str, _args: Option<&Value>) -> Result<(), String> {
	let first_time = state
		.ipc_seen
		.lock()
		.map_err(|error| format!("ipc state poisoned: {error}"))?
		.insert(format!("send:{channel}"));

	if first_time {
		log::info!("[ipc] send (fire-and-forget, not yet dispatched): `{channel}`");
	}
	Ok(())
}

/// Build the `ISandboxConfiguration`/`INativeWindowConfiguration` payload that
/// Electron main normally resolves in `code/electron-main/app.ts`.
fn build_window_config(state: &ShellState) -> Result<Value, String> {
	let product = read_product(state);
	let nls_messages = read_nls_messages(state);

	Ok(json!({
		"windowId": 1,
		"appRoot": state.app_root.to_string_lossy(),
		"userEnv": env_object(),
		"product": product,
		"zoomLevel": 0,
		// NLS: English build default. `setupNLS` only assigns these globals.
		"nls": {
			"messages": nls_messages,
			"language": Value::Null
		},
		// Fields consumed by the splash screen before the workbench loads.
		"mainPid": std::process::id(),
		"machineId": "vscode-tauri-prototype",
		"colorScheme": { "dark": true, "highContrast": false },
		"autoDetectColorScheme": false,
		"autoDetectHighContrast": false,
		"perfEntries": [],
	}))
}

/// The product configuration Electron main reads from `product.json` at startup.
/// In dev builds the file sits next to `out/`; fall back to an embedded minimum.
fn read_product(state: &ShellState) -> Value {
	let candidates = [
		state.app_root.join("product.json"),
		state.app_root.join("out").join("product.json"),
	];
	for candidate in candidates {
		match std::fs::read_to_string(&candidate) {
			Ok(text) => match serde_json::from_str::<Value>(&text) {
				Ok(value) => return value,
				Err(error) => {
					log::warn!("[ipc] {} is not valid JSON ({error}); using embedded product fallback", candidate.display());
					return product_fallback();
				}
			},
			Err(error) if error.kind() != std::io::ErrorKind::NotFound => {
				log::warn!("[ipc] failed to read {}: {error}", candidate.display());
			}
			Err(_) => {}
		}
	}
	log::warn!("[ipc] product.json not found under {}; using embedded product fallback", state.app_root.display());
	product_fallback()
}

fn product_fallback() -> Value {
	json!({
		"nameShort": "Code",
		"nameLong": "Visual Studio Code",
		"applicationName": "code",
		"dataFolderName": ".vscode",
		"version": "0.0.0-tauri",
		"quality": Value::Null,
		"commit": Value::Null,
		// Keep telemetry & crash reporting inert in the prototype shell.
		"enableTelemetry": false,
		"enableCrashReporter": false,
		"crashReporterId": Value::Null
	})
}

/// English dev builds emit `out/nls.messages.json`; an empty array is acceptable
/// because `setupNLS` only assigns the global.
fn read_nls_messages(state: &ShellState) -> Value {
	let path = state.app_root.join("out").join("nls.messages.json");
	match std::fs::read_to_string(&path) {
		Ok(text) => serde_json::from_str(&text).unwrap_or_else(|error| {
			log::warn!("[ipc] nls.messages.json invalid ({error}); continuing with empty messages");
			Value::Array(Vec::new())
		}),
		Err(_) => Value::Array(Vec::new()),
	}
}

/// The environment forwarded to the renderer. Electron forwards the window's
/// environment; we do the same but drop obvious secret-looking keys so tokens
/// never reach the webview.
fn env_object() -> Value {
	let mut env: HashMap<String, String> = HashMap::new();
	for (key, value) in std::env::vars() {
		if looks_secretish(&key) {
			continue;
		}
		env.insert(key, value);
	}
	serde_json::to_value(env).unwrap_or_else(|error| {
		log::error!("[ipc] failed to serialize environment: {error}");
		Value::Object(Default::default())
	})
}

fn looks_secretish(key: &str) -> bool {
	const MARKERS: [&str; 8] = ["TOKEN", "SECRET", "PASSWORD", "PAT", "CREDENTIAL", "PRIVATE", "APIKEY", "API_KEY"];
	let upper = key.to_ascii_uppercase();
	MARKERS.iter().any(|marker| upper.contains(marker))
}
