//! The backbone HTTP server.
//!
//! Serves the pristine vscode-web client and exposes the native services the
//! web workbench lacks in a browser (real file system, PTY terminal, native
//! dialogs, watcher) over a small authenticated JSON-RPC + WebSocket bridge.
//! This mirrors what the official `webClientServer.ts` does for vscode.dev,
//! except everything local runs in Rust — no Node.js, no Electron.

pub mod dialog;
pub mod fs;
pub mod pty;
pub mod sys;

use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Path as AxPath, Query as AxQuery, State as AxState};
use axum::http::{header, HeaderMap, StatusCode, Uri};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde_json::Value;
use std::collections::HashMap;
use std::sync::Arc;
use tauri::Manager;

use crate::state::AppState;

type SharedState = Arc<AppState>;

/// Start the backbone on a random loopback port and return the port once bound.
pub fn start(state: SharedState) -> Result<u16, String> {
    let (tx, rx) = std::sync::mpsc::channel::<Result<u16, String>>();
    tauri::async_runtime::spawn(async move {
        match tokio::net::TcpListener::bind("127.0.0.1:0").await {
            Ok(listener) => {
                let port = listener.local_addr().map(|a| a.port()).unwrap_or(0);
                state.set_port(port);
                let app = build_router(state);
                let _ = tx.send(Ok(port));
                if let Err(e) = axum::serve(listener, app).await {
                    eprintln!("backbone server stopped: {e}");
                }
            }
            Err(e) => {
                let _ = tx.send(Err(format!("backbone bind failed: {e}")));
            }
        }
    });
    rx.recv().map_err(|_| "backbone thread died".to_string())?
}

fn build_router(state: SharedState) -> Router {
    Router::new()
        .route("/", get(workbench))
        .route("/bridge/rpc/{method}", post(rpc))
        .route("/bridge/ws", get(ws))
        .fallback(static_files)
        .with_state(state)
}

// ---------------------------------------------------------------------------
// client location
// ---------------------------------------------------------------------------

pub const WORKBENCH_HTML: &str = "out/vs/code/browser/workbench/workbench.html";

pub fn resolve_client_dir(app: &tauri::AppHandle) -> Option<std::path::PathBuf> {
    let mut candidates: Vec<std::path::PathBuf> = Vec::new();
    if let Ok(rd) = app.path().resource_dir() {
        candidates.push(rd.join("client"));
    }
    candidates.extend(default_candidates());
    candidates
        .into_iter()
        .find(|p| p.join(WORKBENCH_HTML).is_file())
}

/// Client-dir resolution without a Tauri app context (smoke mode).
pub fn resolve_client_dir_generic() -> Option<std::path::PathBuf> {
    default_candidates()
        .into_iter()
        .find(|p| p.join(WORKBENCH_HTML).is_file())
}

fn default_candidates() -> Vec<std::path::PathBuf> {
    let mut candidates: Vec<std::path::PathBuf> = Vec::new();
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            candidates.push(dir.join("client"));
            candidates.push(dir.join("../../vscode-web")); // dev: target/<profile> -> src-tauri -> repo
        }
    }
    if let Ok(cwd) = std::env::current_dir() {
        candidates.push(cwd.join("vscode-web")); // repo root (gulp vscode-web output)
        candidates.push(cwd.join("upstream/../vscode-web"));
    }
    candidates
}

// ---------------------------------------------------------------------------
// workbench page
// ---------------------------------------------------------------------------

/// Rendered a visible banner if the workbench never reaches a running state
/// (the "blank window" failure mode) — release builds have no console, so
/// without this the only symptom was an empty page. 15s: slow disks/machines
/// still boot the workbench well within this budget.
const BOOT_WATCHDOG: &str = r#"(function(){var e=[];window.addEventListener('error',function(ev){e.push(String((ev&&ev.message)||'error'));});window.addEventListener('unhandledrejection',function(){e.push('unhandledrejection');});setTimeout(function(){if(document.querySelector('.monaco-workbench'))return;var p=document.createElement('pre');p.style.cssText='position:fixed;left:0;right:0;bottom:0;z-index:2147483647;margin:0;padding:10px 14px;background:#181818;color:#d7d7d7;font:12px/1.5 monospace;white-space:pre-wrap;border-top:1px solid #454545';p.textContent='VSTauri: workbench did not finish starting.\nerrors: '+(e.length?e.join(' | '):'none captured');(document.body||document.documentElement).appendChild(p);},15000);})();"#;

