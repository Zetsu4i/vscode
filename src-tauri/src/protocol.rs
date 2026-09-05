//! `vscode-file://` custom protocol handler.
//!
//! In Electron, the main process registers the `vscode-file` scheme as
//! privileged and serves the application tree (`out/`, `node_modules`,
//! `product.json`, ...) from disk. The workbench renderer derives its ESM
//! module base URL from the window configuration:
//!
//!   vscode-file://vscode-app/<appRoot>/out/
//!
//! (see `fileUriFromPath` in src/vs/code/electron-browser/workbench/workbench.ts)
//!
//! This handler reimplements that scheme natively inside the Tauri shell with
//! WebView2's custom scheme registration (wry): fully in-process, no HTTP
//! server, no socket, no background process. Every incoming URL is mapped
//! onto the bundled client directory regardless of the authority or path
//! shape the webview produced, because the URL is rewritten several ways by
//! the boot code (`vscode-file://vscode-app/<appRoot>/out/...`) and by the
//! document URL (`vscode-file://vscode-app/out/...`).

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::OnceLock;
use tauri::http::{Request, Response, StatusCode};
use tauri::Manager;

static CLIENT_ROOT: OnceLock<PathBuf> = OnceLock::new();
static REQUEST_TRACE_COUNT: AtomicU64 = AtomicU64::new(0);

/// Resolve (and cache) the directory holding the bundled workbench client.
pub fn client_root(app: &tauri::AppHandle) -> &'static PathBuf {
    CLIENT_ROOT.get_or_init(|| resolve_client_root(app))
}

fn resolve_client_root(app: &tauri::AppHandle) -> PathBuf {
    // 1. Explicit override: CI smoke tests and development runs.
    if let Ok(dir) = std::env::var("VSTAURI_CLIENT_DIR") {
        if !dir.is_empty() {
            let path = PathBuf::from(&dir);
            if path.is_dir() {
                return path;
            }
        }
    }

    // 2. Bundled resources (installed app; set in tauri.conf.json).
    if let Ok(resource_dir) = app.path().resource_dir() {
        for candidate in ["resources/client", "client"] {
            let path = resource_dir.join(candidate);
            if path.is_dir() {
                return path;
            }
        }
    }

    // 3. Development fallback: next to the built executable.
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            for candidate in ["resources/client", "client"] {
                let path = dir.join(candidate);
                if path.is_dir() {
                    return path;
                }
            }
        }
    }

    PathBuf::new()
}

/// Entry point wired to `register_uri_scheme_protocol("vscode-file", ...)`.
pub fn serve(app: &tauri::AppHandle, request: Request<Vec<u8>>) -> Response<Vec<u8>> {
    let raw_path = request.uri().path().to_string();
    let decoded = crate::util::percent_decode(&raw_path).replace('\\', "/");
    let method = request.method().as_str().to_string();
    let root = client_root(app).clone();

    if root.as_os_str().is_empty() {
        crate::logger::log_app("error", "vscode-file: client bundle directory not found");
        return text_response(StatusCode::NOT_FOUND, "client bundle not found");
    }

    // CSS is requested in two different roles (mirrors Electron dev, where
    // the cssModules import map turns every `import './x.css'` into a blob
    // module that injects a <link>):
    //   * as a JS module graph member (Sec-Fetch-Dest: script) — serve a tiny
    //     module that calls `globalThis._VSCODE_CSS_LOAD(url)` (defined by the
    //     preload shim) which appends the real <link rel=stylesheet>;
    //   * as a stylesheet (<link>, Sec-Fetch-Dest: style) — serve text/css.
    // Without this bridge the ESM tree (which keeps its `import './x.css'`
    // statements in the dev compile) would fail to load with a MIME error.
    if decoded.ends_with(".css") && fetch_dest_is_script(&request) {
        trace_request(&method, &raw_path, 200);
        return css_module_response(&raw_path);
    }

    let rel = decoded.trim_start_matches('/').to_string();
    let mapped = match map_relative(&root, &rel) {
        Some(path) => path,
        None => {
            crate::logger::log_app("warn", &format!("vscode-file 404 (unmapped path): {}", raw_path));
            return text_response(StatusCode::NOT_FOUND, "not found");
        }
    };

    // Reject traversal and build the on-disk target path component by
    // component (never string-concatenating raw request data).
    let mut target = root.clone();
    for component in mapped.split('/') {
        if component.is_empty() {
            continue;
        }
        if component == ".." || component == "." {
            return text_response(StatusCode::FORBIDDEN, "forbidden");
        }
        target.push(component);
    }
    if target.as_os_str().is_empty() {
        return text_response(StatusCode::NOT_FOUND, "not found");
    }

    // Defense in depth: verify containment on the canonicalized paths.
    if let (Ok(canon_target), Ok(canon_root)) =
        (std::fs::canonicalize(&target), std::fs::canonicalize(&root))
    {
        if !canon_target.starts_with(&canon_root) {
            return text_response(StatusCode::FORBIDDEN, "forbidden");
        }
    }

    match std::fs::metadata(&target) {
        Ok(metadata) if metadata.is_file() => {}
        Ok(_) => return text_response(StatusCode::NOT_FOUND, "not a file"),
        Err(_) => {
            crate::logger::log_app("warn", &format!("vscode-file 404: {}", mapped));
            return text_response(StatusCode::NOT_FOUND, "not found");
        }
    }

    match std::fs::read(&target) {
        Ok(bytes) => {
            trace_request(&method, &raw_path, 200);
            file_response(&target, bytes)
        }
        Err(err) => {
            trace_request(&method, &raw_path, 500);
            crate::logger::log_app("error", &format!("vscode-file read error {}: {}", mapped, err));
            text_response(StatusCode::INTERNAL_SERVER_ERROR, "read error")
        }
    }
}

