//! IPC routing, contract capture and the `vscode:` protocol server (Phase 1/2).
//!
//! Two IPC surfaces exist side by side:
//!
//! 1. **Plain ipcRenderer calls** (`send`/`invoke`, `vscode:`-prefixed channel
//!    strings). Every call is logged to `ipc-calls.jsonl` for contract
//!    extraction, and simple channels are answered natively.
//! 2. **The main-process message protocol** — the transport behind the
//!    renderer's `MainProcessService` (src/vs/base/parts/ipc/electron-browser/
//!    ipc.electron.ts). The renderer connects with `ipcRenderer.send('vscode:hello')`
//!    and then exchanges binary frames over `ipcRenderer.send('vscode:message',
//!    ArrayBuffer)` / the `vscode:message` event. This module implements the
//!    electron-main side of that protocol natively in Rust:
//!
//!    * `vscode:hello`    -> immediately send the `Initialize` (200) frame, the
//!      exact handshake `ChannelServer` performs in Electron (src/vs/base/
//!      parts/ipc/common/ipc.ts, constructor: `sendResponse({ type: 200 })`).
//!    * request frames    -> parse `[type, id, channelName, name]` + arg, route
//!      to the channel registry below, answer with `[201, id]` + data.
//!    * unregistered channels -> reject with `[203, id]` + error object. In
//!      Electron, requests to a channel that never registers reject after the
//!      1s pending-request timeout with a "channel not found" style error; we
//!      reject immediately with the same shape so callers take their existing
//!      error paths and the call lands in the contract log.
//!    * `EventListen` (102) requests are registered (so Rust can fire them
//!      later via `[204, id]` frames) but never answered, exactly like a
//!      server channel that simply never emits.
//!
//! Frame codec (mirror of `serialize`/`deserialize` in ipc.ts):
//!   frame   = serialize(header) || serialize(body)
//!   value   = tag:byte + payload
//!   Undefined (0) | String (1) len:VQL bytes | Buffer (2) | VSBuffer (3) |
//!   Array (4) count:VQL values | Object (5) len:VQL json | Int (6) value:VQL
//!   VQL = 7-bit little-endian base-128 varint, high bit = "more".
//!
//! Binary transport: WebView2 has no Electron MessagePort for us yet, so the
//! shim base64-encodes `vscode:message` ArrayBuffers into the `vscode_ipc`
//! Tauri command and Rust delivers event frames by evaluating
//! `window.__VSTAURI_DISPATCH__('vscode:message', '<base64>')` on the webview.

use serde_json::{json, Value};
use std::collections::HashMap;
use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{LazyLock, Mutex, OnceLock};

use tauri::Manager;

/// Marker object key lifting binary VSBuffer payloads through
/// serde_json::Value (see encode_value/decode_value).
const VSBUFFER_KEY: &str = "__vsc_vsbuffer_b64__";

/// Wrap raw bytes as a VSBuffer marker value (encoded as DataType::VSBuffer
/// on the wire). Public for channel services returning binary data
/// (localFilesystem readFile and friends).
pub fn vsbuffer(bytes: &[u8]) -> Value {
    json!({ VSBUFFER_KEY: base64_encode(bytes) })
}

/// Extract the bytes from a VSBuffer marker value (decoded from a
/// DataType::VSBuffer wire payload).
pub fn vsbuffer_bytes(value: &Value) -> Option<Vec<u8>> {
    let b64 = value.as_object()?.get(VSBUFFER_KEY)?.as_str()?;
    base64_decode(b64)
}

struct IpcState {
    log: Option<File>,
    counts: HashMap<String, u64>,
}

static IPC: Mutex<Option<IpcState>> = Mutex::new(None);
static APP_HANDLE: OnceLock<tauri::AppHandle> = OnceLock::new();
static PROTOCOL_READY: AtomicBool = AtomicBool::new(false);

/// Registered `EventListen` handlers: request id -> (channelName, event name).
static EVENT_LISTENERS: LazyLock<Mutex<HashMap<i64, (String, String)>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

/// Fire a protocol event to every renderer listener registered for
/// `(channel, event)` — the native equivalent of Electron's
/// `webContents.send`-backed `ChannelServer` event delivery ([204, id]
/// frames in ipc.ts). Called by the Mountain channel services
/// (storage/profiles/keyboardLayout/...).
pub fn fire_event(channel: &str, event: &str, payload: &Value) {
    let targets: Vec<i64> = if let Ok(guard) = EVENT_LISTENERS.lock() {
        guard
            .iter()
            .filter(|(_, (ch, ev))| ch == channel && ev == event)
            .map(|(id, _)| *id)
            .collect()
    } else {
        Vec::new()
    };
    for id in targets {
        let frame = encode_frame(&json!([204, id]), payload);
        dispatch_frame(&frame);
    }
}

