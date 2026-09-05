//! `vscode-file://vscode-app/...` custom URI protocol.
//!
//! Electron registers this exact scheme in `src/vs/code/electron-main/app.ts`
//! and the renderer builds every asset URL through
//! `FileAccess.asBrowserUri` -> `vscode-file://vscode-app/<abs path>`.
//! Reproducing the scheme (rather than rewriting the renderer to use
//! `asset://` or an HTTP origin) is what lets the workbench bundle load
//! byte-for-byte unmodified, per AGENTS.md constraints 2 and 11.

use std::path::{Component, Path, PathBuf};

use percent_encoding::percent_decode_str;
use tauri::http::{header, Request, Response, StatusCode};

pub const SCHEME: &str = "vscode-file";
pub const AUTHORITY: &str = "vscode-app";

/// Map a file extension to the MIME type the webview needs.
///
/// Getting this wrong is the classic failure mode: a `.js` served as
/// `text/plain` makes an ES module import fail silently and the window boots
/// blank, and a `.css` served as `text/plain` is ignored outright.
pub fn mime_for(path: &Path) -> &'static str {
	match path.extension().and_then(|e| e.to_str()).unwrap_or_default().to_ascii_lowercase().as_str() {
		"html" | "htm" => "text/html; charset=utf-8",
		"js" | "mjs" | "cjs" => "text/javascript; charset=utf-8",
		"css" => "text/css; charset=utf-8",
		"json" | "map" => "application/json; charset=utf-8",
		"wasm" => "application/wasm",
		"svg" => "image/svg+xml",
		"png" => "image/png",
		"jpg" | "jpeg" => "image/jpeg",
		"gif" => "image/gif",
		"webp" => "image/webp",
		"ico" => "image/x-icon",
		"ttf" => "font/ttf",
		"woff" => "font/woff",
		"woff2" => "font/woff2",
		"eot" => "application/vnd.ms-fontobject",
		"txt" | "md" => "text/plain; charset=utf-8",
		"sh" | "ps1" | "bat" | "cmd" | "zsh" | "fish" => "text/plain; charset=utf-8",
		_ => "application/octet-stream",
	}
}

/// Turn a `vscode-file://vscode-app/<path>` URI into an absolute filesystem
/// path, rejecting anything that escapes the allowed roots.
pub fn resolve_uri(uri: &str, roots: &[PathBuf]) -> Result<PathBuf, StatusCode> {
	let after_scheme = uri
		.split_once("://")
		.map(|(_, rest)| rest)
		.ok_or(StatusCode::BAD_REQUEST)?;

	// strip authority
	let raw_path = match after_scheme.find('/') {
		Some(idx) => &after_scheme[idx..],
		None => "/",
	};
	// drop query/fragment
	let raw_path = raw_path.split(['?', '#']).next().unwrap_or("/");

	let decoded = percent_decode_str(raw_path)
		.decode_utf8()
		.map_err(|_| StatusCode::BAD_REQUEST)?
		.into_owned();

	// On Windows the URI carries `/C:/Users/...`; drop the leading slash.
	let decoded = if cfg!(windows) {
		decoded
			.strip_prefix('/')
			.filter(|rest| {
				let bytes = rest.as_bytes();
				bytes.len() > 1 && bytes[1] == b':'
			})
			.map(str::to_string)
			.unwrap_or(decoded)
	} else {
		decoded
	};

	let candidate = PathBuf::from(&decoded);
	let normalized = normalize(&candidate);

	// `..` must never let the renderer read outside the shipped roots.
	if normalized.components().any(|c| matches!(c, Component::ParentDir)) {
		return Err(StatusCode::FORBIDDEN);
	}
	if !roots.iter().any(|root| normalized.starts_with(root)) {
		log::warn!("blocked out-of-root asset request: {}", normalized.display());
		return Err(StatusCode::FORBIDDEN);
	}

	Ok(normalized)
}

/// Lexical normalization (no filesystem access, so it also works for paths
/// that do not exist yet).
fn normalize(path: &Path) -> PathBuf {
	let mut out = PathBuf::new();
	for component in path.components() {
		match component {
			Component::CurDir => {}
			Component::ParentDir => {
				if !out.pop() {
					out.push(Component::ParentDir);
				}
			}
			other => out.push(other.as_os_str()),
		}
	}
	out
}

fn error_response(status: StatusCode, message: &str) -> Response<Vec<u8>> {
	Response::builder()
		.status(status)
		.header(header::CONTENT_TYPE, "text/plain; charset=utf-8")
		.body(message.as_bytes().to_vec())
		.unwrap_or_else(|_| Response::new(Vec::new()))
}

/// Serve one asset request. Never panics: every failure becomes a status code.
pub fn handle(request: &Request<Vec<u8>>, roots: &[PathBuf]) -> Response<Vec<u8>> {
	let uri = request.uri().to_string();

	let path = match resolve_uri(&uri, roots) {
		Ok(path) => path,
		Err(status) => return error_response(status, "invalid resource path"),
	};

	match std::fs::read(&path) {
		Ok(bytes) => {
			let mime = mime_for(&path);
			log::trace!("asset {} -> {} ({} bytes)", uri, mime, bytes.len());
			Response::builder()
				.status(StatusCode::OK)
				.header(header::CONTENT_TYPE, mime)
				.header(header::ACCESS_CONTROL_ALLOW_ORIGIN, "*")
				.header(header::CACHE_CONTROL, "no-cache")
				.body(bytes)
				.unwrap_or_else(|_| error_response(StatusCode::INTERNAL_SERVER_ERROR, "response build failed"))
		}
		Err(err) => {
			log::warn!("asset miss {}: {err}", path.display());
			error_response(StatusCode::NOT_FOUND, "not found")
		}
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn mime_types_cover_boot_files() {
		assert!(mime_for(Path::new("workbench.html")).starts_with("text/html"));
		assert!(mime_for(Path::new("workbench.js")).starts_with("text/javascript"));
		assert!(mime_for(Path::new("workbench.desktop.main.css")).starts_with("text/css"));
		assert_eq!(mime_for(Path::new("onig.wasm")), "application/wasm");
		assert!(mime_for(Path::new("product.json")).starts_with("application/json"));
	}

	#[test]
	fn traversal_is_rejected() {
		let root = PathBuf::from("/app/out");
		let err = resolve_uri("vscode-file://vscode-app/app/out/../../etc/passwd", &[root]);
		assert!(err.is_err());
	}

	#[test]
	fn resolves_inside_root() {
		let root = PathBuf::from("/app/out");
		let got = resolve_uri("vscode-file://vscode-app/app/out/vs/workbench/x.js", &[root]).unwrap();
		assert_eq!(got, PathBuf::from("/app/out/vs/workbench/x.js"));
	}

	#[test]
	fn percent_decoding_and_query_stripping() {
		let root = PathBuf::from("/app/out");
		let got = resolve_uri("vscode-file://vscode-app/app/out/a%20b/c.js?v=1", &[root]).unwrap();
		assert_eq!(got, PathBuf::from("/app/out/a b/c.js"));
	}
}
