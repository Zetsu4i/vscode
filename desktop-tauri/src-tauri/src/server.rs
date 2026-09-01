/*---------------------------------------------------------------------------------------------
 *  Copyright (c) Microsoft Corporation. All rights reserved.
 *  Licensed under the MIT License. See License.txt in the project root for license information.
 *--------------------------------------------------------------------------------------------*/

//! Management of the VS Code server sidecar process.
//!
//! The Tauri shell does not reimplement the workbench: it launches the very same
//! `vscode-reh-web` server build that powers vscode.dev remotes / openvscode-server
//! (Node.js extension host, node-pty terminals, real filesystem access) and points
//! the OS webview at it. This module owns the full lifecycle of that process:
//!
//!   resolve launcher -> spawn (loopback only, random port, secret token)
//!     -> parse the "Web UI available at ..." readiness line -> hand the URL
//!        to the window -> terminate the whole process group on exit.

use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc;
use std::time::Duration;
#[cfg(unix)]
use std::time::Instant;

use tauri::{AppHandle, Manager};

/// How long we give the server to print its readiness line before giving up.
const READY_TIMEOUT: Duration = Duration::from_secs(180);

/// Marker printed by `remoteExtensionHostAgentServer.ts` once the HTTP server listens.
const READY_MARKER: &str = "Web UI available at ";

/// A running VS Code server process.
pub struct ServerHandle {
	child: Child,
}

/// How the server is started, depending on what kind of build we found.
enum Launcher {
	/// A packaged `vscode-reh-web-*` build: `bin/code-server-oss` (`.cmd` on Windows).
	Script(PathBuf),
	/// A raw entry point run with a Node.js binary: `node <...>/server-main.js`.
	NodeEntry { node: PathBuf, entry: PathBuf },
}

impl ServerHandle {
	/// Terminate the server and everything it spawned (extension hosts, ptys, watchers).
	pub fn shutdown(&mut self) {
		let pid = self.child.id();
		log::info!("shutting down VS Code server (pid {pid})");

		#[cfg(unix)]
		{
			// The child was started in its own session (setsid), so signalling the
			// process group reaches extension hosts and pty children as well.
			// SIGTERM first so the server can flush state, SIGKILL as a last resort.
			unsafe {
				libc::killpg(pid as libc::pid_t, libc::SIGTERM);
			}
			let deadline = Instant::now() + Duration::from_secs(5);
			loop {
				match self.child.try_wait() {
					Ok(Some(_)) => return,
					Ok(None) if Instant::now() < deadline => {
						std::thread::sleep(Duration::from_millis(100));
					}
					_ => break,
				}
			}
			unsafe {
				libc::killpg(pid as libc::pid_t, libc::SIGKILL);
			}
			let _ = self.child.wait();
		}

		#[cfg(windows)]
		{
			// `taskkill /T` tears down the whole tree (conhost, extension hosts, ...).
			let _ = Command::new("taskkill")
				.args(["/PID", &pid.to_string(), "/T", "/F"])
				.creation_flags(CREATE_NO_WINDOW)
				.status();
			let _ = self.child.kill();
			let _ = self.child.wait();
		}
	}
}

#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;
#[cfg(windows)]
use std::os::windows::process::CommandExt;

