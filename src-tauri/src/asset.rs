//! `vstauri://` custom asset protocol — serves workspace files (image/media
//! previews) straight from disk.
//!
//! Security guards, in order:
//! 1. the requested path must not contain any `..` component (checked before
//!    the filesystem is touched),
//! 2. it must canonicalize to an existing regular file,
//! 3. the canonical path must live inside one of the registered workspace
//!    roots (`set_asset_roots`, canonicalized at registration time),
//! 4. reads are capped at 64 MiB.
//!
//! Paths travel in the query string (`vstauri://localhost/file?path=...`) so
//! platform-specific URI normalization (WebView2 rewrites custom schemes to
//! `https://vstauri.<host>` on Windows) can never mangle them.

use std::io::Read;
use std::path::{Component, PathBuf};
use std::sync::Mutex;
use tauri::http::header::{ACCESS_CONTROL_ALLOW_ORIGIN, CONTENT_TYPE};
use tauri::http::Response;
use tauri::Manager;

/// 64 MiB read cap — previews, not whole-video streaming.
const MAX_BYTES: u64 = 64 * 1024 * 1024;

#[derive(Default)]
pub struct AssetState {
    roots: Mutex<Vec<PathBuf>>,
}

/// (Re)register the workspace roots the asset protocol is allowed to serve.
/// Each root is canonicalized immediately, so later symlinks/cwd tricks in
/// the frontend cannot widen the sandbox.
#[tauri::command]
pub fn set_asset_roots(state: tauri::State<'_, AssetState>, roots: Vec<String>) -> Result<(), String> {
    let mut rs = state.roots.lock().map_err(|_| "asset lock poisoned")?;
    rs.clear();
    for r in roots {
        if let Ok(c) = std::fs::canonicalize(&r) {
            rs.push(c);
        }
    }
    Ok(())
}

fn mime_for(path: &std::path::Path) -> &'static str {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    match ext.as_str() {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "bmp" => "image/bmp",
        "ico" => "image/x-icon",
        "svg" => "image/svg+xml",
        "pdf" => "application/pdf",
        "mp4" => "video/mp4",
        "webm" => "video/webm",
        "mp3" => "audio/mpeg",
        "wav" => "audio/wav",
        "ogg" => "audio/ogg",
        "wasm" => "application/wasm",
        "json" => "application/json",
        "txt" | "md" | "log" => "text/plain; charset=utf-8",
        _ => "application/octet-stream",
    }
}

fn resolve(state: &AssetState, uri: &str) -> Result<(Vec<u8>, &'static str), String> {
    let raw = uri
        .split_once("path=")
        .map(|(_, q)| q.split('&').next().unwrap_or(""))
        .ok_or_else(|| "missing path".to_string())?;

    let decoded = percent_encoding::percent_decode_str(raw)
        .decode_utf8_lossy()
        .to_string();
    if decoded.is_empty() || decoded.contains('\0') {
        return Err("bad path".into());
    }
    let path = PathBuf::from(&decoded);

    // Guard 1: reject traversal before touching the filesystem.
    if path.components().any(|c| matches!(c, Component::ParentDir)) {
        return Err("traversal rejected".into());
    }

    // Guard 2: must canonicalize to an existing file.
    let canon = std::fs::canonicalize(&path).map_err(|_| "not found".to_string())?;

    // Guard 3: canonical path must live under a registered (canonical) root.
    {
        let roots = state.roots.lock().map_err(|_| "asset lock poisoned")?;
        if !roots.iter().any(|r| canon.starts_with(r)) {
            return Err("outside workspace".into());
        }
    }

    if !canon.is_file() {
        return Err("not a file".into());
    }

    // Guard 4: size cap.
    let meta = std::fs::metadata(&canon).map_err(|e| e.to_string())?;
    if meta.len() > MAX_BYTES {
        return Err("asset too large".into());
    }

    let mut buf = Vec::with_capacity(meta.len() as usize);
    std::fs::File::open(&canon)
        .and_then(|mut f| f.read_to_end(&mut buf))
        .map_err(|e| e.to_string())?;
    Ok((buf, mime_for(&canon)))
}

/// Register the protocol on the builder. `bytes.into()` deliberately
/// compiles for both the `Vec<u8>` and the `Cow<'static, [u8]>` response
/// body variants of the Tauri 2 protocol handler.
pub fn register<R: tauri::Runtime>(builder: tauri::Builder<R>) -> tauri::Builder<R> {
    builder.register_as_uri_scheme_protocol("vstauri", move |ctx, request| {
        let uri = request.uri().to_string();
        let state = ctx.app_handle().state::<AssetState>();
        let static_builder = Response::builder().header(ACCESS_CONTROL_ALLOW_ORIGIN, "*");
        match resolve(&state, &uri) {
            Ok((bytes, mime)) => static_builder
                .clone()
                .status(200)
                .header(CONTENT_TYPE, mime)
                .body(bytes.into())
                .expect("asset response builder is static"),
            Err(msg) => static_builder
                .clone()
                .status(404)
                .header(CONTENT_TYPE, "text/plain; charset=utf-8")
                .body(msg.into_bytes().into())
                .expect("asset response builder is static"),
        }
    })
}
