//! Mountain: `logger` protocol channel.
//!
//! Implements the electron-main `LoggerChannel` command surface
//! (src/vs/platform/log/electron-main/logIpc.ts) natively in Rust:
//!
//!   createLogger(file, options, windowId)  -> void
//!   log(file, messages: [level, message][]) -> void
//!   consoleLog(level, args)                 -> void (app log)
//!   setLogLevel(level | [resource, level])  -> void
//!   setVisibility(resource, visibility)     -> void
//!   registerLogger(loggerResource, windowId)-> void
//!   deregisterLogger(resource)              -> void
//!   getRegisteredLoggers()                  -> ILoggerResource[]
//!   events: onDidChangeLoggers / onDidChangeLogLevel / onDidChangeVisibility
//!
//! The renderer's `LoggerChannelClient` (created in desktop.main
//! initServices) drives this channel from the very first moments of boot:
//! its constructor registers the main window logger and every log call the
//! workbench makes flows through here. Files are written under
//! <dataRoot>/logs/<sessionId>/ exactly like Electron's spdlog loggers.

use serde_json::{json, Map, Value};
use std::collections::BTreeMap;
use std::io::Write;
use std::path::PathBuf;
use std::sync::{LazyLock, Mutex, OnceLock};

static LOGS_DIR: OnceLock<PathBuf> = OnceLock::new();
static STATE: LazyLock<Mutex<LoggerState>> = LazyLock::new(|| {
    Mutex::new(LoggerState {
        loggers: BTreeMap::new(),
        console_log_count: 0u64,
    })
});

struct LoggerState {
    /// resource-path -> open append handle + registration resource.
    loggers: BTreeMap<String, (Option<std::fs::File>, Value)>,
    console_log_count: u64,
}

/// Called from config::build once the logs directory exists.
pub fn init(logs_dir: &PathBuf) {
    let _ = LOGS_DIR.set(logs_dir.join("renderer"));
    if let Err(err) = std::fs::create_dir_all(LOGS_DIR.get().unwrap()) {
        crate::logger::log_app("warn", &format!("logger channel: cannot create renderer log dir: {}", err));
    }
}

/// Resolve the on-disk location for a logger `file` URI. Logger resources
/// arrive as `UriComponents` objects; anything outside the logs dir is
/// clamped into it (defense in depth: the renderer must not be able to
/// open arbitrary files through this channel).
fn logger_path(file: &Value) -> PathBuf {
    let raw = file
        .get("path")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .replace('\\', "/");
    let base = LOGS_DIR
        .get()
        .cloned()
        .unwrap_or_else(|| std::env::temp_dir());
    let name = raw.rsplit('/').next().unwrap_or("renderer.log");
    // Only the final path component is honored, keeping writes inside the
    // logs tree regardless of the resource URI's shape.
    base.join(sanitize_file_name(name))
}

fn sanitize_file_name(name: &str) -> String {
    let cleaned: String = name
        .chars()
        .map(|c| if c.is_alphanumeric() || matches!(c, '.' | '-' | '_' | ' ') { c } else { '_' })
        .collect();
    if cleaned.is_empty() {
        "renderer.log".to_string()
    } else {
        cleaned
    }
}

fn log_level_name(level: i64) -> &'static str {
    match level {
        1 => "TRACE",
        2 => "DEBUG",
        3 => "INFO",
        4 => "WARN",
        5 => "ERROR",
        _ => "INFO",
    }
}

