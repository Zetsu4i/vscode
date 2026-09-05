/*---------------------------------------------------------------------------------------------
 *  Loopback-only static file server for the VS Code compile output (`out/`).
 *
 *  Phase 1 intentionally loads the workbench over HTTP instead of embedding the assets:
 *  the directory is produced by `npm run compile` and does not exist in a fresh clone,
 *  and `npm run watch` can regenerate it while the shell keeps running.
 *
 *  When `out/` is missing the server answers every request with a diagnostic page that
 *  explains how to produce it.
 *--------------------------------------------------------------------------------------------*/

use std::io::Read;
use std::net::TcpListener;
use std::path::{Path, PathBuf};

use tiny_http::{Header, Method, Response, Server};

pub struct ServerHandle {
	pub port: u16,
}

/// Bind an ephemeral loopback port and start serving `out_dir` on a background thread.
pub fn start(out_dir: Option<&Path>) -> Result<ServerHandle, Box<dyn std::error::Error + Send + Sync>> {
	// Bind port 0 first to pick a free port without racing, then hand it to tiny_http.
	let probe = TcpListener::bind("127.0.0.1:0")?;
	let port = probe.local_addr()?.port();
	drop(probe);

	let server = Server::http(("127.0.0.1", port))?;
	let out_dir = out_dir.map(Path::to_path_buf);

	std::thread::Builder::new()
		.name("vscode-tauri-static-server".into())
		.spawn(move || {
			for request in server.incoming_requests() {
				handle_request(request, out_dir.as_deref());
			}
		})?;

	Ok(ServerHandle { port })
}

fn handle_request(mut request: tiny_http::Request, out_dir: Option<&Path>) {
	let method = request.method().clone();
	let url = request.url().to_string();

	// Only what a browser needs to load the workbench; everything else is rejected.
	if method != Method::Get && method != Method::Head {
		let _ = request.respond(Response::from_string(String::new()).with_status_code(405));
		return;
	}

	// Drain the body so tiny_http can reuse the connection slot.
	let mut body = String::new();
	let _ = request.as_reader().read_to_string(&mut body);

	let path = url.split(['?', '#']).next().unwrap_or("");
	let relative = percent_decode(path);

	// `/` redirects to the workbench entry page served from `out/`.
	if relative == "/" || relative.is_empty() {
		match out_dir {
			Some(_) => {
				let location = "/vs/code/electron-browser/workbench/workbench.html";
				let response = Response::from_string(String::new())
					.with_status_code(302)
					.with_header(header("Location", location));
				let _ = request.respond(response);
			}
			None => respond_with_page(request, diagnostics_page_missing_build(), 200),
		}
		return;
	}

	let Some(out_dir) = out_dir else {
		respond_with_page(request, diagnostics_page_missing_build(), 200);
		return;
	};

	match safe_resolve(out_dir, &relative) {
		Some(file_path) => match std::fs::read(&file_path) {
			Ok(bytes) => {
				let mime = content_type(&file_path);
				let mut response = Response::from_data(bytes).with_header(header("Content-Type", mime));
				if method == Method::Head {
					// tiny_http has no head helper; clients tolerate a body on HEAD here.
					response = response.with_status_code(200);
				}
				let response = response.with_header(header("Cache-Control", "no-store"));
				let _ = request.respond(response);
			}
			Err(error) => {
				log::warn!("[server] failed to read `{}`: {error}", file_path.display());
				let _ = request.respond(Response::from_string("Internal server error".to_string()).with_status_code(500));
			}
		},
		None => {
			log::debug!("[server] 404: {relative}");
			let _ = request.respond(Response::from_string("Not found".to_string()).with_status_code(404));
		}
	}
}

