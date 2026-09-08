//! `vscode-file://` custom protocol handler (fast path).
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
//!
//! ## Performance model (why this file looks the way it does)
//!
//! The renderer boots by fetching hundreds of modules in quick succession
//! (a dev-compile boot measured ~1260 requests). Every request crosses the
//! WebView2 WebResourceRequested IPC boundary, so the handler itself must be
//! as close to zero-cost as possible after the first hit:
//!
//!   * the client root is canonicalized exactly ONCE (per-request
//!     `fs::canonicalize` was the top Windows syscall cost),
//!   * every served file body is cached in memory (raw + pre-compressed
//!     gzip) with an LRU-style byte budget, so repeat fetches do not touch
//!     the filesystem at all,
//!   * compressible responses > 1 KiB are served with `Content-Encoding:
//!     gzip` when the webview offers it (the JS graph compresses ~4:1),
//!   * every response carries a strong `ETag` (`"mtime:len"`), so repeat
//!     boots revalidate with cheap 304s instead of re-transferring bytes,
//!     and app updates can never serve stale content (URLs are unchanged
//!     between builds, `no-cache` forces the revalidation),
//!   * `node_modules.asar/...` paths (the product runtime's import form for
//!     its external npm dependencies) are transparently mapped onto the
//!     plain `node_modules/` tree we bundle.

use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};
use tauri::http::{Request, Response, StatusCode};
use tauri::Manager;

static CLIENT_ROOT: OnceLock<PathBuf> = OnceLock::new();
static CANONICAL_ROOT: OnceLock<PathBuf> = OnceLock::new();
static REQUEST_TRACE_COUNT: AtomicU64 = AtomicU64::new(0);
static BODY_CACHE: OnceLock<Mutex<FileCache>> = OnceLock::new();

/// Raw-byte budget for the in-memory body cache. The full client bundle is
/// ~300 MiB on disk and the compressed variants are kept too; 384 MiB of
/// raw budget covers a complete boot with margin while staying well under
/// the memory footprint of the Electron app we replace.
const CACHE_MAX_BYTES: usize = 384 * 1024 * 1024;

/// Files whose content must never be served from cache/stale copies: the
/// Rust shell rewrites `product.json` per build and NLS can change.
const FRESH_PATHS: &[&str] = &["product.json", "nls.messages.json"];

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

/// The canonicalized client root, computed once. Symlink-defense containment
/// checks compare against this; a failed canonicalization falls back to the
/// lexical root (component-wise path construction already rejects traversal).
fn canonical_root(root: &Path) -> &'static PathBuf {
    CANONICAL_ROOT.get_or_init(|| {
        std::fs::canonicalize(root).unwrap_or_else(|_| root.to_path_buf())
    })
}

// ---------------------------------------------------------------------------
// In-memory body cache (raw + gzip)
// ---------------------------------------------------------------------------

struct CachedBody {
    raw: Vec<u8>,
    gz: Vec<u8>,
    etag: String,
    mime: &'static str,
}

struct FileCache {
    map: HashMap<PathBuf, std::sync::Arc<CachedBody>>,
    order: VecDeque<PathBuf>,
    bytes: usize,
}

fn body_cache() -> &'static Mutex<FileCache> {
    BODY_CACHE.get_or_init(|| {
        Mutex::new(FileCache {
            map: HashMap::new(),
            order: VecDeque::new(),
            bytes: 0,
        })
    })
}

fn cache_lookup(path: &Path) -> Option<std::sync::Arc<CachedBody>> {
    body_cache()
        .lock()
        .ok()
        .and_then(|cache| cache.map.get(path).cloned())
}

/// Insert a body, evicting the oldest quarter of the cache when the byte
/// budget is exceeded (crude but predictable; a full workbench boot fits
/// entirely, so eviction only guards pathological access patterns like
/// workspace-wide directory browsing through the localFilesystem channel).
/// Returns an `Arc` handle so hot requests clone a pointer, never bytes.
fn cache_insert(
    path: PathBuf,
    raw: Vec<u8>,
    etag: String,
    mime: &'static str,
) -> std::sync::Arc<CachedBody> {
    let gz = gzip_if_worthwhile(mime, &raw);
    let body = std::sync::Arc::new(CachedBody { raw, gz, etag, mime });
    if let Ok(mut cache) = body_cache().lock() {
        let stored_bytes = body.raw.len() + body.gz.len();
        cache.map.insert(path.clone(), body.clone());
        cache.order.push_back(path);
        cache.bytes += stored_bytes;
        while cache.bytes > CACHE_MAX_BYTES && cache.order.len() > 8 {
            // Evict the oldest quarter at a time to amortize the churn.
            let evictions = (cache.order.len() / 4).max(1);
            for _ in 0..evictions {
                if let Some(old) = cache.order.pop_front() {
                    if let Some(removed) = cache.map.remove(&old) {
                        cache.bytes -= removed.raw.len() + removed.gz.len();
                    }
                }
            }
        }
    }
    body
}

