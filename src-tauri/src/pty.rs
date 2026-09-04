use base64::Engine;
use portable_pty::{native_pty_system, ChildKiller, CommandBuilder, MasterPty, PtySize};
use serde::Serialize;
use std::collections::HashMap;
use std::io::{Read, Write};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Mutex;
use tauri::{AppHandle, Emitter, Manager, State};

pub struct PtySession {
    pub writer: Box<dyn Write + Send>,
    pub master: Box<dyn MasterPty + Send>,
    pub killer: Box<dyn ChildKiller + Send + Sync>,
}

#[derive(Default)]
pub struct PtyState {
    next_id: AtomicU32,
    pub sessions: Mutex<HashMap<u32, PtySession>>,
}

impl PtyState {
    pub fn kill_all(&self) {
        if let Ok(mut sessions) = self.sessions.lock() {
            for (_, mut s) in sessions.drain() {
                let _ = s.killer.kill();
            }
        }
    }
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct PtyInfo {
    pub id: u32,
    pub shell: String,
}

fn default_shell() -> String {
    if cfg!(windows) {
        std::env::var("COMSPEC").unwrap_or_else(|_| "powershell.exe".to_string())
    } else {
        std::env::var("SHELL").unwrap_or_else(|_| "/bin/bash".to_string())
    }
}

/// Spawn a real pseudo-terminal session.
///
/// - Output streams to the frontend via `pty-output-{id}` (base64 chunks)
/// - Exit is announced with `pty-exit-{id}`
/// - The child process is owned by a dedicated monitor thread that blocks in
///   `wait()`; killing goes through a `ChildKiller` handle, so there is never
///   any mutex held across a blocking wait.
#[tauri::command]
pub fn create_pty(
    app: AppHandle,
    state: State<'_, PtyState>,
    cwd: Option<String>,
    shell: Option<String>,
) -> Result<PtyInfo, String> {
    let shell = shell.unwrap_or_else(default_shell);
    let pty_system = native_pty_system();
    let pair = pty_system
        .openpty(PtySize {
            rows: 24,
            cols: 80,
            pixel_width: 0,
            pixel_height: 0,
        })
        .map_err(|e| format!("Cannot open pty: {}", e))?;

    let mut cmd = CommandBuilder::new(&shell);
    if let Some(dir) = &cwd {
        if std::path::Path::new(dir).is_dir() {
            cmd.cwd(dir);
        }
    }
    cmd.env("TERM", "xterm-256color");
    cmd.env("COLORTERM", "truecolor");

    let mut child = pair
        .slave
        .spawn_command(cmd)
        .map_err(|e| format!("Failed to spawn shell '{}': {}", shell, e))?;

    let killer = child.clone_killer();
    let mut reader = pair
        .master
        .try_clone_reader()
        .map_err(|e| format!("pty reader: {}", e))?;
    let writer = pair
        .master
        .take_writer()
        .map_err(|e| format!("pty writer: {}", e))?;

    let id = state.next_id.fetch_add(1, Ordering::SeqCst);

    // Output pump: pty bytes -> base64 -> frontend event
    let app_out = app.clone();
    std::thread::spawn(move || {
        let mut buf = [0u8; 8192];
        loop {
            match reader.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => {
                    let payload = base64::engine::general_purpose::STANDARD.encode(&buf[..n]);
                    if app_out
                        .emit(&format!("pty-output-{}", id), payload)
                        .is_err()
                    {
                        break;
                    }
                }
                Err(_) => break,
            }
        }
    });

    // Exit monitor: owns the child, blocks in wait(), then cleans up.
    let app_exit = app.clone();
    std::thread::spawn(move || {
        let _ = child.wait(); // blocks until the shell exits or is killed
        if let Ok(mut sessions) = app_exit.state::<PtyState>().sessions.lock() {
            sessions.remove(&id);
        }
        let _ = app_exit.emit(&format!("pty-exit-{}", id), id);
    });

    let mut sessions = state.sessions.lock().map_err(|_| "pty lock poisoned")?;
    sessions.insert(
        id,
        PtySession {
            writer,
            master: pair.master,
            killer,
        },
    );

    let shell_name = std::path::Path::new(&shell)
        .file_name()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| shell.clone());

    Ok(PtyInfo {
        id,
        shell: shell_name,
    })
}

#[tauri::command]
pub fn write_pty(state: State<'_, PtyState>, id: u32, data: String) -> Result<(), String> {
    let mut sessions = state.sessions.lock().map_err(|_| "pty lock poisoned")?;
    let session = sessions.get_mut(&id).ok_or("terminal not found")?;
    session
        .writer
        .write_all(data.as_bytes())
        .map_err(|e| e.to_string())?;
    session.writer.flush().map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
pub fn resize_pty(state: State<'_, PtyState>, id: u32, rows: u16, cols: u16) -> Result<(), String> {
    let sessions = state.sessions.lock().map_err(|_| "pty lock poisoned")?;
    let session = sessions.get(&id).ok_or("terminal not found")?;
    session
        .master
        .resize(PtySize {
            rows: rows.max(1),
            cols: cols.max(1),
            pixel_width: 0,
            pixel_height: 0,
        })
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn kill_pty(state: State<'_, PtyState>, id: u32) -> Result<(), String> {
    let mut sessions = state.sessions.lock().map_err(|_| "pty lock poisoned")?;
    if let Some(mut session) = sessions.remove(&id) {
        let _ = session.killer.kill();
    }
    Ok(())
}