/// Handle one `logger` channel request. `arg` is an ARRAY (LoggerChannel
/// clients pass `[...]` argument tuples).
pub fn handle(command: &str, arg: &Value) -> Result<Value, String> {
    let args = arg.as_array().cloned().unwrap_or_default();

    match command {
        "createLogger" => {
            // createLogger(file, options, windowId): open the file for
            // appending (creating it), exactly what the spdlog-backed
            // main-process implementation does.
            let file = args.first().cloned().unwrap_or(Value::Null);
            let path = logger_path(&file);
            if let Some(parent) = path.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            let handle = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(&path)
                .map_err(|err| format!("logger channel: cannot open {:?}: {}", path, err))?;
            if let Ok(mut state) = STATE.lock() {
                let path_key = path.to_string_lossy().to_string();
                let resource = logger_resource(&file);
                state.loggers.insert(path_key, (Some(handle), resource));
            }
            Ok(Value::Null)
        }
        "log" => {
            // log(file, messages: [LogLevel, string][])
            let file = args.first().cloned().unwrap_or(Value::Null);
            let messages = args.get(1).and_then(Value::as_array);
            let path = logger_path(&file);
            let Some(messages) = messages else {
                return Ok(Value::Null);
            };
            let mut handle = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(&path)
                .map_err(|err| format!("logger channel: cannot open {:?}: {}", path, err))?;
            for message in messages {
                let level = message.get(0).and_then(Value::as_i64).unwrap_or(3);
                let text = message.get(1).and_then(Value::as_str).unwrap_or("");
                let timestamp = crate::util::format_log_timestamp();
                let _ = writeln!(handle, "[{}-{:05}] [{}] {}", timestamp.0, timestamp.1, log_level_name(level), text);
            }
            Ok(Value::Null)
        }
        "consoleLog" => {
            // consoleLog(level, args): mirror into the shell's app log
            // (bounded — the console channel can be very chatty).
            let level = args.first().and_then(Value::as_i64).unwrap_or(3);
            let text = args
                .get(1)
                .map(|a| compact_stringify(a))
                .unwrap_or_default();
            if let Ok(mut state) = STATE.lock() {
                state.console_log_count += 1;
                if state.console_log_count <= 500 || state.console_log_count % 50 == 0 {
                    crate::logger::log_app(
                        if level >= 4 { "warn" } else { "info" },
                        &format!("renderer console: {}", truncate(&text, 600)),
                    );
                }
            }
            Ok(Value::Null)
        }
        "setLogLevel" => {
            // level (number) or [resource, level] — level is session state
            // today; the persisted default lives in the window config.
            Ok(Value::Null)
        }
        "setVisibility" => {
            // setVisibility(resource, visibility)
            Ok(Value::Null)
        }
        "registerLogger" => {
            // registerLogger(loggerResource, windowId)
            let resource = args.first().cloned().unwrap_or(Value::Null);
            if resource.is_null() {
                return Ok(Value::Null);
            }
            if let Ok(mut state) = STATE.lock() {
                let path_key = logger_path(&resource.get("resource").unwrap_or(&Value::Null))
                    .to_string_lossy()
                    .to_string();
                let entry = state.loggers.remove(&path_key).unwrap_or((None, Value::Null));
                state.loggers.insert(path_key, (entry.0, logger_resource(&resource.get("resource").unwrap_or(&Value::Null))));
            }
            Ok(Value::Null)
        }
        "deregisterLogger" => {
            let resource = args.first().cloned().unwrap_or(Value::Null);
            if let Ok(mut state) = STATE.lock() {
                let path_key = logger_path(&resource).to_string_lossy().to_string();
                state.loggers.remove(&path_key);
            }
            Ok(Value::Null)
        }
        "getRegisteredLoggers" => {
            if let Ok(state) = STATE.lock() {
                Ok(json!(state
                    .loggers
                    .values()
                    .map(|(_, resource)| resource.clone())
                    .collect::<Vec<Value>>()))
            } else {
                Ok(json!([]))
            }
        }
        other => Err(format!("logger channel: call not found: {}", other)),
    }
}

fn logger_resource(resource: &Value) -> Value {
    // ILoggerResource: { resource: UriComponents, id?, name?, logLevel?, hidden? }
    let mut out = Map::new();
    if let Some(uri) = resource.get("resource") {
        out.insert("resource".to_string(), uri.clone());
    } else {
        out.insert(
            "resource".to_string(),
            json!({ "scheme": "file", "authority": "", "path": "", "query": "", "fragment": "" }),
        );
    }
    for key in ["id", "name", "logLevel", "hidden", "ext"] {
        if let Some(value) = resource.get(key) {
            out.insert(key.to_string(), value.clone());
        }
    }
    Value::Object(out)
}

fn compact_stringify(value: &Value) -> String {
    match serde_json::to_string(value) {
        Ok(s) => s,
        Err(_) => String::new(),
    }
}

fn truncate(input: &str, max: usize) -> String {
    if input.len() <= max {
        return input.to_string();
    }
    let mut end = max;
    while end > 0 && !input.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}...", &input[..end])
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn logger_paths_stay_inside_the_logs_dir() {
        let benign = logger_path(&json!({ "scheme": "file", "path": "/x/window.log" }));
        assert!(benign.file_name().unwrap() == "window.log");

        let traversal = logger_path(&json!({ "scheme": "file", "path": "/x/../../windows/system32/evil.log" }));
        // Only the final component is honored -> the name, inside LOGS_DIR.
        assert!(traversal.file_name().unwrap() == "evil.log");
        let base = LOGS_DIR.get().cloned().unwrap();
        assert!(traversal.starts_with(&base));

        let weird = logger_path(&json!({ "scheme": "file", "path": "/x/<script>:?.log" }));
        assert!(weird.starts_with(&base));
    }

    #[test]
    fn sanitize_rejects_empty_names() {
        assert_eq!(sanitize_file_name(""), "renderer.log");
        assert_eq!(sanitize_file_name("///"), "renderer.log");
    }

    #[test]
    fn level_names_match_vscode_loglevel() {
        assert_eq!(log_level_name(1), "TRACE");
        assert_eq!(log_level_name(3), "INFO");
        assert_eq!(log_level_name(5), "ERROR");
    }

    #[test]
    fn create_and_log_writes_a_file() {
        let tmp = std::env::temp_dir().join(format!("vstauri-logger-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        init(&tmp);

        let file = json!({ "scheme": "file", "path": "/logs/main.log" });
        handle("createLogger", &json!([file.clone(), { "logLevel": 3 }, 1])).expect("createLogger");
        handle("log", &json!([file, [[3, "hello workbench"], [4, "careful"]]])).expect("log");

        let path = logger_path(&json!({ "scheme": "file", "path": "/logs/main.log" }));
        let content = std::fs::read_to_string(&path).expect("log file exists");
        assert!(content.contains("hello workbench"));
        assert!(content.contains("WARN"));

        let _ = std::fs::remove_dir_all(&tmp);
    }
}
