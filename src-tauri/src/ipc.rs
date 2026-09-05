//! IPC routing and contract capture (Phase 1 + Phase 2 instrumentation).
//!
//! Every `window.vscode.ipcRenderer.send/invoke` from the workbench arrives
//! here with its original `vscode:` channel name. Two things happen:
//!
//! 1. The call is appended to `ipc-calls.jsonl` (next to vstauri.log). This
//!    is the raw material for the Phase 2 IPC contract extraction: after a
//!    real boot on Windows the file contains every channel, argument shape
//!    and call frequency the workbench actually uses.
//! 2. Channels with implemented Rust handlers are answered; everything else
//!    resolves to `null` and is logged as unhandled, which lets the
//!    workbench fall through to degraded-but-alive code paths instead of
//!    crashing.

use serde_json::{json, Value};
use std::collections::HashMap;
use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::Path;
use std::sync::Mutex;

struct IpcState {
    log: Option<File>,
    counts: HashMap<String, u64>,
}

static IPC: Mutex<Option<IpcState>> = Mutex::new(None);

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

/// Route an ipcRenderer call. `kind` is `"send"` (fire and forget) or
/// `"invoke"` (expects a response).
pub fn route(channel: &str, args: &[Value], kind: &str) -> Result<Value, String> {
    log_call(channel, args, kind);

    match channel {
        // The one channel the original preload itself calls during boot
        // (preload.ts resolveShellEnv). Without an answer the environment
        // resolution promise hangs forever, so answer with the user env.
        "vscode:fetchShellEnv" => Ok(crate::config::user_env()),

        // Everything else: Phase 2+ will grow this table channel by channel.
        _ => Ok(Value::Null),
    }
}

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
