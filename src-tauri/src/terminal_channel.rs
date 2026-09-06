//! Mountain: `localPty` protocol channel (Phase 5 — portable-pty).
//!
//! Implements the `IPtyService` / `IPtyHostService` surface (registered as
//! channel `localPty` in electron-main app.ts: `ProxyChannel.fromService(
//! accessor.get(ILocalPtyService))`, see src/vs/code/electron-main/app.ts
//! ~line 1447) natively in Rust on top of the `portable-pty` crate
//! (ConPTY on Windows, openpty on Unix — the same backends node-pty uses).
//!
//! Command surface (ProxyChannel: method names are command names, arg =
//! the argument array):
//!   createProcess(shellLaunchConfig, cwd, cols, rows, unicodeVersion,
//!                 env, executableEnv, options, shouldPersist,
//!                 workspaceId, workspaceName) -> number (persistent id)
//!   start(id) / shutdown(id, immediate) / input(id, data) /
//!   processBinary(id, data) / sendSignal(id, signal) / resize(id, c, r,
//!   pw?, ph?) / clearBuffer(id) / acknowledgeDataEvent(id, n) /
//!   getInitialCwd(id) / getCwd(id) / attachToProcess(id) /
//!   detachFromProcess(id, forcePersist?) / shutdownAll() /
//!   listProcesses() / getPerformanceMarks() / getLatency() /
//!   getDefaultSystemShell(os?) / getEnvironment() / getWslPath(p, dir) /
//!   getProfiles(workspaceId, profiles, defaultProfile, detected) /
//!   getRevivedPtyNewId / setTerminalLayoutInfo / getTerminalLayoutInfo /
//!   reduceConnectionGraceTime / requestDetachInstance /
//!   acceptDetachInstanceReply / freePortKillProcess /
//!   serializeTerminalState / reviveTerminalProcesses / refreshProperty /
//!   updateProperty / updateTitle / updateIcon / setUnicodeVersion /
//!   setNextCommandId / orphanQuestionReply / installAutoReply /
//!   uninstallAllAutoReplies / refreshIgnoreProcessNames
//!
//! Events (ProxyChannel: fired by property name, payload = { id, event }):
//!   onProcessData  { id, event: { data, trackCommit: false } }
//!   onProcessReady { id, event: { pid, cwd, windowsPty } }
//!   onProcessExit  { id, event: exitCode | undefined }
//!   onDidChangeProperty / onProcessReplay / onProcessOrphanQuestion /
//!   onDidRequestDetach — reserved, fired when the features land.
//!
//! Not yet implemented (tracked in ROADMAP.md Phase 5):
//!   - persistent terminal state across app restarts
//!     (serializeTerminalState/reviveTerminalProcesses are in-memory stubs)
//!   - shell-integration script injection (injectedArgs: [])
//!   - dynamic cwd tracking via OSC 633/9;9 (xterm.js title/OSC parsing
//!     already runs renderer-side; cwd refresh stays initial).

use portable_pty::{Child, ChildKiller, CommandBuilder, MasterPty, PtySize};
use serde_json::{json, Map, Value};
use std::collections::HashMap;
use std::io::{Read, Write};
use std::path::PathBuf;
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::{LazyLock, Mutex};

const CHANNEL: &str = "localPty";

// TitleEventSource (src/vs/platform/terminal/common/terminal.ts)
const TITLE_SOURCE_PROCESS: i64 = 1;

struct PtyProcess {
    master: Box<dyn MasterPty + Send>,
    writer: Mutex<Option<Box<dyn Write + Send>>>,
    /// The child stays in the process record; the reader thread claims it
    /// on pty EOF to `wait()` for the exit code.
    child: Mutex<Option<Box<dyn Child + Send + Sync>>>,
    /// Split killer so `shutdown` can kill without racing the reader
    /// thread's `wait` (portable-pty clone_killer pattern).
    killer: Mutex<Box<dyn ChildKiller + Send + Sync>>,
    pid: Option<u32>,
    cwd: String,
    initial_cwd: String,
    title: String,
    title_source: i64,
    icon: Value,
    color: Value,
    workspace_id: String,
    workspace_name: String,
    should_persist: bool,
    is_orphan: bool,
    attached: bool,
    has_child_processes: bool,
}

impl PtyProcess {
    fn process_details(&self, id: i64) -> Value {
        json!({
            "id": id,
            "pid": self.pid.unwrap_or(0),
            "title": self.title,
            "titleSource": self.title_source,
            "cwd": self.cwd,
            "initialCwd": self.initial_cwd,
            "workspaceId": self.workspace_id,
            "workspaceName": self.workspace_name,
            "isOrphan": self.is_orphan,
            "icon": self.icon,
            "color": self.color,
            "fixedDimensions": Value::Null,
            "environmentVariableCollections": Value::Null,
            "hasChildProcesses": self.has_child_processes,
            "shellIntegrationNonce": "",
        })
    }
}

static NEXT_PTY_ID: AtomicI64 = AtomicI64::new(1);

static PTY_PROCS: LazyLock<Mutex<HashMap<i64, PtyProcess>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

/// Terminal layout info per workspace (ITerminalsLayoutInfo), stored when
/// the renderer calls setTerminalLayoutInfo. In-memory only: survives
/// window reloads (the shell process keeps running), not app restarts —
/// that requires the persistent-state work in Phase 5.
static LAYOUT_INFO: LazyLock<Mutex<HashMap<String, Value>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