/// Resolve `relative` inside `root`, refusing anything that escapes the root.
fn safe_resolve(root: &Path, relative: &str) -> Option<PathBuf> {
	let relative = relative.trim_start_matches('/');
	if relative.split(['/', '\\']).any(|segment| segment == "..") {
		return None;
	}

	let candidate = root.join(relative);
	let canonical_root = root.canonicalize().ok()?;
	let canonical_candidate = candidate.canonicalize().ok()?;

	if canonical_candidate.starts_with(&canonical_root) && canonical_candidate.is_file() {
		Some(canonical_candidate)
	} else {
		None
	}
}

fn percent_decode(input: &str) -> String {
	let bytes = input.as_bytes();
	let mut output = Vec::with_capacity(bytes.len());
	let mut index = 0;
	while index < bytes.len() {
		if bytes[index] == b'%' && index + 2 < bytes.len() {
			let hex = std::str::from_utf8(&bytes[index + 1..index + 3]).ok().and_then(|hex| u8::from_str_radix(hex, 16).ok());
			match hex {
				Some(byte) => {
					output.push(byte);
					index += 3;
				}
				None => {
					output.push(bytes[index]);
					index += 1;
				}
			}
		} else {
			output.push(bytes[index]);
			index += 1;
		}
	}
	String::from_utf8_lossy(&output).into_owned()
}

fn content_type(path: &Path) -> &'static str {
	match path.extension().and_then(|extension| extension.to_str()).unwrap_or("").to_ascii_lowercase().as_str() {
		"html" | "htm" => "text/html; charset=utf-8",
		"js" | "mjs" | "cjs" => "text/javascript; charset=utf-8",
		"css" => "text/css; charset=utf-8",
		"json" | "map" => "application/json; charset=utf-8",
		"svg" => "image/svg+xml",
		"png" => "image/png",
		"jpg" | "jpeg" => "image/jpeg",
		"gif" => "image/gif",
		"webp" => "image/webp",
		"ico" => "image/x-icon",
		"woff" => "font/woff",
		"woff2" => "font/woff2",
		"ttf" => "font/ttf",
		"wasm" => "application/wasm",
		_ => "application/octet-stream",
	}
}

fn header(name: &str, value: &str) -> Header {
	match Header::from_bytes(name.as_bytes(), value.as_bytes()) {
		Ok(header) => header,
		Err(error) => {
			// Unreachable for the static ASCII pairs used at call sites; never panic on it.
			log::error!("[server] invalid header `{name}: {value}`: {error}");
			// `content-type: application/octet-stream` is valid by construction (pure ASCII).
			Header::from_bytes(b"content-type", b"application/octet-stream")
				.expect("statically valid ASCII header bytes")
		}
	}
}

fn respond_with_page(request: tiny_http::Request, body: String, status: u16) {
	let _ = request.respond(
		Response::from_string(body)
			.with_status_code(status)
			.with_header(header("Content-Type", "text/html; charset=utf-8"))
			.with_header(header("Cache-Control", "no-store")),
	);
}

fn diagnostics_page_missing_build() -> String {
	r#"<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8" />
<title>VSCode-Tauri - build output missing</title>
<style>
	body { font-family: Segoe UI, Consolas, sans-serif; background: #1e1e1e; color: #cccccc; margin: 0; display: grid; place-items: center; height: 100vh; }
	.card { max-width: 680px; padding: 32px; }
	code, pre { font-family: Consolas, monospace; }
	code { background: #2d2d2d; padding: 2px 6px; border-radius: 4px; }
	pre { background: #2d2d2d; padding: 12px; border-radius: 6px; overflow: auto; }
	h1 { color: #3794ff; font-size: 20px; }
</style>
</head>
<body>
<div class="card">
	<h1>VS Code build output not found</h1>
	<p>The Tauri shell is running, but it could not find the compiled workbench (<code>out/</code>).
	Produce it with:</p>
	<pre>npm ci
npm run compile</pre>
	<p>Then start the shell again from <code>src-tauri/</code>:</p>
	<pre>cargo run</pre>
	<p>The shell looks for <code>out/</code> next to <code>src-tauri/</code>, or under
	<code>VSCODE_TAURI_OUT_DIR</code> if that variable is set.</p>
</div>
</body>
</html>"#
	.to_string()
}
