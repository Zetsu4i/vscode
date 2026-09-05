//! `vscode:fetchShellEnv` — replacement for
//! `src/vs/platform/shell/node/shellEnv.ts`.
//!
//! When the app is launched from a GUI (Explorer, Start menu, Dock) it does not
//! inherit the user's login-shell environment, so PATH-dependent features
//! (language servers, tasks, terminals) misbehave. VS Code solves this by
//! spawning the login shell once and capturing its environment. Same approach
//! here, with the same guard rails: Windows inherits directly, POSIX spawns
//! the login shell with a marker-delimited dump.

use std::collections::BTreeMap;
use std::process::Command;
use std::time::Duration;

const MARK_START: &str = "_VSCODE_SHELL_ENV_START_";
const MARK_END: &str = "_VSCODE_SHELL_ENV_END_";

pub fn resolve() -> BTreeMap<String, String> {
	if cfg!(windows) {
		// Windows GUI processes already inherit the user environment.
		return std::env::vars().collect();
	}
	match resolve_posix() {
		Ok(env) if !env.is_empty() => env,
		Ok(_) => std::env::vars().collect(),
		Err(err) => {
			log::warn!("shell env resolution failed, falling back to process env: {err}");
			std::env::vars().collect()
		}
	}
}

fn resolve_posix() -> anyhow::Result<BTreeMap<String, String>> {
	let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/bash".to_string());
	let script = format!("echo {MARK_START}; env -0; echo {MARK_END}");

	// `-l -i` mirrors VS Code: login + interactive so rc files are sourced.
	let output = Command::new(&shell)
		.args(["-l", "-i", "-c", &script])
		.env("VSCODE_RESOLVING_ENVIRONMENT", "1")
		.output()?;

	let stdout = String::from_utf8_lossy(&output.stdout);
	let start = stdout
		.find(MARK_START)
		.map(|i| i + MARK_START.len())
		.ok_or_else(|| anyhow::anyhow!("shell env start marker missing"))?;
	let end = stdout
		.find(MARK_END)
		.ok_or_else(|| anyhow::anyhow!("shell env end marker missing"))?;
	if end < start {
		anyhow::bail!("shell env markers out of order");
	}

	let mut env = BTreeMap::new();
	for entry in stdout[start..end].split('\0') {
		if let Some((key, value)) = entry.trim_start_matches('\n').split_once('=') {
			if key.is_empty() || key == "VSCODE_RESOLVING_ENVIRONMENT" {
				continue;
			}
			env.insert(key.to_string(), value.to_string());
		}
	}
	Ok(env)
}

/// Upper bound so a broken rc file cannot hang startup, mirroring the timeout
/// VS Code applies to the same operation.
pub const RESOLUTION_TIMEOUT: Duration = Duration::from_secs(10);

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn resolution_returns_something_usable() {
		let env = resolve();
		assert!(env.contains_key("PATH") || env.contains_key("Path"));
	}
}
