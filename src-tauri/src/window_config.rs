//! Builds the `INativeWindowConfiguration` payload the renderer awaits.
//!
//! `src/vs/code/electron-browser/workbench/workbench.ts` blocks on
//! `preloadGlobals.context.resolveConfiguration()` before it imports
//! `workbench.desktop.main.js`. In Electron that promise is fulfilled by
//! `ipcRenderer.invoke('vscode:window-config:<id>')`. Here the same shape is
//! produced by Rust and handed to the shim, so the renderer cannot tell the
//! difference.
//!
//! Field names mirror `src/vs/platform/window/common/window.ts`
//! (`INativeWindowConfiguration extends IWindowConfiguration, NativeParsedArgs,
//! ISandboxConfiguration`) and must not be renamed.

use std::collections::BTreeMap;
use std::path::Path;

use serde_json::{json, Map, Value};

use crate::paths::AppPaths;

fn path_to_uri_components(path: &Path) -> Value {
	let mut s = path.to_string_lossy().replace('\\', "/");
	if !s.starts_with('/') {
		s.insert(0, '/');
	}
	json!({
		"scheme": "file",
		"authority": "",
		"path": s,
		"query": "",
		"fragment": ""
	})
}

fn env_map() -> Map<String, Value> {
	// Only forward what the renderer legitimately needs; never forward secrets
	// wholesale into the webview (AGENTS.md security rules).
	const ALLOW: &[&str] = &[
		"PATH", "HOME", "USERPROFILE", "TEMP", "TMP", "TMPDIR", "LANG", "LC_ALL",
		"SHELL", "COMSPEC", "USERNAME", "USER", "OS", "PROCESSOR_ARCHITECTURE",
		"APPDATA", "LOCALAPPDATA", "SystemRoot", "windir", "NUMBER_OF_PROCESSORS",
		"VSCODE_DEV", "VSCODE_CWD", "VSCODE_PORTABLE", "VSCODE_NLS_CONFIG",
	];
	let mut map = Map::new();
	for key in ALLOW {
		if let Ok(value) = std::env::var(key) {
			map.insert((*key).to_string(), Value::String(value));
		}
	}
	map
}

fn platform() -> &'static str {
	if cfg!(target_os = "windows") {
		"win32"
	} else if cfg!(target_os = "macos") {
		"darwin"
	} else {
		"linux"
	}
}

pub struct WindowConfigBuilder<'a> {
	pub paths: &'a AppPaths,
	pub window_id: i64,
	pub product: Value,
	pub nls_messages: Vec<String>,
	pub nls_language: Option<String>,
	pub folder_uri: Option<String>,
	pub dark: bool,
}

