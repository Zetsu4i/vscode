//! Minimal append-only file logger. Every production code path in this shell
//! is instrumented (AGENTS.md technical rule: "Every replacement service must
//! be instrumented with logs"). No `unwrap`/`panic` on logging failures: the
//! logger silently disables itself if the file cannot be opened or written.

use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

static LOG_FILE: Mutex<Option<File>> = Mutex::new(None);
static LOG_PATH: Mutex<Option<PathBuf>> = Mutex::new(None);

/// Open (or create) `vstauri.log` inside the given logs directory.
pub fn init(logs_dir: &Path) {
    if let Err(err) = std::fs::create_dir_all(logs_dir) {
        eprintln!("[vstauri] cannot create logs dir {:?}: {}", logs_dir, err);
    }
    let path = logs_dir.join("vstauri.log");
    match OpenOptions::new().create(true).append(true).open(&path) {
        Ok(file) => {
            if let Ok(mut guard) = LOG_FILE.lock() {
                *guard = Some(file);
            }
            if let Ok(mut guard) = LOG_PATH.lock() {
                *guard = Some(path);
            }
            log_app("info", &format!("logger initialized"));
        }
        Err(err) => {
            eprintln!("[vstauri] cannot open log file {:?}: {}", path, err);
        }
    }
}

/// Log file location (reserved for the Phase 2 diagnostics command surface).
#[allow(dead_code)]
pub fn log_path() -> Option<PathBuf> {
    LOG_PATH.lock().ok().and_then(|guard| guard.clone())
}

fn write_line(prefix: &str, level: &str, message: &str) {
    let line = format!("[{}] [{}] [{}] {}\n", crate::util::unix_timestamp(), prefix, level, message);
    if let Ok(mut guard) = LOG_FILE.lock() {
        if let Some(file) = guard.as_mut() {
            // Write failures disable nothing: we keep the file open and retry
            // next time; a permanently broken file just drops log lines.
            let _ = file.write_all(line.as_bytes());
        }
    }
}

/// A log line originating from the Rust shell itself.
pub fn log_app(level: &str, message: &str) {
    write_line("app", level, message);
}

/// A log line forwarded from the renderer (shim.js `vscode_log` command).
pub fn log_renderer(level: &str, message: &str) {
    write_line("renderer", level, message);
}