/// Open the call log inside the logs directory.
pub fn init(logs_dir: &Path) {
    let path = logs_dir.join("ipc-calls.jsonl");
    let file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .map_err(|err| {
            crate::logger::log_app("warn", &format!("cannot open ipc call log {:?}: {}", path, err));
            err
        })
        .ok();
    if let Ok(mut guard) = IPC.lock() {
        *guard = Some(IpcState { log: file, counts: HashMap::new() });
    }
}

/// Store the app handle so protocol responses can be pushed into the webview.
/// Called once from `setup` after the main window exists.
pub fn init_dispatch(app: tauri::AppHandle) {
    let _ = APP_HANDLE.set(app);
    PROTOCOL_READY.store(true, Ordering::SeqCst);
}

/// Route an ipcRenderer call. `kind` is `"send"` (fire and forget) or
/// `"invoke"` (expects a response).
pub fn route(
    app: &tauri::AppHandle,
    channel: &str,
    args: &[Value],
    kind: &str,
) -> Result<Value, String> {
    log_call(channel, args, kind);

    match channel {
        // The one channel the original preload itself calls during boot
        // (preload.ts resolveShellEnv). Without an answer the environment
        // resolution promise hangs forever, so answer with the user env.
        "vscode:fetchShellEnv" => Ok(crate::config::user_env()),

        // Main-process protocol transport (see module docs).
        "vscode:hello" => {
            on_protocol_hello();
            Ok(Value::Null)
        }
        "vscode:disconnect" => {
            on_protocol_disconnect();
            Ok(Value::Null)
        }
        "vscode:message" => {
            let frame_b64 = args
                .first()
                .and_then(|v| v.as_str())
                .ok_or_else(|| "vscode:message expects a base64 frame string".to_string())?;
            on_protocol_frame(frame_b64);
            Ok(Value::Null)
        }

        // Simple ipcRenderer channels answered directly.
        "vscode:toggleDevTools" => {
            toggle_devtools(app);
            Ok(Value::Null)
        }
        "vscode:openDevTools" => {
            open_devtools(app);
            Ok(Value::Null)
        }
        "vscode:reloadWindow" => {
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.eval("window.location.reload()");
            }
            Ok(Value::Null)
        }
        "vscode:notifyZoomLevel" => {
            // webFrame zoom is already applied through vscode_set_zoom_level.
            Ok(Value::Null)
        }

        // Everything else: Phase 2+ will grow this table channel by channel.
        _ => Ok(Value::Null),
    }
}

// ---------------------------------------------------------------------------
// Devtools helpers (require the tauri "devtools" feature, enabled in Cargo.toml)
// ---------------------------------------------------------------------------

fn toggle_devtools(app: &tauri::AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        if window.is_devtools_open() {
            window.close_devtools();
        } else {
            window.open_devtools();
        }
    }
}

fn open_devtools(app: &tauri::AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        window.open_devtools();
    }
}

// ---------------------------------------------------------------------------
// vscode: protocol state machine
// ---------------------------------------------------------------------------

fn on_protocol_hello() {
    crate::logger::log_app("info", "ipc protocol: renderer connected (vscode:hello)");
    // ChannelServer parity: the Initialize frame goes out immediately so the
    // renderer's ChannelClient leaves its Uninitialized state and can flush
    // queued requests.
    let frame = encode_frame(&json!([200]), &Value::Null);
    dispatch_frame(&frame);
}

fn on_protocol_disconnect() {
    crate::logger::log_app("info", "ipc protocol: renderer disconnected (vscode:disconnect)");
    if let Ok(mut guard) = EVENT_LISTENERS.lock() {
        guard.clear();
    }
}