/// Handle one `localPty` channel request.
pub fn handle(command: &str, arg: &Value) -> Result<Value, String> {
    let args = arg.as_array().cloned().unwrap_or_default();
    let arg0 = args.first().cloned().unwrap_or(Value::Null);

    match command {
        // ---- process lifecycle ----
        "createProcess" => create_process(&args),
        "start" => {
            // ITerminalLaunchResult: the injected args we (don't) add.
            // Shell-integration injection is later Phase 5 work.
            Ok(json!({ "injectedArgs": [] }))
        }
        "shutdown" => {
            let id = arg0.as_i64().unwrap_or(-1);
            let immediate = args.get(1).and_then(Value::as_bool).unwrap_or(false);
            shutdown(id, immediate);
            Ok(Value::Null)
        }
        "shutdownAll" => {
            let ids: Vec<i64> = PTY_PROCS
                .lock()
                .map(|procs| procs.keys().copied().collect())
                .unwrap_or_default();
            for id in ids {
                shutdown(id, true);
            }
            Ok(Value::Null)
        }
        "attachToProcess" => {
            let id = arg0.as_i64().unwrap_or(-1);
            if let Ok(mut procs) = PTY_PROCS.lock() {
                if let Some(proc) = procs.get_mut(&id) {
                    proc.attached = true;
                    proc.is_orphan = false;
                }
            }
            Ok(Value::Null)
        }
        "detachFromProcess" => {
            let id = arg0.as_i64().unwrap_or(-1);
            let force_persist = args.get(1).and_then(Value::as_bool).unwrap_or(true);
            if let Ok(mut procs) = PTY_PROCS.lock() {
                if let Some(proc) = procs.get_mut(&id) {
                    proc.attached = false;
                    proc.is_orphan = !force_persist && proc.should_persist;
                }
            }
            Ok(Value::Null)
        }
        "listProcesses" => {
            let details: Vec<Value> = PTY_PROCS
                .lock()
                .map(|procs| {
                    procs
                        .iter()
                        .filter(|(_, proc)| proc.should_persist || proc.attached)
                        .map(|(id, proc)| proc.process_details(*id))
                        .collect()
                })
                .unwrap_or_default();
            Ok(Value::Array(details))
        }
        "getPerformanceMarks" => Ok(json!([])),

        // ---- io ----
        "input" | "processBinary" => {
            let id = arg0.as_i64().unwrap_or(-1);
            let data = args.get(1).and_then(Value::as_str).unwrap_or("");
            write_input(id, data.as_bytes())
        }
        "sendSignal" => {
            // node-pty on Windows does not support POSIX signals; map the
            // one signal that has a terminal-level equivalent.
            let id = arg0.as_i64().unwrap_or(-1);
            let signal = args.get(1).and_then(Value::as_str).unwrap_or("");
            if signal == "SIGINT" {
                write_input(id, b"\x03")?;
            }
            Ok(Value::Null)
        }
        "resize" => {
            let id = arg0.as_i64().unwrap_or(-1);
            let cols = args.get(1).and_then(Value::as_i64).unwrap_or(80).max(1) as u16;
            let rows = args.get(2).and_then(Value::as_i64).unwrap_or(24).max(1) as u16;
            let pixel_width = args.get(3).and_then(Value::as_i64).unwrap_or(0).max(0) as u16;
            let pixel_height = args.get(4).and_then(Value::as_i64).unwrap_or(0).max(0) as u16;
            if let Ok(procs) = PTY_PROCS.lock() {
                if let Some(proc) = procs.get(&id) {
                    proc.master
                        .resize(PtySize { rows, cols, pixel_width, pixel_height })
                        .map_err(|err| err.to_string())?;
                } else {
                    return Err(format!("Persistent process {} does not exist", id));
                }
            }
            Ok(Value::Null)
        }
        "clearBuffer" => {
            // ANSI "erase entire screen + scrollback, home cursor" — what
            // the upstream ConPTY reset resolves to for xterm.
            let id = arg0.as_i64().unwrap_or(-1);
            let _ = write_input(id, b"\x1b[3J\x1b[H\x1b[2J");
            Ok(Value::Null)
        }
        "acknowledgeDataEvent" => {
            // Renderer-side flow control ack. The native reader drains the
            // pty continuously so there is nothing to pause.
            Ok(Value::Null)
        }
        "setUnicodeVersion" | "setNextCommandId" | "orphanQuestionReply" => Ok(Value::Null),

        // ---- metadata ----
        "getInitialCwd" | "getCwd" => {
            let id = arg0.as_i64().unwrap_or(-1);
            if let Ok(procs) = PTY_PROCS.lock() {
                if let Some(proc) = procs.get(&id) {
                    return Ok(json!(if command == "getCwd" { proc.cwd.clone() } else { proc.initial_cwd.clone() }));
                }
            }
            Err(format!("Persistent process {} does not exist", id))
        }
        "updateTitle" => {
            let id = arg0.as_i64().unwrap_or(-1);
            let title = args.get(1).and_then(Value::as_str).unwrap_or("").to_string();
            let title_source = args.get(2).and_then(Value::as_i64).unwrap_or(TITLE_SOURCE_PROCESS);
            if let Ok(mut procs) = PTY_PROCS.lock() {
                if let Some(proc) = procs.get_mut(&id) {
                    proc.title = title;
                    proc.title_source = title_source;
                }
            }
            Ok(Value::Null)
        }
        "updateIcon" => {
            let id = arg0.as_i64().unwrap_or(-1);
            let icon = args.get(2).cloned().unwrap_or(Value::Null);
            let color = args.get(3).cloned().unwrap_or(Value::Null);
            if let Ok(mut procs) = PTY_PROCS.lock() {
                if let Some(proc) = procs.get_mut(&id) {
                    proc.icon = icon;
                    proc.color = color;
                }
            }
            Ok(Value::Null)
        }
        "refreshProperty" => {
            let id = arg0.as_i64().unwrap_or(-1);
            let property = args.get(1).and_then(Value::as_str).unwrap_or("");
            if let Ok(procs) = PTY_PROCS.lock() {
                if let Some(proc) = procs.get(&id) {
                    return Ok(match property {
                        "cwd" => json!(proc.cwd),
                        "initialCwd" => json!(proc.initial_cwd),
                        "title" => json!(proc.title),
                        "hasChildProcesses" => json!(proc.has_child_processes),
                        _ => Value::Null,
                    });
                }
            }
            Err(format!("Persistent process {} does not exist", id))
        }
        "updateProperty" => {
            let id = arg0.as_i64().unwrap_or(-1);
            let property = args.get(1).and_then(Value::as_str).unwrap_or("");
            let value = args.get(2).cloned().unwrap_or(Value::Null);
            if let Ok(mut procs) = PTY_PROCS.lock() {
                if let Some(proc) = procs.get_mut(&id) {
                    match property {
                        "cwd" => {
                            if let Some(cwd) = value.as_str() {
                                proc.cwd = cwd.to_string();
                            }
                        }
                        "title" => {
                            if let Some(title) = value.as_str() {
                                proc.title = title.to_string();
                            }
                        }
                        "hasChildProcesses" => {
                            proc.has_child_processes = value.as_bool().unwrap_or(false);
                        }
                        _ => {}
                    }
                }
            }
            Ok(Value::Null)
        }

        // ---- environment / shell discovery ----
        "getDefaultSystemShell" => {
            let os_override = arg0.as_i64().unwrap_or(0);
            let is_windows_request = if os_override == 0 {
                cfg!(windows)
            } else {
                os_override == 1 // OperatingSystem.Windows
            };
            if is_windows_request {
                Ok(json!(std::env::var("COMSPEC").unwrap_or_else(|_| "cmd.exe".to_string())))
            } else if os_override == 2 {
                Ok(json!("/bin/zsh"))
            } else {
                Ok(json!(std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".to_string())))
            }
        }
        "getEnvironment" => Ok(crate::config::user_env()),
        "getWslPath" => get_wsl_path(&args),
        "getProfiles" => get_profiles(&args),
        "getLatency" => Ok(json!([])),
        "getRevivedPtyNewId" => Ok(Value::Null),
        "freePortKillProcess" => free_port_kill_process(&arg0),

        // ---- terminal layout / persistence ----
        "setTerminalLayoutInfo" => {
            // arg0 is ISetTerminalLayoutInfoArgs itself.
            let workspace_id = arg0
                .get("workspaceId")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string();
            if let Ok(mut layouts) = LAYOUT_INFO.lock() {
                layouts.insert(workspace_id, arg0);
            }
            Ok(Value::Null)
        }
        "getTerminalLayoutInfo" => {
            let workspace_id = arg0.get("workspaceId").and_then(Value::as_str).unwrap_or("").to_string();
            if let Ok(layouts) = LAYOUT_INFO.lock() {
                if let Some(layout) = layouts.get(&workspace_id) {
                    return Ok(layout.get("tabs").cloned().unwrap_or(Value::Null));
                }
            }
            Ok(Value::Null)
        }
        "serializeTerminalState" => Ok(json!("[]")),
        "reviveTerminalProcesses" => Ok(Value::Null),
        "reduceConnectionGraceTime" => Ok(Value::Null),
        "requestDetachInstance" => Ok(Value::Null),
        "acceptDetachInstanceReply" => Ok(Value::Null),

        // ---- auto reply (Windows feature) / contribution surface ----
        "installAutoReply" | "uninstallAllAutoReplies" | "refreshIgnoreProcessNames" => {
            Ok(Value::Null)
        }

        other => Err(format!("localPty channel: call not found: {}", other)),
    }
}

// ---------------------------------------------------------------------------
// Process creation and lifecycle
// ---------------------------------------------------------------------------

fn create_process(args: &[Value]) -> Result<Value, String> {
    // createProcess(shellLaunchConfig, cwd, cols, rows, unicodeVersion,
    //               env, executableEnv, options, shouldPersist,
    //               workspaceId, workspaceName) -> persistent id
    let slc = args.first().cloned().unwrap_or(Value::Null);
    let cwd_arg = args.get(1).and_then(Value::as_str).unwrap_or("");
    let cols = args.get(2).and_then(Value::as_i64).unwrap_or(80).clamp(2, 500) as u16;
    let rows = args.get(3).and_then(Value::as_i64).unwrap_or(24).clamp(2, 500) as u16;
    let env_arg = args.get(5).cloned().unwrap_or(Value::Null);
    let should_persist = args.get(8).and_then(Value::as_bool).unwrap_or(false);
    let workspace_id = args.get(9).and_then(Value::as_str).unwrap_or("").to_string();
    let workspace_name = args.get(10).and_then(Value::as_str).unwrap_or("").to_string();

    // Resolve the executable: explicit profile path, or the OS default
    // (COMSPEC / $SHELL) — mirrors TerminalProcess's fallback.
    let executable = slc
        .get("executable")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .unwrap_or_else(default_shell);

    let mut cmd = if executable.is_empty() {
        CommandBuilder::new_default_prog()
    } else {
        CommandBuilder::new(&executable)
    };

    // shellLaunchConfig.args: string[] | string
    match slc.get("args") {
        Some(Value::Array(items)) => {
            for item in items {
                if let Some(arg) = item.as_str() {
                    cmd.arg(arg);
                }
            }
        }
        Some(Value::String(single)) if !single.is_empty() => {
            cmd.arg(single);
        }
        _ => {}
    }

    // cwd precedence: shellLaunchConfig.cwd (string | UriComponents) over
    // the resolved cwd argument over the user home directory.
    let slc_cwd = launch_cwd(&slc);
    let cwd = slc_cwd
        .or_else(|| {
            if cwd_arg.is_empty() {
                None
            } else {
                Some(PathBuf::from(cwd_arg))
            }
        })
        .unwrap_or_else(user_home);
    let cwd = if cwd.exists() { cwd } else { user_home() };
    cmd.cwd(&cwd);

    // Environment: inherited process env, overlaid with the renderer's
    // resolved env (arg 5), then the launch-config env (most specific).
    if let Some(env_map) = env_arg.as_object() {
        for (key, value) in env_map {
            if let Some(value_str) = value.as_str() {
                cmd.env(key, value_str);
            }
        }
    }
    if let Some(env_map) = slc.get("env").and_then(Value::as_object) {
        for (key, value) in env_map {
            if let Some(value_str) = value.as_str() {
                cmd.env(key, value_str);
            }
        }
    }

    // Spawn through the native pty (ConPTY / openpty).
    let pty_system = portable_pty::native_pty_system();
    let pair = pty_system
        .openpty(PtySize { rows, cols, pixel_width: 0, pixel_height: 0 })
        .map_err(|err| format!("localPty: openpty failed: {}", err))?;
    let portable_pty::PtyPair { slave, master } = pair;
    let child = slave
        .spawn_command(cmd)
        .map_err(|err| format!("localPty: spawn {:?} failed: {}", executable, err))?;
    drop(slave); // close the slave handle in the parent, like node-pty

    let reader = master
        .try_clone_reader()
        .map_err(|err| format!("localPty: clone reader failed: {}", err))?;
    let writer = master
        .take_writer()
        .map_err(|err| format!("localPty: take writer failed: {}", err))?;
    let pid = child.process_id();
    let killer = Mutex::new(child.clone_killer());
    let cwd_str = cwd.to_string_lossy().to_string();
    let title = slc
        .get("name")
        .and_then(Value::as_str)
        .unwrap_or(executable.as_str())
        .to_string();

    let id = NEXT_PTY_ID.fetch_add(1, Ordering::Relaxed);
    let initial_cwd = cwd_str.clone();
    if let Ok(mut procs) = PTY_PROCS.lock() {
        procs.insert(
            id,
            PtyProcess {
                master,
                writer: Mutex::new(Some(writer)),
                child: Mutex::new(Some(child)),
                killer,
                pid,
                cwd: cwd_str.clone(),
                initial_cwd,
                title,
                title_source: TITLE_SOURCE_PROCESS,
                icon: slc.get("icon").cloned().unwrap_or(Value::Null),
                color: slc.get("color").cloned().unwrap_or(Value::Null),
                workspace_id,
                workspace_name,
                should_persist,
                is_orphan: false,
                attached: true,
                has_child_processes: false,
            },
        );
    }

    crate::logger::log_app(
        "info",
        &format!(
            "localPty: created process {} ({} {:?}, pid {:?}, {}x{})",
            id,
            executable,
            args.get(1).and_then(Value::as_str).unwrap_or(""),
            pid,
            cols,
            rows
        ),
    );

    // Reader thread: pty output -> onProcessData until EOF, then collect
    // the exit status and fire onProcessExit.
    std::thread::Builder::new()
        .name(format!("vstauri-pty-read-{}", id))
        .spawn(move || reader_loop(id, reader))
        .map_err(|err| err.to_string())?;

    // ChannelServer parity: onProcessReady fires right after spawn with the
    // pid, cwd and the windows pty backend info.
    crate::ipc::fire_event(
        CHANNEL,
        "onProcessReady",
        &json!({
            "id": id,
            "event": {
                "pid": pid.unwrap_or(0),
                "cwd": cwd_str,
                "windowsPty": windows_pty_json(),
            }
        }),
    );

    Ok(json!(id))
}

/// Read pty output until EOF, delivering UTF-8-safe data events, then
/// reap the child and report the exit code.
fn reader_loop(id: i64, mut reader: Box<dyn Read + Send>) {
    let mut decoder = Utf8Decoder::default();
    let mut buffer = [0u8; 8192];
    loop {
        match reader.read(&mut buffer) {
            Ok(0) => break, // pty closed => child exited
            Ok(n) => {
                let text = decoder.push(&buffer[..n]);
                if !text.is_empty() {
                    crate::ipc::fire_event(
                        CHANNEL,
                        "onProcessData",
                        &json!({
                            "id": id,
                            "event": { "data": text, "trackCommit": false }
                        }),
                    );
                }
            }
            Err(err) => {
                if err.kind() != std::io::ErrorKind::Interrupted {
                    break;
                }
            }
        }
    }
    on_pty_eof(id);
}

/// The pty hit EOF: wait for the child, fire onProcessExit, clean up.
fn on_pty_eof(id: i64) {
    let exit_code = if let Ok(mut procs) = PTY_PROCS.lock() {
        match procs.get_mut(&id) {
            Some(proc) => match proc.child.lock().ok().and_then(|mut guard| guard.take()) {
                Some(mut child) => child
                    .wait()
                    .map(|status| status.exit_code() as i64)
                    .unwrap_or(0),
                None => 0, // already reaped (shutdown raced us)
            },
            None => return,
        }
    } else {
        return;
    };
    crate::ipc::fire_event(CHANNEL, "onProcessExit", &json!({ "id": id, "event": exit_code }));
    if let Ok(mut procs) = PTY_PROCS.lock() {
        procs.remove(&id);
    }
    crate::logger::log_app("info", &format!("localPty: process {} exited with code {}", id, exit_code));
}

fn shutdown(id: i64, immediate: bool) {
    let take_action = if let Ok(procs) = PTY_PROCS.lock() {
        match procs.get(&id) {
            Some(proc) => immediate || !proc.should_persist,
            None => false,
        }
    } else {
        false
    };
    if !take_action {
        // Persistent terminal + graceful shutdown: keep running for a
        // future reconnect (upstream detach semantics).
        if let Ok(mut procs) = PTY_PROCS.lock() {
            if let Some(proc) = procs.get_mut(&id) {
                proc.attached = false;
            }
        }
        return;
    }
    if let Ok(procs) = PTY_PROCS.lock() {
        if let Some(proc) = procs.get(&id) {
            if let Ok(mut killer) = proc.killer.lock() {
                let _ = killer.kill();
            }
        }
    }
    // The reader thread observes the pty EOF, reaps the child and fires
    // onProcessExit.
}

fn write_input(id: i64, bytes: &[u8]) -> Result<Value, String> {
    if let Ok(mut procs) = PTY_PROCS.lock() {
        if let Some(proc) = procs.get_mut(&id) {
            if let Ok(mut guard) = proc.writer.lock() {
                if let Some(writer) = guard.as_mut() {
                    writer
                        .write_all(bytes)
                        .map_err(|err| format!("localPty: write to {} failed: {}", id, err))?;
                }
            }
            return Ok(Value::Null);
        }
    }
    Err(format!("Persistent process {} does not exist", id))
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// `IShellLaunchConfig.cwd` is `string | UriComponents`.
fn launch_cwd(slc: &Value) -> Option<PathBuf> {
    match slc.get("cwd") {
        Some(Value::String(path)) if !path.is_empty() => Some(PathBuf::from(path)),
        Some(uri @ Value::Object(_)) => {
            if uri.get("scheme").and_then(Value::as_str) == Some("file") {
                let raw = uri.get("path").and_then(Value::as_str).unwrap_or("");
                let decoded = crate::util::percent_decode(raw);
                let normalized = decoded.replace('\\', "/");
                let trimmed = normalized.trim_start_matches('/');
                if cfg!(windows) {
                    Some(PathBuf::from(trimmed.replace('/', "\\")))
                } else {
                    Some(PathBuf::from(format!("/{}", trimmed)))
                }
            } else {
                None
            }
        }
        _ => None,
    }
}

fn default_shell() -> String {
    if cfg!(windows) {
        std::env::var("COMSPEC").unwrap_or_else(|_| "cmd.exe".to_string())
    } else {
        std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".to_string())
    }
}

fn user_home() -> PathBuf {
    if cfg!(windows) {
        std::env::var("USERPROFILE")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from("C:\\"))
    } else {
        std::env::var("HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from("/"))
    }
}

/// IProcessReadyWindowsPty: ConPTY backend + the real Windows build number
/// (xterm.js uses the build for ConPTY reflow quirks). Null on other
/// platforms, matching upstream.
fn windows_pty_json() -> Value {
    if cfg!(windows) {
        json!({ "backend": "conpty", "buildNumber": windows_build_number() })
    } else {
        Value::Null
    }
}

#[cfg(windows)]
fn windows_build_number() -> i64 {
    #[repr(C)]
    #[allow(non_snake_case)]
    struct OSVERSIONINFOW {
        dwOSVersionInfoSize: u32,
        dwMajorVersion: u32,
        dwMinorVersion: u32,
        dwBuildNumber: u32,
        dwPlatformId: u32,
        szCSDVersion: [u16; 128],
    }
    #[link(name = "ntdll")]
    extern "system" {
        fn RtlGetVersion(lpVersionInformation: *mut OSVERSIONINFOW) -> i32;
    }
    unsafe {
        let mut info = OSVERSIONINFOW {
            dwOSVersionInfoSize: std::mem::size_of::<OSVERSIONINFOW>() as u32,
            dwMajorVersion: 0,
            dwMinorVersion: 0,
            dwBuildNumber: 0,
            dwPlatformId: 0,
            szCSDVersion: [0; 128],
        };
        if RtlGetVersion(&mut info) == 0 {
            info.dwBuildNumber as i64
        } else {
            19041 // Windows 10 2004 baseline fallback
        }
    }
}

#[cfg(not(windows))]
fn windows_build_number() -> i64 {
    0
}

/// Incremental UTF-8 decoder: pty reads may split multi-byte sequences at
/// 8 KB chunk boundaries; emitting them lossily would corrupt CJK output
/// in xterm.js. Incomplete trailing sequences are held back until the next
/// chunk; genuinely invalid bytes become U+FFFD (Node's string_decoder
/// behavior in node-pty's read path).
#[derive(Default)]
struct Utf8Decoder {
    pending: Vec<u8>,
}

impl Utf8Decoder {
    fn push(&mut self, chunk: &[u8]) -> String {
        self.pending.extend_from_slice(chunk);
        let mut out = String::new();
        loop {
            match std::str::from_utf8(&self.pending) {
                Ok(text) => {
                    out.push_str(text);
                    self.pending.clear();
                    return out;
                }
                Err(err) => {
                    let valid = err.valid_up_to();
                    match err.error_len() {
                        Some(invalid) => {
                            out.push_str(&String::from_utf8_lossy(&self.pending[..valid]));
                            out.push('\u{FFFD}');
                            self.pending.drain(..valid + invalid);
                        }
                        None => {
                            // Truncated multi-byte sequence at the buffer
                            // end: emit what is complete, keep the tail.
                            out.push_str(&String::from_utf8_lossy(&self.pending[..valid]));
                            self.pending.drain(..valid);
                            return out;
                        }
                    }
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Shell discovery (profiles / WSL / free-port)
// ---------------------------------------------------------------------------

/// getProfiles(workspaceId, profiles, defaultProfile, includeDetectedProfiles)
/// -> ITerminalProfile[]. Mirrors terminalProfiles.ts: config profiles are
/// passed through (the renderer resolves most variables), detected profiles
/// are appended for the shells that actually exist on this machine.
fn get_profiles(args: &[Value]) -> Result<Value, String> {
    let profiles_arg = args.get(1).cloned().unwrap_or(Value::Null);
    let default_profile = args.get(2).and_then(Value::as_str).unwrap_or("");
    let include_detected = args.get(3).and_then(Value::as_bool).unwrap_or(true);

    let mut out: Vec<Value> = Vec::new();
    let mut names: Vec<String> = Vec::new();

    // 1. Config-defined profiles (object map: name -> { path, args, ... }).
    if let Some(config) = profiles_arg.as_object() {
        for (name, spec) in config {
            names.push(name.clone());
            let mut profile = match spec {
                Value::Object(fields) => {
                    let mut map = Map::new();
                    for (key, value) in fields {
                        map.insert(key.clone(), value.clone());
                    }
                    map
                }
                _ => Map::new(),
            };
            profile.insert("profileName".to_string(), json!(name));
            profile.insert(
                "isDefault".to_string(),
                json!(!name.is_empty() && *name == default_profile),
            );
            out.push(Value::Object(profile));
        }
    }

    // 2. Auto-detected profiles (existence-checked).
    if include_detected {
        for mut profile in detect_platform_profiles() {
            let name = profile
                .get("profileName")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string();
            if !names.contains(&name) {
                if !name.is_empty() && name == default_profile {
                    if let Some(obj) = profile.as_object_mut() {
                        obj.insert("isDefault".to_string(), json!(true));
                    }
                }
                names.push(name);
                out.push(profile);
            }
        }
    }

    Ok(Value::Array(out))
}

/// Windows detection set (terminalProfiles.ts detectAvailableWindowsProfiles):
/// PowerShell (pwsh), Windows PowerShell, Command Prompt, Git Bash, WSL.
/// On Unix: the login shell plus common fallbacks that exist.
fn detect_platform_profiles() -> Vec<Value> {
    let mut profiles: Vec<Value> = Vec::new();
    if cfg!(windows) {
        let windir = std::env::var("windir").unwrap_or_else(|_| "C:\\Windows".to_string());
        let system32 = format!("{}\\System32", windir);
        let program_files =
            std::env::var("ProgramFiles").unwrap_or_else(|_| "C:\\Program Files".to_string());

        // Windows PowerShell ships with the OS.
        profiles.push(json!({
            "profileName": "Windows PowerShell",
            "path": format!("{}\\WindowsPowerShell\\v1.0\\powershell.exe", system32),
            "isAutoDetected": true,
            "icon": { "id": "terminal-powershell" },
        }));
        profiles.push(json!({
            "profileName": "Command Prompt",
            "path": format!("{}\\cmd.exe", system32),
            "isAutoDetected": true,
            "icon": { "id": "terminal-cmd" },
        }));
        // PowerShell 7+ (pwsh) from the well-known install locations.
        let pwsh_candidates = [
            format!("{}\\PowerShell\\7\\pwsh.exe", program_files),
            format!("{}\\PowerShell\\7-preview\\pwsh.exe", program_files),
            format!("{}\\PowerShell\\7\\pwsh.exe", system32.replace("\\System32", "")),
        ];
        if let Some(path) = pwsh_candidates.iter().find(|path| PathBuf::from(path).exists()) {
            profiles.push(json!({
                "profileName": "PowerShell",
                "path": path,
                "isAutoDetected": true,
                "icon": { "id": "terminal-powershell" },
            }));
        }
        // Git Bash.
        let git_bash = [
            format!("{}\\Git\\bin\\bash.exe", program_files),
            format!("{}\\Git\\usr\\bin\\bash.exe", program_files),
        ];
        if let Some(path) = git_bash.iter().find(|path| PathBuf::from(path).exists()) {
            profiles.push(json!({
                "profileName": "Git Bash",
                "path": path,
                "args": ["--login"],
                "isAutoDetected": true,
                "icon": { "id": "terminal-git-bash" },
            }));
        }
        // WSL.
        let wsl = format!("{}\\wsl.exe", system32);
        if PathBuf::from(&wsl).exists() {
            profiles.push(json!({
                "profileName": "WSL",
                "path": wsl,
                "args": ["-e", "/bin/bash"],
                "isAutoDetected": true,
                "icon": { "id": "terminal-linux" },
            }));
        }
    } else {
        let home_shell = std::env::var("SHELL").unwrap_or_default();
        if !home_shell.is_empty() && PathBuf::from(&home_shell).exists() {
            let name = home_shell.rsplit('/').next().unwrap_or("sh").to_string();
            profiles.push(json!({
                "profileName": name,
                "path": home_shell,
                "isAutoDetected": true,
            }));
        }
        for shell in ["/bin/bash", "/bin/zsh", "/bin/sh"] {
            if PathBuf::from(shell).exists() {
                let name = shell.rsplit('/').next().unwrap_or("sh").to_string();
                if !profiles.iter().any(|p| {
                    p.get("profileName").and_then(Value::as_str) == Some(name.as_str())
                }) {
                    profiles.push(json!({
                        "profileName": name,
                        "path": shell,
                        "isAutoDetected": true,
                    }));
                }
            }
        }
    }
    profiles
}

/// getWslPath(original, direction) — runs wslpath inside WSL (the same
/// mechanism the pty host uses). Windows-only; other platforms pass the
/// path through unchanged (the renderer only calls this for WSL).
fn get_wsl_path(args: &[Value]) -> Result<Value, String> {
    let original = args.first().and_then(Value::as_str).unwrap_or("");
    let direction = args.get(1).and_then(Value::as_str).unwrap_or("");
    if !cfg!(windows) {
        return Ok(json!(original));
    }
    let flag = match direction {
        "win-to-unix" => "-u",
        "unix-to-win" => "-w",
        other => {
            return Err(format!("localPty: getWslPath unknown direction {}", other));
        }
    };
    let output = std::process::Command::new("wsl.exe")
        .arg("-e")
        .arg("wslpath")
        .arg(flag)
        .arg(original)
        .output()
        .map_err(|err| format!("localPty: wsl.exe failed: {}", err))?;
    if !output.status.success() {
        return Err(format!(
            "localPty: wslpath failed: {}",
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    let converted = String::from_utf8_lossy(&output.stdout).trim().to_string();
    Ok(json!(converted))
}

/// freePortKillProcess(port) — find the LISTENING owner via netstat and
/// taskkill it (ptyService.freePortKillProcess parity).
fn free_port_kill_process(port: &Value) -> Result<Value, String> {
    let port = port.as_str().unwrap_or_default().to_string();
    if port.is_empty() {
        return Err("localPty: freePortKillProcess expects a port".to_string());
    }
    let netstat = if cfg!(windows) {
        std::process::Command::new("netstat").arg("-ano").output()
    } else {
        std::process::Command::new("sh")
            .arg("-c")
            .arg("ss -ltnp 2>/dev/null || netstat -ltnp 2>/dev/null")
            .output()
    }
    .map_err(|err| format!("localPty: netstat failed: {}", err))?;
    let text = String::from_utf8_lossy(&netstat.stdout);

    let needle = format!(":{}", port);
    let mut pid: Option<String> = None;
    for line in text.lines() {
        let lower = line.to_lowercase();
        if (lower.contains("listen") || lower.contains("users:")) && line.contains(&needle) {
            // last whitespace-separated token is the PID (netstat -ano) or
            // the pid= field (ss -ltnp).
            if cfg!(windows) {
                if let Some(last) = line.split_whitespace().last() {
                    if last.chars().all(|c| c.is_ascii_digit()) {
                        pid = Some(last.to_string());
                        break;
                    }
                }
            } else if let Some(idx) = line.find("pid=") {
                let tail = &line[idx + 4..];
                let end = tail
                    .find(|c: char| !c.is_ascii_digit())
                    .unwrap_or(tail.len());
                pid = Some(tail[..end].to_string());
                break;
            }
        }
    }
    let Some(pid) = pid else {
        return Err(format!("localPty: no process found listening on port {}", port));
    };

    if cfg!(windows) {
        std::process::Command::new("taskkill")
            .arg("/PID")
            .arg(&pid)
            .arg("/T")
            .arg("/F")
            .output()
            .map_err(|err| format!("localPty: taskkill failed: {}", err))?;
    } else {
        std::process::Command::new("kill")
            .arg(&pid)
            .output()
            .map_err(|err| format!("localPty: kill failed: {}", err))?;
    }
    Ok(json!({ "port": port, "processId": pid }))
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn utf8_decoder_handles_split_multi_byte_sequences() {
        let mut decoder = Utf8Decoder::default();
        // "héllo" — é is U+00E9 (2 bytes), split across chunks.
        let first = decoder.push(b"h\xc3");
        assert_eq!(first, "h");
        let second = decoder.push(b"\xa9llo");
        assert_eq!(second, "\u{e9}llo");

        // A 4-byte emoji split 3 ways.
        let mut decoder = Utf8Decoder::default();
        let bytes = "😀".as_bytes(); // F0 9F 98 80
        assert_eq!(decoder.push(&bytes[..1]), "");
        assert_eq!(decoder.push(&bytes[1..2]), "");
        assert_eq!(decoder.push(&bytes[2..]), "😀");

        // Invalid bytes become U+FFFD instead of poisoning the stream.
        let mut decoder = Utf8Decoder::default();
        let text = decoder.push(b"ok\xffstill");
        assert!(text.starts_with("ok"));
        assert!(text.contains('\u{FFFD}'));
        assert!(text.ends_with("still"));
    }

    #[test]
    fn utf8_decoder_is_correct_on_full_sequences() {
        let mut decoder = Utf8Decoder::default();
        let text = "终端 output — mixed 中文 and ASCII";
        let out = decoder.push(text.as_bytes());
        assert_eq!(out, text);
    }

    #[test]
    fn launch_cwd_accepts_string_and_file_uri() {
        let slc = json!({ "cwd": "C:\\dev\\project" });
        assert_eq!(
            launch_cwd(&slc).map(|p| p.to_string_lossy().to_string()),
            Some("C:\\dev\\project".to_string())
        );
        let slc = json!({ "cwd": { "scheme": "file", "path": "/C:/dev/project" } });
        let cwd = launch_cwd(&slc).expect("uri cwd");
        let text = cwd.to_string_lossy().replace('/', "\\");
        assert!(text.contains("dev"), "got {:?}", text);
        assert!(launch_cwd(&json!({})).is_none());
        // Non-file schemes (vscode-remote) are ignored upstream too.
        assert!(launch_cwd(&json!({ "cwd": { "scheme": "vscode-remote", "path": "/x" } })).is_none());
    }

    #[test]
    fn unknown_commands_reject_like_upstream() {
        let err = handle("notACommand", &json!([])).expect_err("must reject");
        assert!(err.contains("notACommand"));
    }

    #[cfg(unix)]
    #[test]
    fn terminal_round_trip_echoes_data_and_exits() {
        // No clear_test_listeners(): tests run in parallel and share the
        // listener registry; unique listener ids + frame filtering keep
        // this test isolated.
        // Listen like the renderer's ProxyChannel does (no listen arg).
        crate::ipc::register_test_listener(31, "localPty", "onProcessData", Value::Null);
        crate::ipc::register_test_listener(32, "localPty", "onProcessReady", Value::Null);
        crate::ipc::register_test_listener(33, "localPty", "onProcessExit", Value::Null);

        // A tiny POSIX shell script: prints a marker, echoes stdin back,
        // exits with code 7.
        let marker = format!("vstauri-pty-test-{}", std::process::id());
        let script_dir = std::env::temp_dir().join(&marker);
        std::fs::create_dir_all(&script_dir).unwrap();
        let script = script_dir.join("echo.sh");
        std::fs::write(
            &script,
            "#!/bin/sh\nprintf 'MARKER1\\n'\nread line\nprintf \"ECHO:%s\\n\" \"$line\"\nexit 7\n",
        )
        .unwrap();

        let create_args: Vec<Value> = vec![
            json!({ "executable": "/bin/sh", "args": [script.to_string_lossy()] }),
            json!(script_dir.to_string_lossy()),
            json!(80), json!(24), json!("6"),
            json!({}), json!({}), json!({}),
            json!(false), json!("ws"), json!("Test Workspace"),
        ];
        let id = create_process(&create_args)
            .expect("createProcess")
            .as_i64()
            .unwrap();

        // start returns the (empty) injected args.
        let launch = handle("start", &json!([id])).expect("start");
        assert_eq!(launch.get("injectedArgs").and_then(Value::as_array), Some(&Vec::new()));

        // InitialCwd is the cwd we passed.
        let cwd = handle("getInitialCwd", &json!([id])).expect("initialCwd");
        assert!(cwd.as_str().unwrap_or("").contains(&marker));

        // Feed stdin; the shell echoes it back and exits 7.
        std::thread::sleep(std::time::Duration::from_millis(200));
        handle("input", &json!([id, "hello-vstauri\n"])).expect("input");

        // Wait for process exit to propagate.
        for _ in 0..50 {
            if handle("getCwd", &json!([id])).is_err() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(100));
        }

        // Assert the observed event stream. Filter to this process's
        // localPty event payloads — other tests running in parallel push
        // their own shapes into the shared frame buffer.
        let frames = crate::ipc::TEST_FRAMES
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let mut saw_ready = false;
        let mut saw_marker = false;
        let mut saw_echo = false;
        let mut saw_exit = false;
        for (listener, payload) in frames.iter() {
            let id_in_event = payload.get("id").and_then(Value::as_i64);
            if id_in_event != Some(id) {
                continue;
            }
            match *listener {
                32 if id_in_event == Some(id) => saw_ready = true,
                31 if id_in_event == Some(id) => {
                    let data = payload["event"]["data"].as_str().unwrap_or_default();
                    if data.contains("MARKER1") {
                        saw_marker = true;
                    }
                    if data.contains("ECHO:hello-vstauri") {
                        saw_echo = true;
                    }
                }
                33 if id_in_event == Some(id) => {
                    if payload["event"].as_i64() == Some(7) {
                        saw_exit = true;
                    }
                }
                _ => {}
            }
        }
        drop(frames);
        assert!(saw_ready, "onProcessReady missing");
        assert!(saw_marker, "onProcessData MARKER1 missing");
        assert!(saw_echo, "onProcessData ECHO missing");
        assert!(saw_exit, "onProcessExit code 7 missing");

        // After exit the persistent process is gone.
        assert!(handle("input", &json!([id, "late\n"])).is_err());
        let _ = std::fs::remove_dir_all(&script_dir);
    }

    /// Windows twin of the unix round-trip: ConPTY + cmd.exe. Marker
    /// output, exit code, cleanup — stdin echo has no direct cmd.exe
    /// equivalent so input() is exercised by writing before exit.
    #[cfg(windows)]
    #[test]
    fn terminal_round_trip_echoes_data_and_exits() {
        crate::ipc::register_test_listener(31, "localPty", "onProcessData", Value::Null);
        crate::ipc::register_test_listener(32, "localPty", "onProcessReady", Value::Null);
        crate::ipc::register_test_listener(33, "localPty", "onProcessExit", Value::Null);

        let marker = format!("vstauri-pty-test-{}", std::process::id());
        let dir = std::env::temp_dir().join(&marker);
        std::fs::create_dir_all(&dir).unwrap();

        let create_args: Vec<Value> = vec![
            json!({
                "executable": std::env::var("COMSPEC").unwrap_or_else(|_| "cmd.exe".into()),
                "args": ["/c", "echo MARKER1& exit /b 7"]
            }),
            json!(dir.to_string_lossy()),
            json!(80), json!(24), json!("6"),
            json!({}), json!({}), json!({}),
            json!(false), json!("ws"), json!("Test Workspace"),
        ];
        let id = create_process(&create_args)
            .expect("createProcess")
            .as_i64()
            .unwrap();

        let cwd = handle("getInitialCwd", &json!([id])).expect("initialCwd");
        assert!(cwd.as_str().unwrap_or("").contains(&marker));

        // Wait for the process to exit and be cleaned up.
        for _ in 0..50 {
            if handle("getCwd", &json!([id])).is_err() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(100));
        }

        let frames = crate::ipc::TEST_FRAMES
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let mut saw_ready = false;
        let mut saw_marker = false;
        let mut saw_exit = false;
        for (listener, payload) in frames.iter() {
            if payload.get("id").and_then(Value::as_i64) != Some(id) {
                continue;
            }
            match *listener {
                32 => saw_ready = true,
                31 => {
                    let data = payload["event"]["data"].as_str().unwrap_or_default();
                    if data.contains("MARKER1") {
                        saw_marker = true;
                    }
                }
                33 => {
                    if payload["event"].as_i64() == Some(7) {
                        saw_exit = true;
                    }
                }
                _ => {}
            }
        }
        drop(frames);
        assert!(saw_ready, "onProcessReady missing");
        assert!(saw_marker, "onProcessData MARKER1 missing");
        assert!(saw_exit, "onProcessExit code 7 missing");
        assert!(handle("input", &json!([id, "late\n"])).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn layout_info_round_trips_per_workspace() {
        let layout = json!({
            "workspaceId": "ws-1",
            "tabs": [{ "tabId": 1, "terminals": [ { "id": 5 } ] }],
        });
        handle("setTerminalLayoutInfo", &json!([layout])).expect("set");
        let tabs = handle("getTerminalLayoutInfo", &json!([{ "workspaceId": "ws-1" }]))
            .expect("get");
        assert!(tabs.is_array());
        assert!(serde_json::to_string(&tabs).unwrap_or_default().contains("\"id\":5"));
        // Other workspaces get nothing.
        assert!(
            handle("getTerminalLayoutInfo", &json!([{ "workspaceId": "other" }]))
                .unwrap()
                .is_null()
        );
    }

    #[test]
    fn get_profiles_merges_config_and_detected() {
        let profiles = handle(
            "getProfiles",
            &json!([
                "ws",
                { "My Custom": { "path": "/bin/dash", "args": ["-l"] } },
                "My Custom",
                true,
            ]),
        )
        .expect("getProfiles");
        let list = profiles.as_array().expect("array");
        let names: Vec<&str> = list
            .iter()
            .filter_map(|p| p.get("profileName").and_then(Value::as_str))
            .collect();
        assert!(names.contains(&"My Custom"));
        assert!(names.iter().any(|n| n.contains("sh")), "no detected shell in {:?}", names);
        let custom = list
            .iter()
            .find(|p| p.get("profileName").and_then(Value::as_str) == Some("My Custom"))
            .unwrap();
        assert_eq!(custom.get("isDefault").and_then(Value::as_bool), Some(true));
        assert_eq!(custom.get("path").and_then(Value::as_str), Some("/bin/dash"));
    }

    #[test]
    fn default_system_shell_and_environment_respond() {
        let shell = handle("getDefaultSystemShell", &json!([1])).expect("shell");
        assert!(!shell.as_str().unwrap_or("").is_empty());
        let shell_linux = handle("getDefaultSystemShell", &json!([3])).expect("shell linux");
        assert!(shell_linux.as_str().unwrap_or("").starts_with('/'));
        let env = handle("getEnvironment", &json!([])).expect("env");
        assert!(env.is_object());
    }
}