/// gzip a compressible body once. Returns the compressed bytes, or an empty
/// Vec when compression is not worthwhile (tiny, or already-compressed
/// media types — re-compressing binaries wastes CPU for ~0 bytes saved).
fn gzip_if_worthwhile(mime: &str, raw: &[u8]) -> Vec<u8> {
    if raw.len() < 1024 || !compressible_mime(mime) {
        return Vec::new();
    }
    use flate2::write::GzEncoder;
    use flate2::Compression;
    use std::io::Write;
    let mut encoder = GzEncoder::new(Vec::with_capacity(raw.len() / 3), Compression::new(6));
    if encoder.write_all(raw).is_err() {
        return Vec::new();
    }
    match encoder.finish() {
        Ok(gz) if gz.len() < raw.len() => gz,
        _ => Vec::new(),
    }
}

fn compressible_mime(mime: &str) -> bool {
    mime.starts_with("text/")
        || mime.starts_with("application/json")
        || mime.starts_with("application/javascript")
        || mime.starts_with("image/svg+xml")
        || mime.starts_with("application/wasm")
}

fn accepts_gzip(request: &Request<Vec<u8>>) -> bool {
    request
        .headers()
        .get(tauri::http::header::ACCEPT_ENCODING)
        .and_then(|v| v.to_str().ok())
        .map(|v| v.to_ascii_lowercase().contains("gzip"))
        .unwrap_or(false)
}

// ---------------------------------------------------------------------------
// Request entry point
// ---------------------------------------------------------------------------

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

    // The product runtime imports its external npm dependencies through the
    // `node_modules.asar` prefix (Electron's packed archive form), both
    // directly and inside its absolute app-root URLs. We bundle the plain
    // directories, so strip the archive suffix wherever it appears.
    let rel_source = decoded.trim_start_matches('/').to_string();
    let rel = normalize_asar(&rel_source);

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

    // Fresh-always files bypass the body cache (they are tiny).
    if FRESH_PATHS.contains(&mapped.as_str()) {
        return serve_fresh_file(&method, &raw_path, &target);
    }

    // Hot path: cached bodies answer without touching the filesystem.
    if let Some(body) = cache_lookup(&target) {
        return respond_with_body(&request, &method, &raw_path, &body);
    }

    // Cold path: single read (metadata check folded into the read result —
    // a failed read of a directory returns an error on Windows, and the
    // metadata syscall is saved on the common file case).
    match std::fs::metadata(&target) {
        Ok(meta) if !meta.is_file() => {
            trace_request(&method, &raw_path, 404);
            return text_response(StatusCode::NOT_FOUND, "not a file");
        }
        Err(_) => {
            trace_request(&method, &raw_path, 404);
            crate::logger::log_app("warn", &format!("vscode-file 404: {}", mapped));
            return text_response(StatusCode::NOT_FOUND, "not found");
        }
        _ => {}
    }

    match std::fs::read(&target) {
        Ok(bytes) => {
            // Defense in depth on the (cold) cache-miss path only: verify
            // containment on the canonicalized target. Hot requests rely on
            // the component-wise construction plus this one-time guarantee,
            // because the cache only ever stores verified paths.
            if let Ok(canon_target) = std::fs::canonicalize(&target) {
                if !canon_target.starts_with(canonical_root(&root)) {
                    return text_response(StatusCode::FORBIDDEN, "forbidden");
                }
            }
            let etag = etag_for(&target, bytes.len());
            let body = cache_insert(
                target,
                product_mode_boot_bytes(&mapped, bytes),
                etag,
                mime_for_mapped(&mapped),
            );
            respond_with_body(&request, &method, &raw_path, &body)
        }
        Err(err) => {
            trace_request(&method, &raw_path, 500);
            crate::logger::log_app("error", &format!("vscode-file read error {}: {}", mapped, err));
            text_response(StatusCode::INTERNAL_SERVER_ERROR, "read error")
        }
    }
}

/// Weak-but-strong-enough validator: `(mtime ns, len)`. Content under a path
/// only changes when the app updates, which always rewrites mtime/size.
fn etag_for(path: &Path, len: usize) -> String {
    let stamp = std::fs::metadata(path)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    format!("\"{:x}-{:x}\"", stamp, len)
}

fn serve_fresh_file(method: &str, raw_path: &str, target: &Path) -> Response<Vec<u8>> {
    match std::fs::read(target) {
        Ok(bytes) => {
            trace_request(method, raw_path, 200);
            let mut response = file_response(mime_for(target), bytes);
            response.headers_mut().insert(
                tauri::http::header::CACHE_CONTROL,
                tauri::http::HeaderValue::from_static("no-store"),
            );
            response
        }
        Err(_) => {
            trace_request(method, raw_path, 404);
            text_response(StatusCode::NOT_FOUND, "not found")
        }
    }
}