fn on_protocol_frame(frame_b64: &str) {
    let bytes = match base64_decode(frame_b64) {
        Some(bytes) => bytes,
        None => {
            crate::logger::log_app("warn", "ipc protocol: invalid base64 frame dropped");
            return;
        }
    };

    let mut cursor = 0usize;
    let header = match decode_value(&bytes, &mut cursor) {
        Some(header) => header,
        None => {
            crate::logger::log_app("warn", "ipc protocol: undecodable header, frame dropped");
            return;
        }
    };
    let body = decode_value(&bytes, &mut cursor).unwrap_or(Value::Null);

    let Some(header_arr) = header.as_array() else {
        return;
    };
    let msg_type = header_arr.first().and_then(Value::as_i64).unwrap_or(-1);
    let id = header_arr.get(1).and_then(Value::as_i64).unwrap_or(-1);

    match msg_type {
        100 => {
            // Promise request: [100, id, channelName, name] + arg
            let channel_name = header_arr.get(2).and_then(Value::as_str).unwrap_or("").to_string();
            let command = header_arr.get(3).and_then(Value::as_str).unwrap_or("").to_string();
            let call_desc = format!("protocol:{}:{}", channel_name, command);
            log_call(&call_desc, std::slice::from_ref(&body), "promise");
            let response = route_channel_request(app_handle(), &channel_name, &command, &body);
            let frame = match response {
                Ok(data) => encode_frame(&json!([201, id]), &data),
                Err(err) => encode_frame(
                    &json!([203, id]),
                    &json!({ "message": err, "name": "Error", "stack": null }),
                ),
            };
            dispatch_frame(&frame);
        }
        101 => { /* PromiseCancel: no active long-running requests yet. */ }
        102 => {
            // EventListen: [102, id, channelName, name] + arg. Register so Rust
            // can fire [204, id] frames later; never answered otherwise.
            let channel_name = header_arr.get(2).and_then(Value::as_str).unwrap_or("").to_string();
            let event = header_arr.get(3).and_then(Value::as_str).unwrap_or("").to_string();
            let call_desc = format!("protocol:{}:listen:{}", channel_name, event);
            log_call(&call_desc, std::slice::from_ref(&body), "listen");
            if let Ok(mut guard) = EVENT_LISTENERS.lock() {
                guard.insert(id, (channel_name, event));
            }
        }
        103 => {
            // EventDispose
            if let Ok(mut guard) = EVENT_LISTENERS.lock() {
                guard.remove(&id);
            }
        }
        other => {
            crate::logger::log_app(
                "warn",
                &format!("ipc protocol: unknown request type {} (id {})", other, id),
            );
        }
    }
}

fn app_handle() -> Option<&'static tauri::AppHandle> {
    APP_HANDLE.get()
}

/// Deliver a protocol frame (raw bytes) to the renderer as a
/// `vscode:message` ipcRenderer event.
fn dispatch_frame(frame: &[u8]) {
    if !PROTOCOL_READY.load(Ordering::SeqCst) {
        crate::logger::log_app("warn", "ipc protocol: frame dropped (webview not ready)");
        return;
    }
    let Some(app) = app_handle() else {
        return;
    };
    let Some(window) = app.get_webview_window("main") else {
        return;
    };
    let b64 = base64_encode(frame);
    // The shim decodes the base64 into a Uint8Array and dispatches it to the
    // `vscode:message` listeners (VSBuffer.wrap accepts a Uint8Array).
    let js = format!(
        "(window.__VSTAURI_DISPATCH__||function(){{}})('vscode:message',{})",
        serde_json::to_string(&b64).unwrap_or_else(|_| "\"\"".to_string())
    );
    if let Err(err) = window.eval(&js) {
        crate::logger::log_app("warn", &format!("ipc protocol: dispatch failed: {}", err));
    }
}

// ---------------------------------------------------------------------------
// Channel registry (the Phase 2 contract surface — grows over time)
// ---------------------------------------------------------------------------

fn route_channel_request(
    app: Option<&tauri::AppHandle>,
    channel: &str,
    command: &str,
    arg: &Value,
) -> Result<Value, String> {
    match (channel, command) {
        // nativeHost: the full INativeHostService surface (ProxyChannel,
        // args = [windowId, ...methodArgs])
        ("nativeHost", _) => crate::native_host::handle(app, command, arg),

        // storage: StorageDatabaseChannel (arg is an ISerializableRequest object)
        ("storage", _) => crate::storage_channel::handle(command, arg),

        // logger: electron-main LoggerChannel (arg is an argument array)
        ("logger", _) => crate::logger_channel::handle(command, arg),

        // userDataProfiles: profile CRUD over profiles.json
        ("userDataProfiles", _) => crate::profiles_channel::handle(command, arg),

        // keyboardLayout: INativeKeyboardLayoutService
        ("keyboardLayout", _) => crate::keyboard_channel::handle(command, arg),

        // localFilesystem: DiskFileSystemProviderChannel — the renderer
        // FileService's disk backend (settings, keybindings, workspace
        // files, extensions metadata, ...).
        ("localFilesystem", _) => crate::fs_channel::handle(command, arg),

        // ---- process / launch ----
        ("process", "getMainProcessPid") | ("launch", "getMainProcessPid") => {
            Ok(json!(std::process::id() as i64))
        }
        ("launch", "getOS") => Ok(json!("Windows")),
        ("launch", "getOSRelease") => Ok(json!("10.0.0")),

        // Everything else is a faithful "channel not registered" rejection —
        // same outcome as Electron's 1s pending-request timeout, and the call
        // is already in the contract log for the next implementation round.
        _ => Err(format!(
            "channel '{}' command '{}' is not registered in the VSTauri shell yet",
            channel, command
        )),
    }
}