async fn workbench(AxState(state): AxState<SharedState>) -> Response {
    let path = state.client_dir.join(WORKBENCH_HTML);
    let template = match std::fs::read_to_string(&path) {
        Ok(t) => t,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("workbench.html missing: {e}"),
            )
                .into_response()
        }
    };

    let nonce = random_hex(32);
    let product = &state.product_json;
    let upstream_version = product
        .get("version")
        .and_then(Value::as_str)
        .unwrap_or("unknown")
        .to_string();

    let config = serde_json::json!({
        "serverBasePath": "/",
        "_wrapWebWorkerExtHostInIframe": true,
        "enableWorkspaceTrust": true,
        "productConfiguration": product,
        "callbackRoute": "/"
    });

    let platform = if cfg!(target_os = "windows") {
        "windows"
    } else if cfg!(target_os = "macos") {
        "macos"
    } else {
        "linux"
    };

    // Built by the JSON serializer so the emitted object literal is always
    // valid JavaScript. (A hand-rolled format! here once emitted literal
    // {{...}} — a syntax error that silently killed the bridge bootstrap.)
    let vstauri = serde_json::json!({
        "token": state.token.as_str(),
        "version": env!("CARGO_PKG_VERSION"),
        "upstreamVersion": upstream_version,
        "upstreamCommit": product.get("commit").and_then(Value::as_str).unwrap_or(""),
        "platform": platform,
    });

    let inject = format!(
        "<script nonce=\"{nonce}\">globalThis.__VSTAURI__={vstauri};{watchdog}</script>",
        nonce = nonce,
        vstauri = vstauri,
        watchdog = BOOT_WATCHDOG,
    );

    // server-side rendering, exactly like webClientServer.ts does upstream
    let html = template
        .replace(
            "<body aria-label=\"\">",
            &format!("<body aria-label=\"\">{inject}"),
        )
        .replace("{{WORKBENCH_SCRIPT_NONCE}}", &nonce)
        .replace(
            "{{WORKBENCH_WEB_CONFIGURATION}}",
            &html_attribute_encode(&config.to_string()),
        )
        .replace("{{WORKBENCH_WEB_BASE_URL}}", "")
        .replace("{{WORKBENCH_AUTH_SESSION}}", "")
        // upstream stable ships an empty NLS url and relies on the fallback
        // script below it; we point both at the bundled messages instead so
        // the app stays fully offline.
        .replace("{{WORKBENCH_NLS_FALLBACK_URL}}", "/out/nls.messages.js")
        .replace("{{WORKBENCH_NLS_URL}}", "/out/nls.messages.js");

    let csp = create_workbench_content_security_policy(&nonce);
    // axum's IntoResponse for `String` defaults to `text/plain` — WebView2
    // would then render the HTML source as literal text (the "window full of
    // HTML" bug). Serve the workbench as HTML explicitly.
    let mut res = (
        [(
            header::CONTENT_TYPE,
            header::HeaderValue::from_static("text/html; charset=utf-8"),
        )],
        html,
    )
        .into_response();
    res.headers_mut().insert(
        header::CONTENT_SECURITY_POLICY,
        header::HeaderValue::from_str(&csp).expect("csp header"),
    );
    res.headers_mut().insert(
        header::CACHE_CONTROL,
        header::HeaderValue::from_static("no-cache"),
    );
    res
}

/// Mirrors `createWorkbenchContentSecurityPolicy` for the local (no remote
/// authority) case.
fn create_workbench_content_security_policy(script_nonce: &str) -> String {
    format!(
        "default-src 'self'; \
         img-src 'self' https: data: blob:; \
         media-src 'self'; \
         script-src 'self' 'unsafe-eval' blob: 'nonce-{script_nonce}' \
         'sha256-daEgfo2VIXpx2Np71KqCCbkeQwv+68vPrx54XRcbdcs=' \
         'sha256-/r7rqQ+yrxt57sxLuQ6AMYcy/lUpvAIzHjIJt/OeLWU='; \
         child-src 'self'; \
         frame-src 'self' https://*.vscode-cdn.net data:; \
         worker-src 'self' data: blob:; \
         style-src 'self' 'unsafe-inline'; \
         connect-src 'self' ws: wss: https:; \
         font-src 'self' blob:; \
         manifest-src 'self';"
    )
}

fn html_attribute_encode(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

fn random_hex(len: usize) -> String {
    use rand::Rng;
    let mut rng = rand::thread_rng();
    (0..len)
        .map(|_| format!("{:x}", rng.gen::<u8>() % 16))
        .collect()
}

// ---------------------------------------------------------------------------
// static client files
// ---------------------------------------------------------------------------

fn mime_for(path: &std::path::Path) -> &'static str {
    match path.extension().and_then(|e| e.to_str()).unwrap_or("") {
        "js" | "mjs" | "cjs" => "text/javascript",
        "css" => "text/css",
        "html" => "text/html",
        "json" => "application/json",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "svg" => "image/svg+xml",
        "ico" => "image/x-icon",
        "woff" => "font/woff",
        "woff2" => "font/woff2",
        "ttf" => "font/ttf",
        "mp3" => "audio/mpeg",
        "wasm" => "application/wasm",
        "map" => "application/json",
        "scm" => "text/plain",
        "txt" => "text/plain",
        _ => "application/octet-stream",
    }
}