/// Map a decoded request path onto a path relative to the client root.
///
/// Handled shapes:
///   (a) `<appRoot>/out/...`  - full app root embedded (module base URL form)
///   (b) `out/...`            - direct known root (document URL form)
///   (c) `*/out/...`          - unknown prefix before a known root (URL
///                              rewriting variations across webview versions)
fn map_relative(root: &Path, rel: &str) -> Option<String> {
    let root_slash = root.to_string_lossy().replace('\\', "/").to_ascii_lowercase();
    let root_prefix = format!("{}/", root_slash);
    let rel_lower = rel.to_ascii_lowercase();

    // (a)
    if rel_lower.starts_with(&root_prefix) {
        let stripped = &rel[root_prefix.len()..];
        if is_served(stripped) {
            return Some(stripped.to_string());
        }
    }

    // (b)
    if is_served(rel) {
        return Some(rel.to_string());
    }

    // (c)
    for marker in ["/out/", "/node_modules/", "/resources/", "/extensions/"] {
        if let Some(index) = rel_lower.find(marker) {
            let candidate = &rel[index + 1..];
            if is_served(candidate) {
                return Some(candidate.to_string());
            }
        }
    }

    None
}

fn is_served(rel: &str) -> bool {
    if rel == "product.json" || rel == "nls.messages.json" {
        return true;
    }
    for prefix in ["out/", "node_modules/", "resources/", "extensions/"] {
        if rel.starts_with(prefix) {
            return true;
        }
    }
    false
}

/// Does this request come from the module loader (a `import './x.css'`
/// statement) rather than a stylesheet/<link> fetch?
fn fetch_dest_is_script(request: &Request<Vec<u8>>) -> bool {
    match request.headers().get("sec-fetch-dest").and_then(|v| v.to_str().ok()) {
        Some(dest) => dest == "script",
        None => {
            // Fallback for headers we might not see: module script fetches ask
            // for */* without a text/css preference; stylesheet fetches lead
            // with text/css.
            match request.headers().get("accept").and_then(|v| v.to_str().ok()) {
                Some(accept) if accept.starts_with("text/css") => false,
                Some(_) => true,
                None => false,
            }
        }
    }
}

/// The CSS-as-module wrapper served for `import './x.css'` members of the ESM
/// graph — the server-side twin of the blob modules that
/// `setupCSSImportMaps()` generates in Electron dev mode.
fn css_module_response(raw_path: &str) -> Response<Vec<u8>> {
    let script = format!(
        "/* vstauri css module bridge */\nglobalThis._VSCODE_CSS_LOAD && globalThis._VSCODE_CSS_LOAD('{}');\nexport default undefined;\n",
        raw_path.replace('\'', "%27")
    );
    let mut response = Response::new(script.into_bytes());
    *response.status_mut() = StatusCode::OK;
    let headers = response.headers_mut();
    headers.insert(
        tauri::http::header::CONTENT_TYPE,
        tauri::http::HeaderValue::from_static("text/javascript; charset=utf-8"),
    );
    finish_common_headers(headers);
    response
}

/// Compact request trace — the remote-debugging eyes for this shell. The
/// first 1000 requests log individually (enough to cover a full workbench
/// boot), then every 100th keeps the file bounded on hot paths.
fn trace_request(method: &str, path: &str, status: u16) {
    let n = REQUEST_TRACE_COUNT.fetch_add(1, Ordering::Relaxed);
    if n < 1000 || (n + 1).is_multiple_of(100) {
        crate::logger::log_app("trace", &format!("http {} {} -> {}", method, path, status));
    }
}

fn mime_for(path: &Path) -> &'static str {
    let extension = path
        .extension()
        .and_then(|ext| ext.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    match extension.as_str() {
        "html" | "htm" => "text/html; charset=utf-8",
        "js" | "mjs" | "cjs" => "text/javascript; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "json" | "map" => "application/json",
        "wasm" => "application/wasm",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "bmp" => "image/bmp",
        "svg" => "image/svg+xml",
        "ico" => "image/x-icon",
        "ttf" => "font/ttf",
        "otf" => "font/otf",
        "woff" => "font/woff",
        "woff2" => "font/woff2",
        "txt" => "text/plain; charset=utf-8",
        _ => "application/octet-stream",
    }
}

fn file_response(path: &Path, body: Vec<u8>) -> Response<Vec<u8>> {
    let mut response = Response::new(body);
    *response.status_mut() = StatusCode::OK;
    let headers = response.headers_mut();
    headers.insert(
        tauri::http::header::CONTENT_TYPE,
        tauri::http::HeaderValue::from_static(mime_for(path)),
    );
    finish_common_headers(headers);
    response
}

fn text_response(status: StatusCode, text: &str) -> Response<Vec<u8>> {
    let mut response = Response::new(text.as_bytes().to_vec());
    *response.status_mut() = status;
    let headers = response.headers_mut();
    headers.insert(
        tauri::http::header::CONTENT_TYPE,
        tauri::http::HeaderValue::from_static("text/plain; charset=utf-8"),
    );
    finish_common_headers(headers);
    response
}

fn finish_common_headers(headers: &mut tauri::http::HeaderMap) {
    headers.insert(
        tauri::http::header::CACHE_CONTROL,
        tauri::http::HeaderValue::from_static("no-store"),
    );
    let _ = headers.insert(
        tauri::http::HeaderName::from_static("access-control-allow-origin"),
        tauri::http::HeaderValue::from_static("*"),
    );
}