// ---------------------------------------------------------------------------
// Frame codec (mirror of serialize/deserialize in src/vs/base/parts/ipc/common/ipc.ts)
// ---------------------------------------------------------------------------

/// Encode `serialize(header) || serialize(body)` into a frame.
fn encode_frame(header: &Value, body: &Value) -> Vec<u8> {
    let mut out = Vec::with_capacity(64);
    encode_value(header, &mut out);
    encode_value(body, &mut out);
    out
}

fn encode_value(value: &Value, out: &mut Vec<u8>) {
    match value {
        // JS serialize() maps null/true/false through the JSON.stringify
        // branch -> DataType::Object with the literal as payload.
        Value::Null | Value::Bool(_) => {
            let payload = serde_json::to_string(value).unwrap_or_else(|_| "null".to_string());
            out.push(5); // DataType::Object
            write_vql(payload.len() as u32, out);
            out.extend_from_slice(payload.as_bytes());
        }
        Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                if i >= i64::from(i32::MIN) && i <= i64::from(i32::MAX) {
                    out.push(6); // DataType::Int (VQL)
                    write_vql(i as u32, out);
                } else {
                    encode_value(&json!(n.to_string()), out);
                }
            } else {
                // Floats take the JSON object path (JS: JSON.stringify branch).
                let payload = serde_json::to_string(value).unwrap_or_default();
                encode_value(&json!(payload), out);
            }
        }
        Value::String(s) => {
            out.push(1); // DataType::String
            write_vql(s.len() as u32, out);
            out.extend_from_slice(s.as_bytes());
        }
        Value::Array(items) => {
            out.push(4); // DataType::Array
            write_vql(items.len() as u32, out);
            for item in items {
                encode_value(item, out);
            }
        }
        Value::Object(map) => {
            // VSBuffer bridge: the channel codec's binary payload type. The
            // filesystem service (localFilesystem channel) returns/accepts
            // VSBuffer; decode_value() lifts incoming buffers into this
            // marker and encode_value() writes them back as raw bytes.
            if map.len() == 1 {
                if let Some(Value::String(b64)) = map.get(VSBUFFER_KEY) {
                    if let Some(bytes) = base64_decode(b64) {
                        out.push(3); // DataType::VSBuffer
                        write_vql(bytes.len() as u32, out);
                        out.extend_from_slice(&bytes);
                        return;
                    }
                }
            }
            let payload = serde_json::to_string(value).unwrap_or_else(|_| "{}".to_string());
            out.push(5); // DataType::Object
            write_vql(payload.len() as u32, out);
            out.extend_from_slice(payload.as_bytes());
        }
    }
}

fn decode_value(bytes: &[u8], cursor: &mut usize) -> Option<Value> {
    if *cursor >= bytes.len() {
        return None;
    }
    let tag = bytes[*cursor];
    *cursor += 1;
    match tag {
        0 => Some(Value::Null), // Undefined — represented as null/undefined
        1 => {
            let len = read_vql(bytes, cursor)? as usize;
            if *cursor + len > bytes.len() {
                return None;
            }
            let s = String::from_utf8_lossy(&bytes[*cursor..*cursor + len]).into_owned();
            *cursor += len;
            Some(Value::String(s))
        }
        4 => {
            let count = read_vql(bytes, cursor)? as usize;
            let mut items = Vec::with_capacity(count.min(4096));
            for _ in 0..count {
                items.push(decode_value(bytes, cursor)?);
            }
            Some(Value::Array(items))
        }
        5 => {
            let len = read_vql(bytes, cursor)? as usize;
            if *cursor + len > bytes.len() {
                return None;
            }
            let s = String::from_utf8_lossy(&bytes[*cursor..*cursor + len]).into_owned();
            *cursor += len;
            serde_json::from_str(&s).ok().or(Some(Value::Null))
        }
        6 => {
            let v = read_vql(bytes, cursor)?;
            Some(json!(v))
        }
        2 | 3 => {
            // Buffer / VSBuffer payload — lifted into the base64 marker so
            // filesystem-style commands (writeFile buffers, readFile results)
            // round-trip binary data through the JSON layer.
            let len = read_vql(bytes, cursor)? as usize;
            if *cursor + len > bytes.len() {
                return None;
            }
            let b64 = base64_encode(&bytes[*cursor..*cursor + len]);
            *cursor += len;
            Some(json!({ VSBUFFER_KEY: b64 }))
        }
        _ => None,
    }
}

