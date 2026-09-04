//! PTY service: real pseudo-terminals via portable-pty (ConPTY on Windows,
//! openpty on Linux). All spawning is headless — a ConPTY session never shows
//! a console window. Data flows to the workbench over the bridge websocket.

use portable_pty::{native_pty_system, ChildKiller, CommandBuilder, MasterPty, PtySize};
use serde_json::Value;
use std::io::Write;

use crate::server::SharedState;
use crate::state::AppState;

pub struct SpawnedChild {
    pub child: Box<dyn portable_pty::Child + Send + Sync>,
    pub killer: Box<dyn ChildKiller + Send + Sync>,
    pub pid: Option<u32>,
}

pub struct PtyEntry {
    pub master: Box<dyn MasterPty + Send>,
    pub slave: Box<dyn portable_pty::SlavePty + Send>,
    pub writer: Box<dyn Write + Send>,
    pub spawned: Option<SpawnedChild>,
    pub initial_cwd: String,
}

impl Drop for PtyEntry {
    fn drop(&mut self) {
        if let Some(child) = &mut self.spawned {
            let _ = child.killer.kill();
        }
    }
}

// ---------------------------------------------------------------------------
// helpers
// ---------------------------------------------------------------------------

fn arg_u32(args: &[Value], i: usize) -> Result<u32, String> {
    args.get(i)
        .and_then(Value::as_u64)
        .map(|v| v as u32)
        .ok_or_else(|| format!("missing u32 arg #{i}"))
}

fn arg_opt_str(args: &[Value], i: usize) -> Option<String> {
    args.get(i).and_then(Value::as_str).map(str::to_string)
}

/// Default shell per platform, mirroring VSCode's defaults.
pub fn default_system_shell(os_override: Option<&str>) -> String {
    let os = os_override.unwrap_or(if cfg!(target_os = "windows") {
        "Windows"
    } else {
        "Linux"
    });
    match os {
        "Windows" => {
            let ps = r"C:\Windows\System32\WindowsPowerShell\v1.0\powershell.exe";
            if std::path::Path::new(ps).exists() {
                ps.to_string()
            } else {
                std::env::var("COMSPEC").unwrap_or_else(|_| "cmd.exe".to_string())
            }
        }
        "MacOS" => std::env::var("SHELL").unwrap_or_else(|_| "/bin/zsh".to_string()),
        _ => std::env::var("SHELL").unwrap_or_else(|_| "/bin/bash".to_string()),
    }
}

/// UTF-8 carry buffer: terminal chunks can split multi-byte characters;
/// decode the longest valid prefix and keep the remainder for the next chunk.
struct Utf8Carry {
    buf: Vec<u8>,
}

impl Utf8Carry {
    fn new() -> Self {
        Self {
            buf: Vec::with_capacity(16 * 1024),
        }
    }

