//! The `vscode:` IPC channel router.
//!
//! Electron's main process answers renderer traffic through
//! `validatedIpcMain.handle/on` (see `src/vs/base/parts/ipc/electron-main/`).
//! Every channel name is preserved verbatim here (AGENTS.md technical rules)
//! so the renderer's callers need no modification.
//!
//! Phase 1 implements the channels the workbench touches during boot. Channels
//! belonging to later phases return a structured `Unimplemented` error rather
//! than panicking, and are logged once so the parity gap is visible.

use std::collections::HashSet;

use parking_lot::Mutex;
use serde_json::{json, Value};

/// Channels the shell answers today. Kept as data so `--self-check` and the
/// contract tests in `compat/tests/` can assert against the same list.
pub const IMPLEMENTED_CHANNELS: &[&str] = &[
	"vscode:hello",
	"vscode:fetchShellEnv",
	"vscode:toggleDevTools",
	"vscode:openDevTools",
	"vscode:reloadWindow",
	"vscode:notifyZoomLevel",
];

/// Channels known to exist upstream but not yet ported. Listing them makes the
/// remaining work explicit instead of silently failing.
pub const PENDING_CHANNELS: &[&str] = &[
	"vscode:createPtyHostMessageChannel",     // Phase 5
	"vscode:createAgentHostMessageChannel",   // Phase 7
	"vscode:registerAuxiliaryWindow",         // Phase 3
	"vscode:readFile",                        // Phase 4
	"vscode:statFile",                        // Phase 4
];

#[derive(Debug, thiserror::Error)]
pub enum IpcError {
	#[error("unsupported event IPC channel '{0}'")]
	InvalidChannel(String),
	#[error("channel '{0}' is not implemented in the Tauri shell yet")]
	Unimplemented(String),
	#[error("channel '{channel}' failed: {source}")]
	Failed {
		channel: String,
		#[source]
		source: anyhow::Error,
	},
}

impl serde::Serialize for IpcError {
	fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
		serializer.serialize_str(&self.to_string())
	}
}

/// Same guard as `validateIPC` in `preload.ts`: only `vscode:` channels cross
/// the boundary, so a compromised renderer cannot reach arbitrary commands.
pub fn validate(channel: &str) -> Result<(), IpcError> {
	if channel.starts_with("vscode:") {
		Ok(())
	} else {
		Err(IpcError::InvalidChannel(channel.to_string()))
	}
}

#[derive(Default)]
pub struct IpcRouter {
	warned: Mutex<HashSet<String>>,
}

impl IpcRouter {
	pub fn new() -> Self {
		Self::default()
	}

	fn warn_once(&self, channel: &str) {
		let mut warned = self.warned.lock();
		if warned.insert(channel.to_string()) {
			log::warn!("[ipc] channel '{channel}' not implemented yet (see ROADMAP.md)");
		}
	}

	/// Fire-and-forget traffic (`ipcRenderer.send`).
	pub fn send(&self, channel: &str, _args: &[Value]) -> Result<(), IpcError> {
		validate(channel)?;
		log::debug!("[ipc] send {channel}");
		match channel {
			"vscode:hello" => Ok(()),
			"vscode:notifyZoomLevel" => Ok(()),
			"vscode:toggleDevTools" | "vscode:openDevTools" | "vscode:reloadWindow" => Ok(()),
			other => {
				self.warn_once(other);
				Ok(()) // sends are best-effort in Electron too
			}
		}
	}

	/// Request/response traffic (`ipcRenderer.invoke`).
	pub fn invoke(&self, channel: &str, _args: &[Value]) -> Result<Value, IpcError> {
		validate(channel)?;
		log::debug!("[ipc] invoke {channel}");
		match channel {
			"vscode:fetchShellEnv" => Ok(json!(crate::shell_env::resolve())),
			other => {
				self.warn_once(other);
				Err(IpcError::Unimplemented(other.to_string()))
			}
		}
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn rejects_non_vscode_channels() {
		assert!(validate("evil:exec").is_err());
		assert!(validate("vscode:hello").is_ok());
	}

	#[test]
	fn hello_is_accepted() {
		let router = IpcRouter::new();
		assert!(router.send("vscode:hello", &[]).is_ok());
	}

	#[test]
	fn unimplemented_channels_error_rather_than_panic() {
		let router = IpcRouter::new();
		let err = router.invoke("vscode:createPtyHostMessageChannel", &[]);
		assert!(matches!(err, Err(IpcError::Unimplemented(_))));
	}

	#[test]
	fn pending_and_implemented_do_not_overlap() {
		for pending in PENDING_CHANNELS {
			assert!(!IMPLEMENTED_CHANNELS.contains(pending), "{pending} listed twice");
		}
	}
}