fn write_vql(mut value: u32, out: &mut Vec<u8>) {
    if value == 0 {
        out.push(0);
        return;
    }
    while value != 0 {
        let mut byte = (value & 0x7f) as u8;
        value >>= 7;
        if value > 0 {
            byte |= 0x80;
        }
        out.push(byte);
    }
}

fn read_vql(bytes: &[u8], cursor: &mut usize) -> Option<u32> {
    let mut value: u32 = 0;
    let mut shift = 0;
    loop {
        if *cursor >= bytes.len() {
            return None;
        }
        let byte = bytes[*cursor];
        *cursor += 1;
        value |= ((byte & 0x7f) as u32) << shift;
        if byte & 0x80 == 0 {
            return Some(value);
        }
        shift += 7;
        if shift > 28 {
            return None;
        }
    }
}

// ---------------------------------------------------------------------------
// Base64 (no external crates — keep the dependency tree minimal)
// ---------------------------------------------------------------------------

const B64_ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

fn base64_encode(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b0 = chunk[0] as u32;
        let b1 = *chunk.get(1).unwrap_or(&0) as u32;
        let b2 = *chunk.get(2).unwrap_or(&0) as u32;
        let triple = (b0 << 16) | (b1 << 8) | b2;
        out.push(B64_ALPHABET[(triple >> 18) as usize & 0x3f] as char);
        out.push(B64_ALPHABET[(triple >> 12) as usize & 0x3f] as char);
        out.push(if chunk.len() > 1 { B64_ALPHABET[(triple >> 6) as usize & 0x3f] as char } else { '=' });
        out.push(if chunk.len() > 2 { B64_ALPHABET[triple as usize & 0x3f] as char } else { '=' });
    }
    out
}

fn base64_decode(input: &str) -> Option<Vec<u8>> {
    fn char_value(c: u8) -> Option<u32> {
        match c {
            b'A'..=b'Z' => Some((c - b'A') as u32),
            b'a'..=b'z' => Some((c - b'a' + 26) as u32),
            b'0'..=b'9' => Some((c - b'0' + 52) as u32),
            b'+' => Some(62),
            b'/' => Some(63),
            _ => None,
        }
    }
    let filtered: Vec<u8> = input
        .bytes()
        .filter(|b| !b.is_ascii_whitespace() && *b != b'=')
        .collect();
    let mut out = Vec::with_capacity(filtered.len() * 3 / 4);
    for chunk in filtered.chunks(4) {
        let mut acc = 0u32;
        for (i, &c) in chunk.iter().enumerate() {
            acc |= char_value(c)? << (18 - 6 * i);
        }
        let pad = 4 - chunk.len();
        out.push((acc >> 16) as u8);
        if pad < 2 {
            out.push((acc >> 8) as u8);
        }
        if pad < 1 {
            out.push(acc as u8);
        }
    }
    Some(out)
}

// ---------------------------------------------------------------------------
// Contract logging
// ---------------------------------------------------------------------------

/// Byte-length-limited truncation that never splits a UTF-8 code point.
fn truncate_char_safe(input: &str, max_bytes: usize) -> String {
    if input.len() <= max_bytes {
        return input.to_string();
    }
    let mut end = max_bytes;
    while end > 0 && !input.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}...", &input[..end])
}

fn log_call(channel: &str, args: &[Value], kind: &str) {
    if let Ok(mut guard) = IPC.lock() {
        if let Some(state) = guard.as_mut() {
            let count_key = format!("{}:{}", kind, channel);
            let count = state.counts.entry(count_key.clone()).or_insert(0);
            *count += 1;

            // Serialize arguments, truncating aggressively to keep the log
            // usable for contract extraction without ballooning on
            // high-frequency channels.
            let args_str = serde_json::to_string(args).unwrap_or_else(|_| "[]".to_string());
            let args_trunc = truncate_char_safe(&args_str, 1500);

            if *count <= 20 || *count % 100 == 0 {
                // First 20 occurrences in full, then every 100th call, to keep
                // the file bounded on hot channels.
                let line = json!({
                    "ts": crate::util::unix_timestamp(),
                    "kind": kind,
                    "channel": channel,
                    "count": *count,
                    "args": Value::String(args_trunc),
                });
                if let Some(file) = state.log.as_mut() {
                    let _ = writeln!(file, "{}", line);
                }
            }
        }
    }
}