impl<'a> WindowConfigBuilder<'a> {
	pub fn build(&self) -> Value {
		let paths = self.paths;
		let user_data = paths.user_data_dir.to_string_lossy().to_string();

		let profile = json!({
			"id": "__default__profile__",
			"name": "Default",
			"isDefault": true,
			"location": path_to_uri_components(&paths.user_data_dir.join("User")),
			"globalStorageHome": path_to_uri_components(&paths.user_data_dir.join("User/globalStorage")),
			"settingsResource": path_to_uri_components(&paths.user_data_dir.join("User/settings.json")),
			"keybindingsResource": path_to_uri_components(&paths.user_data_dir.join("User/keybindings.json")),
			"tasksResource": path_to_uri_components(&paths.user_data_dir.join("User/tasks.json")),
			"snippetsHome": path_to_uri_components(&paths.user_data_dir.join("User/snippets")),
			"promptsHome": path_to_uri_components(&paths.user_data_dir.join("User/prompts")),
			"extensionsResource": path_to_uri_components(&paths.extensions_dir.join("extensions.json")),
			"cacheHome": path_to_uri_components(&paths.user_data_dir.join("CachedProfileData")),
			"mcpResource": path_to_uri_components(&paths.user_data_dir.join("User/mcp.json"))
		});

		let workspace = self.folder_uri.as_ref().map(|uri| {
			json!({
				"id": format!("{:x}", md5_like(uri)),
				"uri": uri_string_to_components(uri)
			})
		});

		let mut config = Map::new();

		// --- ISandboxConfiguration -----------------------------------------
		config.insert("windowId".into(), json!(self.window_id));
		config.insert("appRoot".into(), json!(paths.app_root.to_string_lossy()));
		config.insert("userEnv".into(), Value::Object(env_map()));
		config.insert("product".into(), self.product.clone());
		config.insert("zoomLevel".into(), json!(0));
		config.insert(
			"nls".into(),
			json!({ "messages": self.nls_messages, "language": self.nls_language }),
		);

		// --- IWindowConfiguration ------------------------------------------
		config.insert("filesToOpenOrCreate".into(), Value::Null);
		config.insert("filesToDiff".into(), Value::Null);
		config.insert("filesToMerge".into(), Value::Null);
		config.insert("remoteAuthority".into(), Value::Null);

		// --- INativeWindowConfiguration ------------------------------------
		config.insert("mainPid".into(), json!(std::process::id()));
		config.insert("machineId".into(), json!(stable_machine_id()));
		config.insert("sqmId".into(), json!(""));
		config.insert("devDeviceId".into(), json!(stable_machine_id()));
		config.insert("isPortable".into(), json!(std::env::var_os("VSCODE_PORTABLE").is_some()));
		config.insert(
			"execPath".into(),
			json!(std::env::current_exe().unwrap_or_default().to_string_lossy()),
		);
		config.insert(
			"profiles".into(),
			json!({
				"home": path_to_uri_components(&paths.user_data_dir.join("User/profiles")),
				"all": [profile.clone()],
				"profile": profile
			}),
		);
		config.insert("homeDir".into(), json!(paths.home_dir.to_string_lossy()));
		config.insert("tmpDir".into(), json!(paths.tmp_dir.to_string_lossy()));
		config.insert("userDataDir".into(), json!(user_data));
		if let Some(ws) = workspace {
			config.insert("workspace".into(), ws);
		}
		config.insert("isInitialStartup".into(), json!(true));
		config.insert("logLevel".into(), json!(2)); // LogLevel.Info
		config.insert("loggers".into(), json!([]));
		config.insert("fullscreen".into(), json!(false));
		config.insert("maximized".into(), json!(false));
		config.insert("accessibilitySupport".into(), json!(false));
		config.insert(
			"colorScheme".into(),
			json!({ "dark": self.dark, "highContrast": false }),
		);
		config.insert("autoDetectHighContrast".into(), json!(true));
		config.insert("autoDetectColorScheme".into(), json!(false));
		config.insert("isCustomZoomLevel".into(), json!(false));
		config.insert("perfMarks".into(), json!([]));
		config.insert(
			"os".into(),
			json!({
				"release": os_release(),
				"hostname": hostname(),
				"arch": std::env::consts::ARCH
			}),
		);

		// --- NativeParsedArgs (only what the renderer reads) ----------------
		config.insert("_".into(), json!([]));
		config.insert("user-data-dir".into(), json!(user_data));
		config.insert(
			"extensions-dir".into(),
			json!(paths.extensions_dir.to_string_lossy()),
		);
		config.insert(
			"builtin-extensions-dir".into(),
			json!(paths.builtin_extensions_dir.to_string_lossy()),
		);

		// Not part of the Electron payload: the shim uses it to decide how to
		// rewrite `vscode-file://` asset URLs on platforms whose webview does
		// not allow a bespoke scheme (see preload/shim.js).
		config.insert("__shellPlatform".into(), json!(platform()));

		Value::Object(config)
	}
}

fn uri_string_to_components(uri: &str) -> Value {
	// Accepts `file:///C:/x` or a plain path.
	let path = uri.strip_prefix("file://").unwrap_or(uri);
	let mut path = path.to_string();
	if !path.starts_with('/') {
		path.insert(0, '/');
	}
	json!({ "scheme": "file", "authority": "", "path": path, "query": "", "fragment": "" })
}

/// Deterministic non-cryptographic id; only used where VS Code wants a stable
/// opaque string, never for security.
fn md5_like(input: &str) -> u64 {
	let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
	for byte in input.as_bytes() {
		hash ^= *byte as u64;
		hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
	}
	hash
}