    /// Append raw bytes, return every complete UTF-8 string that became available.
    fn push(&mut self, bytes: &[u8], out: &mut Vec<String>) {
        self.buf.extend_from_slice(bytes);
        loop {
            match std::str::from_utf8(&self.buf) {
                Ok(s) => {
                    if !s.is_empty() {
                        out.push(s.to_string());
                    }
                    self.buf.clear();
                    return;
                }
                Err(e) => {
                    let valid = e.valid_up_to();
                    if valid > 0 {
                        if let Ok(s) = std::str::from_utf8(&self.buf[..valid]) {
                            out.push(s.to_string());
                        }
                        self.buf.drain(..valid);
                    }
                    // incomplete trailing sequence: wait for more bytes unless
                    // the error is permanent (invalid bytes -> drop them)
                    if e.error_len().is_none() {
                        return;
                    }
                    let drop = e.error_len().unwrap_or(1);
                    self.buf.drain(..drop.min(self.buf.len()));
                    if self.buf.is_empty() {
                        return;
                    }
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// rpc operations
// ---------------------------------------------------------------------------

pub fn create(state: &AppState, args: &[Value]) -> Result<Value, String> {
    let id = arg_u32(args, 0)?;
    let dto = args
        .get(1)
        .ok_or_else(|| "missing shell launch config".to_string())?;
    let cols = arg_u32(args, 2).unwrap_or(80);
    let rows = arg_u32(args, 3).unwrap_or(24);

    let _executable = dto
        .get("executable")
        .and_then(Value::as_str)
        .map(str::to_string)
        .unwrap_or_else(|| default_system_shell(None));
    let cwd = match dto.get("cwd") {
        Some(Value::String(s)) => Some(s.clone()),
        Some(Value::Object(o)) => o.get("path").and_then(Value::as_str).map(str::to_string),
        _ => None,
    };
    let initial_cwd = cwd.clone().unwrap_or_else(|| {
        std::env::var("USERPROFILE")
            .or_else(|_| std::env::var("HOME"))
            .unwrap_or_default()
    });

    let pair = native_pty_system()
        .openpty(PtySize {
            rows: rows as u16,
            cols: cols as u16,
            pixel_width: 0,
            pixel_height: 0,
        })
        .map_err(|e| format!("cannot open pty: {e}"))?;

    let writer = pair
        .master
        .take_writer()
        .map_err(|e| format!("pty writer: {e}"))?;

    let mut ptys = state.ptys.lock().map_err(|_| "pty lock poisoned")?;
    if ptys.contains_key(&id) {
        return Err(format!("pty id {id} already exists"));
    }
    ptys.insert(
        id,
        PtyEntry {
            master: pair.master,
            slave: pair.slave,
            writer,
            spawned: None,
            initial_cwd,
        },
    );
    Ok(Value::Null)
}

pub async fn start(state: SharedState, args: &[Value]) -> Result<Value, String> {
    let id = arg_u32(args, 0)?;
    let dto = args.get(1).cloned().unwrap_or(Value::Null);

    // do the spawn on a blocking thread (ConPTY setup can block briefly)
    let st = state.clone();
    tokio::task::spawn_blocking(move || start_sync(&st, id, &dto))
        .await
        .map_err(|e| e.to_string())?
}

fn start_sync(state: &SharedState, id: u32, dto: &Value) -> Result<Value, String> {
    let use_shell_env = dto
        .get("useShellEnvironment")
        .and_then(Value::as_bool)
        .unwrap_or(false);

    let mut entry_state = state.ptys.lock().map_err(|_| "pty lock poisoned")?;
    let entry = entry_state
        .get_mut(&id)
        .ok_or_else(|| format!("pty {id} not found"))?;

    let executable = dto
        .get("executable")
        .and_then(Value::as_str)
        .map(str::to_string)
        .unwrap_or_else(|| default_system_shell(None));

    let mut cmd = CommandBuilder::new(&executable);

    // args: string or array
    match dto.get("args") {
        Some(Value::String(s)) => {
            cmd.arg(s);
        }
        Some(Value::Array(a)) => {
            for v in a {
                if let Some(s) = v.as_str() {
                    cmd.arg(s);
                }
            }
        }
        _ => {}
    }

    // cwd: prefer dto, fall back to the initial cwd chosen at create
    let cwd = match dto.get("cwd") {
        Some(Value::String(s)) => Some(s.clone()),
        Some(Value::Object(o)) => o.get("path").and_then(Value::as_str).map(str::to_string),
        _ => None,
    }
    .unwrap_or_else(|| entry.initial_cwd.clone());
    if !cwd.is_empty() && std::path::Path::new(&cwd).is_dir() {
        cmd.cwd(&cwd);
    }

    if use_shell_env {
        cmd.env_clear();
        for (k, v) in std::env::vars() {
            cmd.env(k, v);
        }
    }
    cmd.env("TERM", "xterm-256color");
    cmd.env("COLORTERM", "truecolor");

    let child = entry
        .slave
        .spawn_command(cmd)
        .map_err(|e| format!("failed to spawn '{executable}': {e}"))?;

    let pid = child.process_id();
    let killer = child.clone_killer();
    let mut reader = entry
        .master
        .try_clone_reader()
        .map_err(|e| format!("pty reader: {e}"))?;

    entry.spawned = Some(SpawnedChild { child, killer, pid });

    // output pump: pty bytes -> utf8 -> websocket broadcast
    let pump_state = state.clone();
    std::thread::spawn(move || {
        let mut carry = Utf8Carry::new();
        let mut buf = [0u8; 8192];
        loop {
            match reader.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => {
                    let mut chunks: Vec<String> = Vec::new();
                    carry.push(&buf[..n], &mut chunks);
                    for data in chunks {
                        pump_state.broadcast(
                            "pty.data",
                            serde_json::json!({ "id": id, "data": data, "trackCommit": false }),
                        );
                    }
                }
                Err(_) => break,
            }
        }
    });

    // exit monitor: takes ownership of the child OUTSIDE the lock (never hold
    // the pty mutex across a blocking wait), then blocks in wait()
    let exit_state = state.clone();
    std::thread::spawn(move || {
        let mut spawned = {
            let mut ptys = exit_state.ptys.lock().ok()?;
            let entry = ptys.get_mut(&id)?;
            entry.spawned.take()?
        };
        let exit_code = match spawned.child.wait() {
            Ok(status) => status.exit_code() as i64,
            Err(_) => -1,
        };
        if let Ok(mut ptys) = exit_state.ptys.lock() {
            ptys.remove(&id);
        }
        exit_state.broadcast(
            "pty.exit",
            serde_json::json!({ "id": id, "exitCode": exit_code }),
        );
        Some(())
    });

    // ready event
    let ready_cwd = cwd;
    state.broadcast(
        "pty.ready",
        serde_json::json!({ "id": id, "pid": pid.unwrap_or(0), "cwd": ready_cwd }),
    );

    Ok(Value::Null)
}

pub fn input(state: &AppState, args: &[Value]) -> Result<Value, String> {
    let id = arg_u32(args, 0)?;
    let data = arg_opt_str(args, 1).unwrap_or_default();
    let mut ptys = state.ptys.lock().map_err(|_| "pty lock poisoned")?;
    let entry = ptys
        .get_mut(&id)
        .ok_or_else(|| format!("pty {id} not found"))?;
    entry
        .writer
        .write_all(data.as_bytes())
        .map_err(|e| e.to_string())?;
    entry.writer.flush().ok();
    Ok(Value::Null)
}

pub fn send_signal(_state: &AppState, _args: &[Value]) -> Result<Value, String> {
    // portable-pty exposes only kill; platform signals land in the roadmap
    Ok(Value::Null)
}

pub fn resize(state: &AppState, args: &[Value]) -> Result<Value, String> {
    let id = arg_u32(args, 0)?;
    let cols = arg_u32(args, 1).unwrap_or(80);
    let rows = arg_u32(args, 2).unwrap_or(24);
    let pw = arg_u32(args, 3).unwrap_or(0);
    let ph = arg_u32(args, 4).unwrap_or(0);
    let ptys = state.ptys.lock().map_err(|_| "pty lock poisoned")?;
    let entry = ptys.get(&id).ok_or_else(|| format!("pty {id} not found"))?;
    entry
        .master
        .resize(PtySize {
            rows: rows as u16,
            cols: cols as u16,
            pixel_width: pw as u16,
            pixel_height: ph as u16,
        })
        .map_err(|e| e.to_string())?;
    Ok(Value::Null)
}

pub fn shutdown(state: &AppState, args: &[Value]) -> Result<Value, String> {
    let id = arg_u32(args, 0)?;
    if let Ok(mut ptys) = state.ptys.lock() {
        if let Some(mut entry) = ptys.remove(&id) {
            if let Some(child) = &mut entry.spawned {
                let _ = child.killer.kill();
            }
        }
    }
    Ok(Value::Null)
}

pub async fn cwd(state: &AppState, args: &[Value]) -> Result<Value, String> {
    let id = arg_u32(args, 0)?;
    let pid = state.ptys.lock().ok().and_then(|ptys| {
        ptys.get(&id)
            .and_then(|e| e.spawned.as_ref())
            .and_then(|s| s.pid)
    });

    #[cfg(target_os = "linux")]
    if let Some(pid) = pid {
        let link = format!("/proc/{pid}/cwd");
        if let Ok(target) = std::fs::read_link(link) {
            return Ok(Value::String(target.to_string_lossy().to_string()));
        }
    }
    #[cfg(not(target_os = "linux"))]
    let _ = pid;

    let initial = state
        .ptys
        .lock()
        .ok()
        .and_then(|ptys| ptys.get(&id).map(|e| e.initial_cwd.clone()))
        .unwrap_or_default();
    Ok(Value::String(initial))
}