async fn static_files(AxState(state): AxState<SharedState>, uri: Uri) -> Response {
    let raw = uri.path().trim_start_matches('/');
    let raw = raw.strip_prefix("resources/server/").unwrap_or(raw); // package puts icons at root

    // path traversal guard
    let segments: Vec<&str> = raw.split('/').filter(|s| !s.is_empty()).collect();
    if segments.is_empty() || segments.iter().any(|s| *s == ".." || *s == ".") {
        return StatusCode::NOT_FOUND.into_response();
    }

    let mut full = state.client_dir.clone();
    for seg in &segments {
        full.push(seg);
    }

    match tokio::fs::read(&full).await {
        Ok(bytes) => {
            let mime = mime_for(&full);
            (
                [
                    (header::CONTENT_TYPE, mime.to_string()),
                    (header::CACHE_CONTROL, "no-cache".to_string()),
                ],
                bytes,
            )
                .into_response()
        }
        Err(_) => {
            crate::state::log(&format!(
                "404 {} (client dir: {})",
                uri.path(),
                state.client_dir.display()
            ));
            (StatusCode::NOT_FOUND, "Not found").into_response()
        }
    }
}

// ---------------------------------------------------------------------------
// bridge: rpc + websocket
// ---------------------------------------------------------------------------

fn authorized(state: &AppState, headers: &HeaderMap, query_token: Option<&str>) -> bool {
    if let Some(t) = headers.get("x-vstauri-token").and_then(|v| v.to_str().ok()) {
        return t == state.token;
    }
    if let Some(t) = query_token {
        return t == state.token;
    }
    false
}

async fn rpc(
    AxState(state): AxState<SharedState>,
    AxPath(method): AxPath<String>,
    headers: HeaderMap,
    body: String,
) -> Response {
    if !authorized(&state, &headers, None) {
        return StatusCode::UNAUTHORIZED.into_response();
    }

    let args: Vec<Value> = serde_json::from_str(&body).unwrap_or_default();
    let result: Result<Value, String> = match method.as_str() {
        "fs.stat" => fs::stat(&args).await,
        "fs.mkdir" => fs::mkdir(&args).await,
        "fs.readdir" => fs::readdir(&args).await,
        "fs.delete" => fs::delete(&args).await,
        "fs.rename" => fs::rename(&args).await,
        "fs.copy" => fs::copy(&args).await,
        "fs.readFile" => fs::read_file(&args).await,
        "fs.writeFile" => fs::write_file(&args).await,
        "fs.watch" => fs::watch(state.clone(), &args).await,
        "fs.unwatch" => fs::unwatch(&state, &args).await,
        "pty.create" => pty::create(&state, &args),
        "pty.start" => pty::start(state.clone(), &args).await,
        "pty.input" => pty::input(&state, &args),
        "pty.sendSignal" => pty::send_signal(&state, &args),
        "pty.resize" => pty::resize(&state, &args),
        "pty.shutdown" => pty::shutdown(&state, &args),
        "pty.clearBuffer" => Ok(Value::Null),
        "pty.acknowledgeDataEvent" => Ok(Value::Null),
        "pty.cwd" => pty::cwd(&state, &args).await,
        "sys.defaultShell" => sys::default_shell(&args),
        "sys.terminalProfiles" => sys::terminal_profiles(&args),
        "sys.env" => sys::env(),
        "dialog.pick" => dialog::pick(&args).await,
        "app.info" => Ok(serde_json::json!({
            "version": env!("CARGO_PKG_VERSION"),
            "upstreamVersion": state.product_json.get("version").and_then(Value::as_str).unwrap_or("unknown"),
        })),
        other => Err(format!("unknown bridge method: {other}")),
    };

    let body = match result {
        Ok(ok) => serde_json::json!({ "ok": ok, "err": Value::Null }),
        Err(err) => serde_json::json!({ "ok": Value::Null, "err": err }),
    };
    Json(body).into_response()
}

async fn ws(
    AxState(state): AxState<SharedState>,
    AxQuery(q): AxQuery<HashMap<String, String>>,
    upgrade: WebSocketUpgrade,
) -> Response {
    if q.get("token").map(|t| t.as_str()) != Some(state.token.as_str()) {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    upgrade.on_upgrade(move |socket| ws_socket(socket, state))
}

async fn ws_socket(mut socket: WebSocket, state: SharedState) {
    let mut rx = state.events.subscribe();
    loop {
        tokio::select! {
            event = rx.recv() => {
                match event {
                    Ok(msg) => {
                        if socket.send(Message::Text(msg.into())).await.is_err() {
                            break;
                        }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                    Err(_) => break,
                }
            }
            msg = socket.recv() => {
                match msg {
                    Some(Ok(Message::Text(_))) => { /* client->server messages unused */ }
                    Some(Ok(Message::Ping(_))) | Some(Ok(Message::Pong(_))) => {}
                    Some(Ok(_)) => {}
                    Some(Err(_)) | None => break,
                }
            }
        }
    }
}