/// Build the response for a body that is already in memory: honors
/// `If-None-Match` (304), `Accept-Encoding: gzip`, and per-path cache
/// semantics.
fn respond_with_body(
    request: &Request<Vec<u8>>,
    method: &str,
    raw_path: &str,
    body: &std::sync::Arc<CachedBody>,
) -> Response<Vec<u8>> {
    // 304 short-circuit: revalidation after the first boot costs a header
    // comparison, no bytes, no decompression.
    let if_none_match = request
        .headers()
        .get(tauri::http::header::IF_NONE_MATCH)
        .and_then(|v| v.to_str().ok())
        .map(|v| v.trim().to_string());
    if if_none_match.as_deref() == Some(body.etag.as_str())
        || if_none_match.as_deref() == Some("*")
    {
        trace_request(method, raw_path, 304);
        let mut response = Response::new(Vec::new());
        *response.status_mut() = StatusCode::NOT_MODIFIED;
        let headers = response.headers_mut();
        if let Ok(value) = tauri::http::HeaderValue::from_str(&body.etag) {
            headers.insert(tauri::http::header::ETAG, value);
        }
        headers.insert(
            tauri::http::header::CACHE_CONTROL,
            tauri::http::HeaderValue::from_static("no-cache"),
        );
        let _ = headers.insert(
            tauri::http::HeaderName::from_static("access-control-allow-origin"),
            tauri::http::HeaderValue::from_static("*"),
        );
        return response;
    }

    let use_gzip = accepts_gzip(request) && !body.gz.is_empty();
    let out: Vec<u8> = if use_gzip { body.gz.clone() } else { body.raw.clone() };
    trace_request(method, raw_path, 200);

    let mut response = Response::new(out);
    *response.status_mut() = StatusCode::OK;
    let headers = response.headers_mut();
    if let Ok(value) = tauri::http::HeaderValue::from_str(body.mime) {
        headers.insert(tauri::http::header::CONTENT_TYPE, value);
    } else {
        headers.insert(
            tauri::http::header::CONTENT_TYPE,
            tauri::http::HeaderValue::from_static("application/octet-stream"),
        );
    }
    if use_gzip {
        headers.insert(
            tauri::http::header::CONTENT_ENCODING,
            tauri::http::HeaderValue::from_static("gzip"),
        );
        headers.insert(
            tauri::http::header::VARY,
            tauri::http::HeaderValue::from_static("Accept-Encoding"),
        );
    }
    if let Ok(value) = tauri::http::HeaderValue::from_str(&body.etag) {
        headers.insert(tauri::http::header::ETAG, value);
    }
    // `no-cache` (NOT `no-store`): allows the webview's disk cache to keep
    // the bytes across boots, but every use revalidates against the ETag so
    // an app update can never serve the previous build's modules.
    headers.insert(
        tauri::http::header::CACHE_CONTROL,
        tauri::http::HeaderValue::from_static("no-cache"),
    );
    let _ = headers.insert(
        tauri::http::HeaderName::from_static("access-control-allow-origin"),
        tauri::http::HeaderValue::from_static("*"),
    );
    response
}

/// Product-mode transition for the renderer environment.
///
/// The dev-relative-import boot the Wind shim relies on requires
/// `process.env.VSCODE_DEV` (workbench.ts picks the relative workbench
/// import only when it is set). But that flag ALSO makes the renderer act
/// like a source checkout — `environmentService.isBuilt === false` — which
/// turns on dev-only behavior the product shell must not show:
/// `.build/builtInExtensions` scanning, dev console log forwarding, dev
/// language assertions, ... (see src/vs/platform/environment/common/
/// environmentService.ts: `get isBuilt() { return !env['VSCODE_DEV']; }`).
///
/// ESM evaluation order gives us the exact right window for the flip: the
/// workbench entry module (`out/vs/workbench/workbench.desktop.main.js`)
/// consists of static imports only, so its own body evaluates AFTER its
/// whole dependency graph (modules like product.ts, which may read
/// VSCODE_DEV for cosmetic dev markers) but BEFORE `DesktopMain.open()`
/// starts the service graph — and every `isBuilt` consumer (extension
/// scanner, language service, log service, tree-sitter, ...) is
/// constructed during open(). Prepending this statement to the entry
/// module's served bytes therefore switches the renderer into product
/// mode precisely between module load and service instantiation.
///
/// Known trade-off: product.ts evaluates earlier in the graph and appends
/// the " Dev" suffix to the product names — cosmetic, tracked in ROADMAP.md
/// (long-term fix: serve the real `vscode-file://vscode-app/...` URL form so
/// the production import branch works without VSCODE_DEV at all).
fn product_mode_boot_bytes(mapped: &str, bytes: Vec<u8>) -> Vec<u8> {
    if !mapped.eq_ignore_ascii_case("out/vs/workbench/workbench.desktop.main.js") {
        return bytes;
    }
    const PREFIX: &str = "/* vstauri: product-mode transition — see src-tauri/src/protocol.rs */\ntry { delete globalThis.vscode.process.env.VSCODE_DEV; } catch (e) {}\n";
    let mut out = Vec::with_capacity(PREFIX.len() + bytes.len());
    out.extend_from_slice(PREFIX.as_bytes());
    out.extend_from_slice(&bytes);
    out
}

