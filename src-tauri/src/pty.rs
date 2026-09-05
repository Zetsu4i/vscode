use base64::Engine;
use portable_pty::{
    native_pty_system, Child, ChildKiller, CommandBuilder, MasterPty, PtySize, SlavePty,
};
use serde::Serialize;
use std::collections::HashMap;
use std::io::{Read, Write};
use std::sync::atomic::{AtomicU32, AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use tauri::{AppHandle, Emitter, Manager, State};

pub struct PtySession {
    pub writer: Box<dyn Write + Send>,
    pub master: Box<dyn MasterPty + Send>,
    pub killer: Box<dyn ChildKiller + Send + Sync>,
}

/// Flow-control + attach state shared between the output pump thread and the
/// `pty_attach` / `pty_ack` commands.
///
/// - `attached`: until the frontend registers its event listener (and calls
///   `pty_attach`), output is buffered in `pending` instead of emitted — the
///   shell prompt and MOTD are never lost, no matter how fast the child is.
/// - `unacked` / `paused`: byte-count backpressure. If the frontend owes us
///   more than HIGH_WATERMARK unacked bytes we stop reading the pty, which
///   applies real backpressure to the child through the pty pipe (a `yes`
///   flood no longer balloons either memory side). Acks below LOW_WATERMARK
///   resume the pump.
struct Gate {
    attached: bool,
    paused: bool,
    pending: Vec<u8>,
}

impl Default for Gate {
    fn default() -> Self {
        Gate {
            attached: false,
            paused: false,
            pending: Vec::new(),
        }
    }
}

struct FlowState {
    gate: Mutex<Gate>,
    cv: Condvar,
    unacked: AtomicUsize,
}

impl Default for FlowState {
    fn default() -> Self {
        FlowState {
            gate: Mutex::new(Gate::default()),
            cv: Condvar::new(),
            unacked: AtomicUsize::new(0),
        }
    }
}

const HIGH_WATERMARK: usize = 1024 * 1024; // pause: 1 MiB unacked
const LOW_WATERMARK: usize = 256 * 1024; // resume below 256 KiB unacked
const PENDING_CAP: usize = 1024 * 1024; // pre-attach buffer cap (drop oldest)

#[derive(Default)]
pub struct PtyState {
    next_id: AtomicU32,
    pub sessions: Mutex<HashMap<u32, PtySession>>,
    flows: Mutex<HashMap<u32, Arc<FlowState>>>,
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

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ShellInfo {
    pub name: String,
    pub path: String,
    pub default: bool,
}

fn default_shell() -> String {
    if cfg!(windows) {
        std::env::var("COMSPEC").unwrap_or_else(|_| "powershell.exe".to_string())
    } else {
        std::env::var("SHELL").unwrap_or_else(|_| "/bin/bash".to_string())
    }
}

/// Enumerate usable shells for the terminal profile picker.
///
/// Windows: cmd, Windows PowerShell, PowerShell 7+, Git Bash, WSL, plus a
/// PATH scan for less common shells (nushell, elvish, xonsh).
/// Unix: everything in /etc/shells plus the usual well-known paths.
/// The user's default shell is always present and flagged, and listed first.
#[tauri::command]
pub fn list_shells() -> Vec<ShellInfo> {
    let mut out: Vec<ShellInfo> = Vec::new();
    let default = default_shell();
    let same = |a: &str, b: &str| {
        if cfg!(windows) {
            a.eq_ignore_ascii_case(b)
        } else {
            a == b
        }
    };

    fn push(path: String, out: &mut Vec<ShellInfo>, same: &dyn Fn(&str, &str) -> bool) {
        if path.is_empty() || !std::path::Path::new(&path).is_file() {
            return;
        }
        if out.iter().any(|s| same(&s.path, &path)) {
            return;
        }
        let name = std::path::Path::new(&path)
            .file_stem()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| path.clone());
        out.push(ShellInfo {
            name,
            path,
            default: false,
        });
    }

    if cfg!(windows) {
        let sys = std::env::var("SystemRoot").unwrap_or_else(|_| "C:\\Windows".to_string());
        let sys = sys.trim_end_matches('\\').to_string();
        push(format!("{}\\System32\\cmd.exe", sys), &mut out, &same);
        push(
            format!("{}\\System32\\WindowsPowerShell\\v1.0\\powershell.exe", sys),
            &mut out,
            &same,
        );
        push(
            "C:\\Program Files\\PowerShell\\7\\pwsh.exe".to_string(),
            &mut out,
            &same,
        );
        push(
            "C:\\Program Files\\Git\\bin\\bash.exe".to_string(),
            &mut out,
            &same,
        );
        push(
            "C:\\Program Files (x86)\\Git\\bin\\bash.exe".to_string(),
            &mut out,
            &same,
        );
        push(format!("{}\\System32\\wsl.exe", sys), &mut out, &same);
        if let Ok(path_var) = std::env::var("PATH") {
            for dir in path_var.split(';') {
                if dir.is_empty() {
                    continue;
                }
                for exe in ["nu.exe", "elvish.exe", "xonsh.exe"] {
                    let p = std::path::Path::new(dir).join(exe);
                    if p.is_file() {
                        push(p.to_string_lossy().to_string(), &mut out, &same);
                    }
                }
            }
        }
    } else {
        if let Ok(shells) = std::fs::read_to_string("/etc/shells") {
            for line in shells.lines() {
                let l = line.trim();
                if !l.is_empty() && !l.starts_with('#') {
                    push(l.to_string(), &mut out, &same);
                }
            }
        }
        for p in [
            "/bin/bash",
            "/bin/zsh",
            "/bin/sh",
            "/usr/bin/fish",
            "/usr/local/bin/fish",
            "/opt/homebrew/bin/fish",
        ] {
            push(p.to_string(), &mut out, &same);
        }
    }

    // Guarantee the default shell is present and flagged, and float it first.
    if !out.iter().any(|s| same(&s.path, &default)) {
        push(default.clone(), &mut out, &same);
    }
    if let Some(s) = out.iter_mut().find(|s| same(&s.path, &default)) {
        s.default = true;
    }
    out.sort_by_key(|s| !s.default);
    out
}

/// Spawn a real pseudo-terminal session.
///
/// - Output streams to the frontend via `pty-output-{id}` (base64 chunks).
///   Output produced before the webview attaches is buffered in Rust and
///   flushed on `pty_attach` (pre-attach buffering, SideX-style).
/// - Flow control: the frontend acks consumed bytes with `pty_ack`; past 1
///   MiB unacked the pump stops reading, backpressuring the child process.
/// - Exit is announced with `pty-exit-{id}`.
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
    let flow = Arc::new(FlowState::default());

    // Output pump: pty bytes -> base64 -> frontend event, with pre-attach
    // buffering and byte-count flow control.
    let app_out = app.clone();
    let flow_pump = flow.clone();
    std::thread::spawn(move || {
        let mut buf = [0u8; 8192];
        loop {
            // Flow control: block while the frontend owes us more than the
            // high watermark. Never hold the gate lock across the blocking
            // read below (that would deadlock pty_attach on a quiet shell).
            {
                let mut g = flow_pump.gate.lock().unwrap_or_else(|e| e.into_inner());
                while g.paused {
                    g = flow_pump.cv.wait(g).unwrap_or_else(|e| e.into_inner());
                }
            }
            match reader.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => {
                    let decide = {
                        let mut g = flow_pump.gate.lock().unwrap_or_else(|e| e.into_inner());
                        if g.attached {
                            None
                        } else {
                            // Pre-attach: buffer early output (prompt, MOTD).
                            if g.pending.len() + n > PENDING_CAP {
                                let overflow = g.pending.len() + n - PENDING_CAP;
                                g.pending.drain(..overflow.min(g.pending.len()));
                            }
                            g.pending.extend_from_slice(&buf[..n]);
                            Some(())
                        }
                    };
                    if decide.is_some() {
                        continue;
                    }
                    flow_pump.unacked.fetch_add(n, Ordering::SeqCst);
                    let payload = base64::engine::general_purpose::STANDARD.encode(&buf[..n]);
                    if app_out
                        .emit(&format!("pty-output-{}", id), payload)
                        .is_err()
                    {
                        break;
                    }
                    if flow_pump.unacked.load(Ordering::SeqCst) >= HIGH_WATERMARK {
                        if let Ok(mut g) = flow_pump.gate.lock() {
                            g.paused = true;
                        }
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
        let state_exit = app_exit.state::<PtyState>();
        if let Ok(mut sessions) = state_exit.sessions.lock() {
            sessions.remove(&id);
        }
        if let Ok(mut flows) = state_exit.flows.lock() {
            flows.remove(&id);
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
    if let Ok(mut flows) = state.flows.lock() {
        flows.insert(id, flow);
    }

    let shell_name = std::path::Path::new(&shell)
        .file_name()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| shell.clone());

    Ok(PtyInfo {
        id,
        shell: shell_name,
    })
}

/// Called by the frontend once its `pty-output-{id}` listener is registered.
/// Flushes everything the child produced before attach, in order.
#[tauri::command]
pub fn pty_attach(app: AppHandle, state: State<'_, PtyState>, id: u32) -> Result<(), String> {
    let flow = {
        let flows = state.flows.lock().map_err(|_| "pty lock poisoned")?;
        flows.get(&id).cloned()
    };
    let Some(flow) = flow else {
        return Ok(()); // session already exited — nothing to flush
    };
    let payload = {
        let mut g = flow.gate.lock().map_err(|_| "pty lock poisoned")?;
        g.attached = true;
        std::mem::take(&mut g.pending)
    };
    if payload.is_empty() {
        return Ok(());
    }
    // Honest accounting: flushed bytes are unacked until the frontend acks.
    flow.unacked.fetch_add(payload.len(), Ordering::SeqCst);
    if flow.unacked.load(Ordering::SeqCst) >= HIGH_WATERMARK {
        if let Ok(mut g) = flow.gate.lock() {
            g.paused = true;
        }
    }
    let encoded = base64::engine::general_purpose::STANDARD.encode(&payload);
    let _ = app.emit(&format!("pty-output-{}", id), encoded);
    Ok(())
}

/// Flow-control ack: the frontend reports `bytes` of terminal output it has
/// written into xterm. Resumes the pump once unacked drops below the low
/// watermark.
#[tauri::command]
pub fn pty_ack(state: State<'_, PtyState>, id: u32, bytes: usize) -> Result<(), String> {
    let flow = {
        let flows = state.flows.lock().map_err(|_| "pty lock poisoned")?;
        flows.get(&id).cloned()
    };
    let Some(flow) = flow else {
        return Ok(());
    };
    flow.unacked
        .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |x| {
            Some(x.saturating_sub(bytes))
        })
        .ok();
    if flow.unacked.load(Ordering::SeqCst) < LOW_WATERMARK {
        if let Ok(mut g) = flow.gate.lock() {
            if g.paused {
                g.paused = false;
                flow.cv.notify_one();
            }
        }
    }
    Ok(())
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