/// Start the server and block until it is reachable.
///
/// Returns the process handle and the ready-to-load URL
/// (loopback host + connection token already included).
pub fn start(app: &AppHandle) -> Result<(ServerHandle, tauri::Url), String> {
	let launcher = resolve_launcher(app)?;
	let token = uuid::Uuid::new_v4().simple().to_string();

	let data_dir = app
		.path()
		.app_data_dir()
		.map_err(|e| format!("could not resolve app data dir: {e}"))?
		.join("server-data");
	std::fs::create_dir_all(&data_dir)
		.map_err(|e| format!("could not create {}: {e}", data_dir.display()))?;

	let mut cmd = match &launcher {
		Launcher::Script(script) => {
			log::info!("launching packaged server: {}", script.display());
			Command::new(script)
		}
		Launcher::NodeEntry { node, entry } => {
			log::info!(
				"launching server entry point: {} {}",
				node.display(),
				entry.display()
			);
			let mut c = Command::new(node);
			c.arg(entry);
			c
		}
	};

	cmd.args([
		"--host",
		"127.0.0.1",
		"--port",
		"0", // let the server pick a free port; we parse it from stdout
		"--connection-token",
		&token,
		"--accept-server-license-terms",
		"--telemetry-level",
		"off",
		"--server-data-dir",
	])
	.arg(&data_dir)
	.stdin(Stdio::null())
	.stdout(Stdio::piped())
	.stderr(Stdio::piped());

	#[cfg(unix)]
	{
		use std::os::unix::process::CommandExt;
		// New session => new process group, so shutdown() can killpg the whole tree.
		unsafe {
			cmd.pre_exec(|| {
				libc::setsid();
				Ok(())
			});
		}
	}

	#[cfg(windows)]
	cmd.creation_flags(CREATE_NO_WINDOW);

	let mut child = cmd
		.spawn()
		.map_err(|e| format!("failed to spawn VS Code server: {e}"))?;

	// Forward stderr to our log so problems are visible.
	if let Some(stderr) = child.stderr.take() {
		std::thread::spawn(move || {
			for line in BufReader::new(stderr).lines().map_while(Result::ok) {
				log::warn!("[server] {line}");
			}
		});
	}

	// Scan stdout for the readiness line; keep draining it afterwards so the
	// server never blocks on a full pipe.
	let stdout = child
		.stdout
		.take()
		.ok_or_else(|| "server stdout was not captured".to_string())?;
	let (tx, rx) = mpsc::channel::<String>();
	std::thread::spawn(move || {
		let mut announced = false;
		for line in BufReader::new(stdout).lines().map_while(Result::ok) {
			log::info!("[server] {line}");
			if !announced {
				if let Some(idx) = line.find(READY_MARKER) {
					let url = line[idx + READY_MARKER.len()..].trim().to_string();
					let _ = tx.send(url);
					announced = true;
				}
			}
		}
	});

	let raw_url = match rx.recv_timeout(READY_TIMEOUT) {
		Ok(url) => url,
		Err(_) => {
			let mut handle = ServerHandle { child };
			handle.shutdown();
			return Err(format!(
				"VS Code server did not become ready within {} seconds",
				READY_TIMEOUT.as_secs()
			));
		}
	};

	let mut url = tauri::Url::parse(&raw_url)
		.map_err(|e| format!("server announced an invalid URL ({raw_url}): {e}"))?;
	// The server prints "localhost"; use the loopback IP to avoid any resolver quirks.
	url.set_host(Some("127.0.0.1"))
		.map_err(|e| format!("could not rewrite server host: {e}"))?;
	// Belt and braces: make sure the connection token is on the URL.
	if !url.query_pairs().any(|(k, _)| k == "tkn") {
		url.query_pairs_mut().append_pair("tkn", &token);
	}

	Ok((ServerHandle { child }, url))
}

/// Locate a server build, in priority order:
///
/// 1. `VSCODE_TAURI_SERVER_DIR` env var - explicit path to a `vscode-reh-web-*` build
///    or to a repository checkout (for development).
/// 2. The `server` directory bundled as a Tauri resource (production installs).
/// 3. A repository checkout above the executable (running `tauri dev` inside the repo).
fn resolve_launcher(app: &AppHandle) -> Result<Launcher, String> {
	let mut tried: Vec<String> = Vec::new();

	if let Ok(dir) = std::env::var("VSCODE_TAURI_SERVER_DIR") {
		let dir = PathBuf::from(dir);
		if let Some(launcher) = launcher_in(&dir) {
			return Ok(launcher);
		}
		tried.push(format!("$VSCODE_TAURI_SERVER_DIR ({})", dir.display()));
	}

	if let Ok(resources) = app.path().resource_dir() {
		let dir = resources.join("server");
		if let Some(launcher) = launcher_in(&dir) {
			return Ok(launcher);
		}
		tried.push(format!("bundled resources ({})", dir.display()));
	}

	// Development convenience: walk up from the executable looking for the repo root
	// (identified by out/server-main.js produced by `npm run compile`).
	if let Ok(exe) = std::env::current_exe() {
		let mut cursor: Option<&Path> = exe.parent();
		while let Some(dir) = cursor {
			let entry = dir.join("out").join("server-main.js");
			if entry.is_file() {
				return Ok(Launcher::NodeEntry {
					node: PathBuf::from(node_binary_name()),
					entry,
				});
			}
			cursor = dir.parent();
		}
		tried.push("repository checkout above the executable".to_string());
	}

	Err(format!(
		"No VS Code server build found. Tried: {}. Run `npm run prepare-server` in desktop-tauri/ \
		 or set VSCODE_TAURI_SERVER_DIR to a vscode-reh-web build.",
		tried.join(", ")
	))
}

/// Identify a usable launcher inside `dir`.
fn launcher_in(dir: &Path) -> Option<Launcher> {
	if !dir.is_dir() {
		return None;
	}

	// Packaged vscode-reh-web build: bin/<serverApplicationName>[.cmd]
	let script = dir.join("bin").join(if cfg!(windows) {
		"code-server-oss.cmd"
	} else {
		"code-server-oss"
	});
	if script.is_file() {
		return Some(Launcher::Script(script));
	}

	// Packaged build addressed directly: bundled node + server-main.js at the root.
	let entry = dir.join("server-main.js");
	let node = dir.join(node_binary_name());
	if entry.is_file() && node.is_file() {
		return Some(Launcher::NodeEntry { node, entry });
	}

	// Repository checkout: out/server-main.js with the system Node.js.
	let entry = dir.join("out").join("server-main.js");
	if entry.is_file() {
		return Some(Launcher::NodeEntry {
			node: PathBuf::from(node_binary_name()),
			entry,
		});
	}

	None
}

fn node_binary_name() -> &'static str {
	if cfg!(windows) {
		"node.exe"
	} else {
		"node"
	}
}
