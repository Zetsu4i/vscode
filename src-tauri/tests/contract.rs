//! Contract tests: the Tauri shell must honour the same IPC surface Electron
//! exposed. Driven by `compat/ipc-contract.json` so the catalogue and the
//! implementation cannot drift apart.

use serde_json::Value;
use vscode_shell::ipc::{self, IpcError, IpcRouter, IMPLEMENTED_CHANNELS, PENDING_CHANNELS};

fn contract() -> Value {
	let text = include_str!("../../compat/ipc-contract.json");
	serde_json::from_str(text).expect("ipc-contract.json must be valid JSON")
}

fn channels() -> Vec<(String, String)> {
	let contract = contract();
	let mut out = Vec::new();
	let groups = contract["groups"].as_object().expect("groups object");
	for (_group, body) in groups {
		for channel in body["channels"].as_array().unwrap_or(&Vec::new()) {
			let name = channel["name"].as_str().unwrap_or_default().to_string();
			let status = channel["status"].as_str().unwrap_or("pending").to_string();
			out.push((name, status));
		}
	}
	out
}

#[test]
fn every_channel_name_is_namespaced() {
	for (name, _) in channels() {
		assert!(
			ipc::validate(&name).is_ok(),
			"channel '{name}' would be rejected by validateIPC"
		);
	}
}

#[test]
fn channels_marked_implemented_actually_answer() {
	let router = IpcRouter::new();
	for (name, status) in channels() {
		if status != "implemented" {
			continue;
		}
		// Either send or invoke must succeed; a channel marked implemented may
		// not fall through to the Unimplemented arm.
		let sent = router.send(&name, &[]);
		assert!(sent.is_ok(), "send({name}) failed: {sent:?}");

		if let Err(IpcError::Unimplemented(_)) = router.invoke(&name, &[]) {
			// invoke-only channels are allowed to be send-shaped; only fail if
			// the contract says kind == invoke.
			let contract = contract();
			let is_invoke = contract["groups"]
				.as_object()
				.into_iter()
				.flatten()
				.flat_map(|(_, body)| body["channels"].as_array().cloned().unwrap_or_default())
				.any(|c| c["name"] == name.as_str() && c["kind"] == "invoke");
			assert!(
				!is_invoke,
				"channel '{name}' is contracted as invoke+implemented but returns Unimplemented"
			);
		}
	}
}

#[test]
fn pending_channels_fail_loudly_not_silently() {
	let router = IpcRouter::new();
	for name in PENDING_CHANNELS {
		match router.invoke(name, &[]) {
			Err(IpcError::Unimplemented(_)) => {}
			other => panic!("pending channel '{name}' returned {other:?}, want Unimplemented"),
		}
	}
}

#[test]
fn implemented_list_matches_the_contract() {
	let contracted: Vec<String> = channels()
		.into_iter()
		.filter(|(_, status)| status == "implemented")
		.map(|(name, _)| name)
		.collect();

	for name in &contracted {
		assert!(
			IMPLEMENTED_CHANNELS.contains(&name.as_str()),
			"contract says '{name}' is implemented but ipc.rs does not list it"
		);
	}
}

#[test]
fn foreign_channels_are_refused() {
	let router = IpcRouter::new();
	for evil in ["exec", "file:read", "tauri:invoke", ""] {
		assert!(
			matches!(router.send(evil, &[]), Err(IpcError::InvalidChannel(_))),
			"channel '{evil}' should have been refused"
		);
	}
}

#[test]
fn preload_surface_is_fully_declared() {
	// Guards against someone trimming the shim: every member globals.ts
	// destructures must be present in the contract AND in shim.js.
	let contract = contract();
	let surface = contract["preloadSurface"].as_object().expect("preloadSurface");
	let shim = include_str!("../preload/shim.js");

	for (namespace, members) in surface {
		if namespace.starts_with('$') {
			continue;
		}
		for member in members.as_array().unwrap_or(&Vec::new()) {
			let member = member.as_str().unwrap_or_default();
			assert!(
				shim.contains(member),
				"preload shim is missing '{namespace}.{member}' required by globals.ts"
			);
		}
	}
}