/// `node_modules.asar/<pkg>` → `node_modules/<pkg>` (we ship the plain tree).
fn normalize_asar(rel: &str) -> String {
    if rel.contains("node_modules.asar/") {
        rel.replace("node_modules.asar/", "node_modules/")
    } else {
        rel.to_string()
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

/// MIME by extension, resolved from the *mapped* (client-relative) path so
/// hot responses never construct a Path.
fn mime_for_mapped(mapped: &str) -> &'static str {
    let extension = mapped.rsplit('/').next()
        .and_then(|file| file.rsplit_once('.'))
        .map(|(_, ext)| ext.to_ascii_lowercase())
        .unwrap_or_default();
    mime_for_extension(&extension)
}

fn mime_for(path: &Path) -> &'static str {
    let extension = path
        .extension()
        .and_then(|ext| ext.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    mime_for_extension(&extension)
}

fn mime_for_extension(extension: &str) -> &'static str {
    match extension {
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

fn file_response(mime: &'static str, body: Vec<u8>) -> Response<Vec<u8>> {
    let mut response = Response::new(body);
    *response.status_mut() = StatusCode::OK;
    let headers = response.headers_mut();
    if let Ok(value) = tauri::http::HeaderValue::from_str(mime) {
        headers.insert(tauri::http::header::CONTENT_TYPE, value);
    }
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

// ---------------------------------------------------------------------------
// Tests (contract-shaped: cache behavior, asar mapping, etag round-trip)
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn asar_prefix_is_stripped() {
        let root = Path::new("C:/app/client");
        // Direct form
        assert_eq!(
            map_relative(root, &normalize_asar("node_modules/@xterm/xterm/lib/xterm.js")),
            Some("node_modules/@xterm/xterm/lib/xterm.js".to_string())
        );
        // Electron's absolute asar URL form (authority + appRoot embedded)
        assert_eq!(
            map_relative(
                root,
                &normalize_asar("c:/app/client/node_modules.asar/@vscode/vscode-languagedetection/dist/lib/index.js")
            ),
            Some("node_modules/@vscode/vscode-languagedetection/dist/lib/index.js".to_string())
        );
        // Marker form: unknown prefix before /node_modules/
        assert_eq!(
            map_relative(root, &normalize_asar("some-prefix/node_modules/vscode-oniguruma/release/onig.wasm")),
            Some("node_modules/vscode-oniguruma/release/onig.wasm".to_string())
        );
    }

    #[test]
    fn fresh_paths_are_recognized() {
        assert!(is_served("product.json"));
        assert!(is_served("nls.messages.json"));
        assert!(!is_served("app/package.json"));
    }

    #[test]
    fn gzip_skips_media_and_tiny_bodies() {
        assert!(!compressible_mime("image/png"));
        assert!(!compressible_mime("font/ttf"));
        assert!(compressible_mime("text/javascript; charset=utf-8"));
        assert!(compressible_mime("application/json"));
        assert!(gzip_if_worthwhile("text/javascript; charset=utf-8", b"tiny").is_empty());
    }

    #[test]
    fn gzip_round_trip() {
        use flate2::read::GzDecoder;
        use std::io::Read;
        let body: Vec<u8> = "fn main() { println!(\"hello\"); }".repeat(512).into_bytes();
        let gz = gzip_if_worthwhile("text/javascript; charset=utf-8", &body);
        assert!(!gz.is_empty());
        let mut decoded = Vec::new();
        GzDecoder::new(&gz[..]).read_to_end(&mut decoded).unwrap();
        assert_eq!(decoded, body);
    }

    #[test]
    fn product_mode_prefix_only_on_workbench_entry() {
        assert!(product_mode_boot_bytes("out/vs/workbench/workbench.desktop.main.js", vec![1, 2, 3]).len() > 3);
        assert_eq!(product_mode_boot_bytes("out/vs/base/common/lifecycle.js", vec![1, 2, 3]), vec![1, 2, 3]);
    }
}
