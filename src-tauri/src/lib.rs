//! VS Code Tauri shell — library surface.
//!
//! Split out of `main.rs` so the logic is unit-testable without a window and
//! so `--self-check` can exercise the same code paths CI does.

pub mod ipc;
pub mod paths;
pub mod protocol;
pub mod shell_env;
pub mod window_config;

use std::path::PathBuf;

use serde_json::Value;

/// Everything the shell needs at runtime, resolved once at startup.
pub struct ShellState {
	pub paths: paths::AppPaths,
	pub product: Value,
	pub nls_messages: Vec<String>,
	pub nls_language: Option<String>,
	pub router: ipc::IpcRouter,
	pub folder_uri: Option<String>,
}

impl ShellState {
	pub fn new(workbench_dir: Option<PathBuf>, folder_uri: Option<String>) -> anyhow::Result<Self> {
		let paths = paths::AppPaths::resolve(workbench_dir)?;
		paths.ensure_dirs();
		let product = window_config::load_product(&paths);
		let (nls_messages, nls_language) = window_config::load_nls(&paths);
		Ok(Self {
			paths,
			product,
			nls_messages,
			nls_language,
			router: ipc::IpcRouter::new(),
			folder_uri,
		})
	}

	pub fn window_configuration(&self, window_id: i64) -> Value {
		window_config::WindowConfigBuilder {
			paths: &self.paths,
			window_id,
			product: self.product.clone(),
			nls_messages: self.nls_messages.clone(),
			nls_language: self.nls_language.clone(),
			folder_uri: self.folder_uri.clone(),
			dark: true,
		}
		.build()
	}

	/// Filesystem roots the custom protocol may read from.
	pub fn asset_roots(&self) -> Vec<PathBuf> {
		vec![
			self.paths.app_root.clone(),
			self.paths.out_dir.clone(),
			self.paths.builtin_extensions_dir.clone(),
			self.paths.extensions_dir.clone(),
			self.paths.user_data_dir.clone(),
		]
	}
}

/// Headless verification used by CI (`--self-check`) and by developers.
///
/// This is the regression guard for the "blank window" class of bug: it proves
/// the boot files exist, that the protocol resolver finds them, that they are
/// served with the MIME types a webview requires, and that the window
/// configuration payload contains every field the renderer dereferences before
/// it can paint.
pub fn self_check(workbench_dir: Option<PathBuf>) -> anyhow::Result<()> {
	let state = ShellState::new(workbench_dir, None)?;
	println!("app_root   = {}", state.paths.app_root.display());
	println!("out_dir    = {}", state.paths.out_dir.display());

	let roots = state.asset_roots();
	let boot_files = [
		("vs/code/electron-browser/workbench/workbench.html", "text/html"),
		("vs/code/electron-browser/workbench/workbench.js", "text/javascript"),
		("vs/workbench/workbench.desktop.main.js", "text/javascript"),
		("vs/workbench/workbench.desktop.main.css", "text/css"),
	];

	let mut failures = Vec::new();
	for (relative, expected_mime) in boot_files {
		let absolute = state.paths.out_dir.join(relative);
		let uri = format!(
			"{}://{}{}",
			protocol::SCHEME,
			protocol::AUTHORITY,
			to_uri_path(&absolute)
		);
		match protocol::resolve_uri(&uri, &roots) {
			Ok(resolved) if resolved.is_file() => {
				let mime = protocol::mime_for(&resolved);
				if mime.starts_with(expected_mime) {
					println!("ok   {relative} -> {mime}");
				} else {
					failures.push(format!("{relative}: mime {mime}, want {expected_mime}"));
				}
			}
			Ok(resolved) => failures.push(format!("{relative}: missing at {}", resolved.display())),
			Err(status) => failures.push(format!("{relative}: resolver rejected ({status})")),
		}
	}

	// Traversal must be refused even for a path that exists.
	let escape = format!(
		"{}://{}/{}",
		protocol::SCHEME,
		protocol::AUTHORITY,
		"../../../../etc/passwd"
	);
	if protocol::resolve_uri(&escape, &roots).is_ok() {
		failures.push("path traversal was NOT rejected".to_string());
	} else {
		println!("ok   path traversal rejected");
	}

	let config = state.window_configuration(1);
	for key in [
		"windowId", "appRoot", "userEnv", "product", "nls", "colorScheme",
		"profiles", "homeDir", "tmpDir", "userDataDir", "execPath", "mainPid",
		"logLevel", "loggers", "perfMarks", "os", "machineId",
	] {
		if config.get(key).is_none() {
			failures.push(format!("window configuration missing '{key}'"));
		}
	}
	println!("ok   window configuration fields present");
	for (key, value) in window_config::describe(&config) {
		println!("     {key} = {value}");
	}

	if failures.is_empty() {
		println!("SELFCHECK_OK");
		Ok(())
	} else {
		for failure in &failures {
			eprintln!("FAIL {failure}");
		}
		Err(anyhow::anyhow!("{} self-check failure(s)", failures.len()))
	}
}

/// Absolute path -> the path portion of a `vscode-file://` URI, matching
/// `fileUriFromPath` in `workbench.ts`.
pub fn to_uri_path(path: &std::path::Path) -> String {
	let mut s = path.to_string_lossy().replace('\\', "/");
	if !s.starts_with('/') {
		s.insert(0, '/');
	}
	s
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn uri_path_always_absolute() {
		assert_eq!(to_uri_path(std::path::Path::new("C:\\a\\b")), "/C:/a/b");
		assert_eq!(to_uri_path(std::path::Path::new("/a/b")), "/a/b");
	}
}
