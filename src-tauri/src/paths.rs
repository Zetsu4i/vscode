//! Resolution of the on-disk layout the shell serves to the renderer.
//!
//! The renderer is the *desktop* workbench bundle, exactly as produced by
//! `gulp minify-vscode` (`out-vscode-min/`) or by the dev compile (`out/`).
//! It is never served over HTTP (AGENTS.md hard constraint 11): the shell
//! resolves files from disk and hands the bytes to WebView2 through the
//! `vscode-file://vscode-app/` custom URI protocol, mirroring the scheme
//! Electron registers so `FileAccess.asBrowserUri` keeps working unchanged.

use std::env;
use std::path::{Path, PathBuf};

/// Layout of the application on disk.
#[derive(Debug, Clone)]
pub struct AppPaths {
	/// Root of the JavaScript sources: the directory that *contains* `out/`.
	/// This is what the workbench receives as `appRoot` and turns into
	/// `vscode-file://vscode-app/<appRoot>/out/`.
	pub app_root: PathBuf,
	/// `<app_root>/out`
	pub out_dir: PathBuf,
	/// Per-user data directory (settings, state, extensions, logs).
	pub user_data_dir: PathBuf,
	/// Built-in extensions shipped with the app.
	pub builtin_extensions_dir: PathBuf,
	/// User installed extensions.
	pub extensions_dir: PathBuf,
	pub home_dir: PathBuf,
	pub tmp_dir: PathBuf,
}

/// Directory names that may hold the compiled workbench, most specific first.
const OUT_CANDIDATES: &[&str] = &["out-vscode-min", "out-vscode", "out"];

fn find_out_dir(base: &Path) -> Option<PathBuf> {
	for candidate in OUT_CANDIDATES {
		let dir = base.join(candidate);
		if dir.join("vs/code/electron-browser/workbench/workbench.html").is_file()
			|| dir.join("vs/workbench/workbench.desktop.main.js").is_file()
		{
			return Some(dir);
		}
	}
	None
}

impl AppPaths {
	/// Resolve the layout, in priority order:
	///  1. `--workbench-dir <path>` / `VSCODE_TAURI_WORKBENCH` (points at the
	///     compiled bundle itself, i.e. the directory holding `vs/`)
	///  2. `<exe dir>/resources/workbench` (installed layout)
	///  3. walking up from the executable / cwd looking for a build output
	///     (repository layout during development)
	pub fn resolve(explicit_workbench: Option<PathBuf>) -> anyhow::Result<Self> {
		let out_dir = match explicit_workbench.or_else(|| env::var_os("VSCODE_TAURI_WORKBENCH").map(PathBuf::from)) {
			Some(dir) => {
				let dir = dir.canonicalize().unwrap_or(dir);
				// Accept either the bundle itself or its parent.
				if dir.join("vs/code/electron-browser/workbench/workbench.html").is_file() {
					dir
				} else {
					find_out_dir(&dir).ok_or_else(|| {
						anyhow::anyhow!("no compiled workbench found under {}", dir.display())
					})?
				}
			}
			None => Self::discover()?,
		};

		let app_root = out_dir
			.parent()
			.map(Path::to_path_buf)
			.ok_or_else(|| anyhow::anyhow!("workbench directory has no parent"))?;

		let home_dir = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
		let user_data_dir = env::var_os("VSCODE_PORTABLE")
			.map(|p| PathBuf::from(p).join("user-data"))
			.unwrap_or_else(|| {
				dirs::config_dir()
					.unwrap_or_else(|| home_dir.join(".config"))
					.join("Code-Tauri")
			});

		Ok(Self {
			builtin_extensions_dir: app_root.join("extensions"),
			extensions_dir: home_dir.join(".vscode-tauri").join("extensions"),
			out_dir,
			app_root,
			user_data_dir,
			home_dir,
			tmp_dir: env::temp_dir(),
		})
	}

	fn discover() -> anyhow::Result<PathBuf> {
		let mut bases: Vec<PathBuf> = Vec::new();
		if let Ok(exe) = env::current_exe() {
			if let Some(dir) = exe.parent() {
				bases.push(dir.join("resources/workbench"));
				bases.push(dir.to_path_buf());
				// target/{debug,release} -> repo root
				for up in dir.ancestors().take(6) {
					bases.push(up.to_path_buf());
				}
			}
		}
		if let Ok(cwd) = env::current_dir() {
			for up in cwd.ancestors().take(4) {
				bases.push(up.to_path_buf());
			}
		}

		for base in &bases {
			if let Some(found) = find_out_dir(base) {
				return Ok(found);
			}
			// installed layout: resources/workbench is itself the bundle
			if base.join("vs/code/electron-browser/workbench/workbench.html").is_file() {
				return Ok(base.clone());
			}
		}

		Err(anyhow::anyhow!(
			"could not locate a compiled VS Code workbench. Build it with `npm run gulp minify-vscode` \
			 or pass --workbench-dir <path>. Searched: {}",
			bases.iter().map(|p| p.display().to_string()).collect::<Vec<_>>().join(", ")
		))
	}

	/// Entry document the window navigates to.
	pub fn workbench_html(&self) -> PathBuf {
		self.out_dir.join("vs/code/electron-browser/workbench/workbench.html")
	}

	pub fn ensure_dirs(&self) {
		for dir in [&self.user_data_dir, &self.extensions_dir] {
			if let Err(err) = std::fs::create_dir_all(dir) {
				log::warn!("could not create {}: {err}", dir.display());
			}
		}
	}
}