fn stable_machine_id() -> String {
	let seed = format!("{}|{}", hostname(), std::env::consts::OS);
	format!("{:016x}{:016x}", md5_like(&seed), md5_like(&seed[..seed.len().min(8)]))
}

fn hostname() -> String {
	sysinfo::System::host_name().unwrap_or_else(|| "localhost".to_string())
}

fn os_release() -> String {
	sysinfo::System::os_version().unwrap_or_else(|| "0.0.0".to_string())
}

/// Load `product.json` from the app root, falling back to a minimal stub so the
/// shell still boots on an incomplete tree.
pub fn load_product(paths: &AppPaths) -> Value {
	for candidate in [
		paths.app_root.join("product.json"),
		paths.out_dir.join("product.json"),
	] {
		if let Ok(text) = std::fs::read_to_string(&candidate) {
			if let Ok(value) = serde_json::from_str::<Value>(&text) {
				log::info!("product.json loaded from {}", candidate.display());
				return value;
			}
		}
	}
	log::warn!("product.json not found; using stub");
	json!({
		"nameShort": "Code - OSS",
		"nameLong": "Code - OSS",
		"applicationName": "code-oss",
		"dataFolderName": ".vscode-oss",
		"version": env!("CARGO_PKG_VERSION")
	})
}

/// Load `nls.messages.json` produced by the build, if present.
pub fn load_nls(paths: &AppPaths) -> (Vec<String>, Option<String>) {
	let candidate = paths.out_dir.join("nls.messages.json");
	if let Ok(text) = std::fs::read_to_string(&candidate) {
		if let Ok(Value::Array(items)) = serde_json::from_str::<Value>(&text) {
			let messages = items
				.into_iter()
				.map(|v| v.as_str().unwrap_or_default().to_string())
				.collect::<Vec<_>>();
			log::info!("loaded {} NLS messages", messages.len());
			return (messages, Some("en".to_string()));
		}
	}
	(Vec::new(), Some("en".to_string()))
}

/// Diagnostics used by `--self-check` and by the unit tests.
pub fn describe(config: &Value) -> BTreeMap<String, String> {
	let mut out = BTreeMap::new();
	if let Value::Object(map) = config {
		for key in ["windowId", "appRoot", "userDataDir", "homeDir", "mainPid"] {
			if let Some(value) = map.get(key) {
				out.insert(key.to_string(), value.to_string());
			}
		}
	}
	out
}

#[cfg(test)]
mod tests {
	use super::*;

	fn paths() -> AppPaths {
		AppPaths {
			app_root: "/app".into(),
			out_dir: "/app/out".into(),
			user_data_dir: "/data".into(),
			builtin_extensions_dir: "/app/extensions".into(),
			extensions_dir: "/data/ext".into(),
			home_dir: "/home/u".into(),
			tmp_dir: "/tmp".into(),
		}
	}

	#[test]
	fn payload_has_every_field_the_renderer_dereferences() {
		let paths = paths();
		let config = WindowConfigBuilder {
			paths: &paths,
			window_id: 1,
			product: json!({ "nameShort": "Code" }),
			nls_messages: vec![],
			nls_language: Some("en".into()),
			folder_uri: None,
			dark: true,
		}
		.build();

		// workbench.ts / desktop.main.ts read these unconditionally.
		for key in [
			"windowId", "appRoot", "userEnv", "product", "nls", "colorScheme",
			"profiles", "homeDir", "tmpDir", "userDataDir", "execPath", "mainPid",
			"logLevel", "loggers", "perfMarks", "os", "machineId",
		] {
			assert!(config.get(key).is_some(), "missing config field: {key}");
		}
		assert!(config["nls"]["messages"].is_array());
		assert!(config["colorScheme"]["dark"].as_bool().unwrap());
	}

	#[test]
	fn env_is_filtered_not_wholesale() {
		std::env::set_var("VSTAURI_SECRET_TOKEN", "should-not-leak");
		let map = env_map();
		assert!(!map.contains_key("VSTAURI_SECRET_TOKEN"));
	}
}
