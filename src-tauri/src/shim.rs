/*---------------------------------------------------------------------------------------------
 *  Renders shim.js with shell-provided values.
 *
 *  The shim runs as a WebView initialization script, i.e. before any page script,
 *  playing the role of Electron's preload (which VS Code expects to have exposed
 *  a `window.vscode` namespace plus a handful of Node-ish globals).
 *--------------------------------------------------------------------------------------------*/

use serde_json::{Map, Value};

use crate::ShellState;

const SHIM_SOURCE: &str = include_str!("shim.js");

pub fn render(state: &ShellState) -> String {
	let platform = if cfg!(target_os = "windows") {
		"win32"
	} else if cfg!(target_os = "macos") {
		"darwin"
	} else {
		"linux"
	};

	let arch = match std::env::consts::ARCH {
		"x86_64" => "x64",
		"aarch64" => "arm64",
		other => other,
	};

	let exec_path = std::env::current_exe()
		.map(|path| path.to_string_lossy().into_owned())
		.unwrap_or_default();

	SHIM_SOURCE
		.replace("__VSCODE_TAURI_PLATFORM__", &json_string(platform))
		.replace("__VSCODE_TAURI_ARCH__", &json_string(arch))
		.replace("__VSCODE_TAURI_EXEC_PATH__", &json_string(&exec_path))
		.replace(
			"__VSCODE_TAURI_APP_ROOT__",
			&json_string(&state.app_root.to_string_lossy()),
		)
		.replace("__VSCODE_TAURI_WINDOW_ID__", "1")
		.replace("__VSCODE_TAURI_ENV__", &serde_json::to_string(&filtered_env()).unwrap_or_else(|_| "{}".to_string()))
}

/// Serialize as a JSON string literal (already quoted and escaped for JS).
fn json_string(value: &str) -> String {
	serde_json::to_string(value).unwrap_or_else(|_| "\"\"".to_string())
}

/// Initial `process.env` contents. Secret-looking keys are dropped so tokens
/// never reach the webview (they are also filtered on every fetchShellEnv).
fn filtered_env() -> Value {
	let mut map = Map::new();
	for (key, value) in std::env::vars() {
		let upper = key.to_ascii_uppercase();
		let secretish = ["TOKEN", "SECRET", "PASSWORD", "PAT", "CREDENTIAL", "PRIVATE", "APIKEY", "API_KEY"]
			.iter()
			.any(|marker| upper.contains(marker));
		if !secretish {
			map.insert(key, Value::String(value));
		}
	}
	Value::Object(map)
}
