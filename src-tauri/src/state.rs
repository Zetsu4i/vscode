//! Shared application state: token, client directory, broadcast event bus,
//! the PTY registry and the file watcher registry.

use rand::Rng;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use crate::server::fs::WatcherEntry;
use crate::server::pty::PtyEntry;

// ---------------------------------------------------------------------------
// file log — release builds have no console (`windows_subsystem = "windows"`),
// so boot info and 404s are appended to a log file for post-mortem debugging.
// ---------------------------------------------------------------------------

static LOG_PATH: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();

const LOG_MAX_BYTES: u64 = 512 * 1024;

/// Install the file logger (best effort; logging never panics).
pub fn init_log(path: PathBuf) {
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    if let Ok(md) = std::fs::metadata(&path) {
        if md.len() > LOG_MAX_BYTES {
            let _ = std::fs::remove_file(&path); // truncate oversize logs
        }
    }
    let _ = LOG_PATH.set(path);
}

/// Append one line to the log file. No-op when the logger was not installed.
pub fn log(msg: &str) {
    use std::io::Write as _;
    let Some(p) = LOG_PATH.get() else { return };
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(p)
    {
        let _ = writeln!(f, "[{ts}] {msg}");
    }
}

// ---------------------------------------------------------------------------
// last opened folder — persisted so the app reopens the previous workspace on
// launch, desktop-style. Stored as a plain fsPath string (e.g. `C:/dev/proj`).
// ---------------------------------------------------------------------------

static LAST_FOLDER_PATH: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();

/// Install the persistence file location (call once at boot).
pub fn init_last_folder(path: PathBuf) {
    let _ = LAST_FOLDER_PATH.set(path);
}

pub fn load_last_folder() -> Option<String> {
    let p = LAST_FOLDER_PATH.get()?;
    std::fs::read_to_string(p)
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

pub fn store_last_folder(folder: &str) {
    if let Some(p) = LAST_FOLDER_PATH.get() {
        let _ = std::fs::write(p, folder);
    }
}

pub struct AppState {
    /// Per-session random token; every bridge call and websocket must present it.
    pub token: String,
    /// Directory containing the built vscode-web client (out/, extensions/, ...).
    pub client_dir: PathBuf,
    /// product.json contents served to the workbench as `productConfiguration`.
    pub product_json: serde_json::Value,
    /// Port the backbone bound (0 -> random assigned by the OS).
    port_bound: AtomicPort,
    /// Event bus: fanned out to every connected websocket client.
    pub events: tokio::sync::broadcast::Sender<String>,
    pub ptys: Mutex<HashMap<u32, PtyEntry>>,
    pub watchers: Mutex<HashMap<u32, WatcherEntry>>,
    /// Folder opened in the current/last session (fsPath); mirrored to disk.
    pub last_folder: Mutex<Option<String>>,
    next_watch_id: std::sync::atomic::AtomicU32,
    shutting_down: AtomicBool,
}

struct AtomicPort(std::sync::atomic::AtomicU16);

impl AppState {
    pub fn new(client_dir: PathBuf) -> Result<Arc<Self>, String> {
        let product_path = client_dir.join("product.json");
        let product_json: serde_json::Value = if product_path.exists() {
            serde_json::from_slice(
                &std::fs::read(&product_path).map_err(|e| format!("read product.json: {e}"))?,
            )
            .map_err(|e| format!("parse product.json: {e}"))?
        } else {
            serde_json::json!({
                "nameShort": "Code",
                "nameLong": "Visual Studio Code",
                "applicationName": "code",
                "dataFolderName": ".vscode",
                "version": env!("CARGO_PKG_VERSION"),
                "quality": "stable"
            })
        };

        let token: String = rand::thread_rng()
            .sample_iter(&rand::distributions::Alphanumeric)
            .take(48)
            .map(char::from)
            .collect();

        let (events, _) = tokio::sync::broadcast::channel(4096);

        Ok(Arc::new(Self {
            token,
            client_dir,
            product_json,
            port_bound: AtomicPort(std::sync::atomic::AtomicU16::new(0)),
            events,
            ptys: Mutex::new(HashMap::new()),
            watchers: Mutex::new(HashMap::new()),
            last_folder: Mutex::new(load_last_folder()),
            next_watch_id: std::sync::atomic::AtomicU32::new(1),
            shutting_down: AtomicBool::new(false),
        }))
    }

    pub fn set_port(&self, port: u16) {
        self.port_bound.0.store(port, Ordering::SeqCst);
    }

    #[allow(dead_code)] // used by tooling/tests; the window gets the port at startup
    pub fn port(&self) -> u16 {
        self.port_bound.0.load(Ordering::SeqCst)
    }

    pub fn next_watch_id(&self) -> u32 {
        self.next_watch_id.fetch_add(1, Ordering::SeqCst)
    }

    /// Publish an event to all websocket clients.
    pub fn broadcast(&self, event: &str, payload: serde_json::Value) {
        if self.shutting_down.load(Ordering::SeqCst) {
            return;
        }
        let msg = serde_json::json!({ "event": event, "payload": payload }).to_string();
        let _ = self.events.send(msg);
    }

    /// Kill all ptys and stop watchers (app exit).
    pub fn shutdown(&self) {
        self.shutting_down.store(true, Ordering::SeqCst);
        if let Ok(mut ptys) = self.ptys.lock() {
            ptys.clear(); // PtyEntry Drop kills children
        }
        if let Ok(mut watchers) = self.watchers.lock() {
            watchers.clear();
        }
    }
}
