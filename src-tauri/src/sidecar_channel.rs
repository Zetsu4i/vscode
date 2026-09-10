//! Mountain-side Node.js sidecar manager — the Electron "utility process"
//! replacement (Phase 7).
//!
//! Electron runs the extension host, the pty host, the file watcher and the
//! TextMate worker as *utility processes*: `bootstrap-fork.js` spawned with
//! `VSCODE_ESM_ENTRYPOINT=<module>`, connected to the renderer through
//! `MessageChannelMain` ports (`win.webContents.postMessage(channel, nonce,
//! [port])` on the renderer side, `process.parentPort.on('message', e =>
//! e.ports[0])` on the child side).
//!
//! VSTauri replaces that with a plain Node.js child process:
//!
//!   node.exe <clientRoot>/vstauri-sidecar.mjs
//!
//! The wrapper (src-tauri/sidecar/vstauri-sidecar.mjs, staged into the client
//! bundle) emulates `process.parentPort` and `MessagePortMain` over a
//! length-prefixed binary framing on stdin/stdout and then imports the
//! original `out/bootstrap-fork.js`, which imports the real entry module.
//! Nothing inside `out/` knows it is not running under Electron.
//!
//! Framing (both directions over the child's stdin/stdout):
//!
//!   [u32 payload_len][u8 frame_type][payload]
//!
//!   frame_type 1 = PORT_MSG: [u64 child_port_id LE][message bytes]
//!                renderer <-> child virtual MessagePort traffic. The Rust
//!                side routes by the port id table below.
//!   frame_type 2 = CTRL (JSON, utf-8):
//!                {"t":"port","portId":n,"data":...}      parent -> child:
//!                  synthesize a `parentPort` message event carrying
//!                  `ports:[FakeMessagePortMain(portId)]` and `data`.
//!                {"t":"ppm","msg":...}                   child -> parent:
//!                  `process.parentPort.postMessage` (lifecycle messages).
//!                {"t":"stdout","data":"..."}             child -> parent
//!                {"t":"stderr","data":"..."}             child -> parent
//!                {"t":"port-close","portId":n}           either direction:
//!                  the port went away.
//!
//! Renderer side: the Wind shim exposes a fake DOM `MessagePort` per
//! connection (`window.vscode.ipcMessagePort.acquire` +
//! `window.__VSTAURI_PORT_MESSAGE__` dispatch — see shim.js). Messages the
//! shim posts arrive here through the `vscode_message_port_send` Tauri
//! command and are written to the child's stdin.
//!
//! Channel surface implemented here:
//!   * `utilityProcessWorker` (IUtilityProcessWorkerService):
//!       createWorker / disposeWorker. `createWorker` mirrors Electron by
//!       resolving its protocol request only when the process terminates
//!       (`{reason:{code,signal}}` — the renderer's `onDidTerminate`).
//!   * `extensionHostStarter` (IExtensionHostStarter):
//!       createExtensionHost / start / kill / waitForExit /
//!       enableInspectPort + the `onDynamicStdout` / `onDynamicStderr` /
//!       `onDynamicMessage` / `onDynamicExit` listen events.
//!   * `vscode:createPtyHostMessageChannel` (plain ipcRenderer channel in
//!     Electron main — see localTerminalBackend.ts) spawns the pty host
//!     sidecar and delivers a port on
//!     `vscode:createPtyHostMessageChannelResult`.

use serde_json::{json, Value};
use std::collections::HashMap;
use std::io::{Read, Write};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicI64, AtomicU64, Ordering};
use std::sync::{Arc, LazyLock, Mutex};

use tauri::Manager;

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

/// How a sidecar was started (drives lifecycle + event routing).
#[derive(Clone, Debug)]
enum SidecarKind {
    /// utilityProcessWorker.createWorker — one process per
    /// (moduleId, windowId), terminated with the window.
    UtilityWorker { key: String },
    /// extensionHostStarter.start — one process per ext host id, terminated
    /// with the owning window (windowLifecycleBound parity).
    ExtensionHost { ext_id: String },
    /// The pty host (spawned on the first
    /// `vscode:createPtyHostMessageChannel` request, shared across windows).
    PtyHost,
}

struct SidecarProcess {
    id: u64,
    kind: SidecarKind,
    /// The window label that owns the process ("" = app-global like the pty
    /// host). Window-bound processes are killed when the window closes.
    owner_label: String,
    entry_module: String,
    child: Mutex<Option<Child>>,
    stdin: Mutex<Option<std::process::ChildStdin>>,
    /// Renderer-facing ports owned by this process: renderer_port_id ->
    /// (child_port_id, target window label).
    ports: Mutex<HashMap<u64, (u64, String)>>,
    /// Ext host exit info once the process terminated.
    exited: Mutex<Option<(i64, String)>>,
}

static NEXT_PROCESS_ID: AtomicU64 = AtomicU64::new(1);
static NEXT_PORT_ID: AtomicU64 = AtomicU64::new(1000);
static NEXT_FAKE_PORT_ID: AtomicU64 = AtomicU64::new(1);

static PROCESSES: LazyLock<Mutex<HashMap<u64, Arc<SidecarProcess>>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));
/// renderer port id -> (process id, child port id). The port-table entry is
/// the single source of truth for routing shim posts into children.
static PORT_TABLE: LazyLock<Mutex<HashMap<u64, (u64, u64)>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));
/// extensionHostStarter id -> process id.
static EXT_HOSTS: LazyLock<Mutex<HashMap<String, u64>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));
/// utilityWorker hash-key -> process id.
static WORKERS: LazyLock<Mutex<HashMap<String, u64>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

/// Deferred protocol responses (request id -> pending answer).
static DEFERRED: LazyLock<Mutex<HashMap<i64, ()>>> = LazyLock::new(|| Mutex::new(HashMap::new()));
/// process id -> deferred createWorker protocol request id (resolved on exit).
static PROCESS_DEFERRED: LazyLock<Mutex<HashMap<u64, i64>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));
/// The exit ledger for waitForExit: process id -> (code, signal).
static EXIT_LEDGER: LazyLock<Mutex<HashMap<u64, (i64, String)>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

// ---------------------------------------------------------------------------
// Public API (ipc.rs channel routing)
// ---------------------------------------------------------------------------

/// `extensionHostStarter` + `utilityProcessWorker` + friends.
///
/// ProxyChannel serializes every method call as `call(command, [args...])`,
/// so single-parameter methods arrive wrapped in a one-element array:
/// `createWorker(config)` comes in as `[{process, reply}]`. The `start`
/// handler already accounts for that; unwrap here so the config-based
/// handlers (`createWorker`/`disposeWorker`) see the object itself.
fn unwrap_proxy_arg(arg: &Value) -> &Value {
    arg.as_array().and_then(|a| a.first()).unwrap_or(arg)
}

pub fn handle(
    app: Option<&tauri::AppHandle>,
    window_label: &str,
    command: &str,
    arg: &Value,
    request_id: i64,
) -> Result<Value, String> {
    match command {
        // ---- IUtilityProcessWorkerService ----
        "createWorker" => {
            let arg = unwrap_proxy_arg(arg);
            let config = arg.as_object().ok_or("createWorker expects an object")?;
            let module_id = config
                .get("process")
                .and_then(|p| p.get("moduleId"))
                .and_then(Value::as_str)
                .ok_or("createWorker: process.moduleId missing")?
                .to_string();
            let reply_channel = config
                .get("reply")
                .and_then(|r| r.get("channel"))
                .and_then(Value::as_str)
                .unwrap_or("vscode:createUtilityProcessWorkerMessageChannelResult")
                .to_string();
            let reply_nonce = config
                .get("reply")
                .and_then(|r| r.get("nonce"))
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string();
            let reply_window = config
                .get("reply")
                .and_then(|r| r.get("windowId"))
                .and_then(Value::as_i64)
                .unwrap_or(1);

            let key = format!("{}#{}", module_id, reply_window);

            // Affinity parity: a second createWorker for the same
            // (module, window) disposes the first.
            if let Some(old) = workers().get(&key).cloned() {
                kill_process(old);
            }

            let process = spawn(
                app,
                window_label,
                &module_id,
                SidecarKind::UtilityWorker { key: key.clone() },
                None,
            )?;

            workers().insert(key, process.id);

            // Create the renderer<->child port pair and deliver the renderer
            // end immediately (Electron posts it after the process spawns).
            deliver_new_port(app, &process, &reply_channel, &reply_nonce, Value::Null);

            // Electron resolves the createWorker request only when the
            // process terminates; mirror that by deferring our answer.
            {
                let mut guard = DEFERRED.lock().unwrap_or_else(|p| p.into_inner());
                guard.insert(request_id, ());
            }
            deferred_requests().insert(process.id, request_id);
            crate::ipc::defer_response(request_id);

            Ok(Value::Null)
        }
        "disposeWorker" => {
            let arg = unwrap_proxy_arg(arg);
            let config = arg.as_object().ok_or("disposeWorker expects an object")?;
            let module_id = config
                .get("process")
                .and_then(|p| p.get("moduleId"))
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string();
            let reply_window = config
                .get("reply")
                .and_then(|r| r.get("windowId"))
                .and_then(Value::as_i64)
                .unwrap_or(1);
            let key = format!("{}#{}", module_id, reply_window);
            if let Some(id) = workers().remove(&key) {
                kill_process(id);
            }
            Ok(Value::Null)
        }

        // ---- IExtensionHostStarter ----
        "createExtensionHost" => {
            let ext_id = next_ext_host_id();
            let processes = processes();
            // Reserve the id (the process spawns with `start`).
            drop(processes);
            ext_hosts().insert(ext_id.clone(), 0);
            Ok(json!({ "id": ext_id }))
        }
        "start" => {
            // ProxyChannel turns `start(id, opts)` into
            // `call('start', [id, opts])` — but IExtensionHostStarter.start
            // has signature (id, opts) so arg is [id, opts]? No: ProxyChannel
            // passes the method args as an array: arg = [id, opts].
            let arr = arg.as_array().ok_or("extensionHostStarter.start expects [id, opts]")?;
            let ext_id = arr
                .first()
                .and_then(Value::as_str)
                .ok_or("extensionHostStarter.start: id missing")?
                .to_string();
            let opts = arr.get(1).cloned().unwrap_or(Value::Null);
            let obj = opts.as_object().ok_or("extensionHostStarter.start: opts missing")?;

            let response_channel = obj
                .get("responseChannel")
                .and_then(Value::as_str)
                .unwrap_or("vscode:startExtensionHostMessagePortResult")
                .to_string();
            let response_nonce = obj
                .get("responseNonce")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string();
            let env = obj.get("env").cloned().unwrap_or(Value::Null);
            let exec_argv: Vec<String> = obj
                .get("execArgv")
                .and_then(Value::as_array)
                .map(|list| {
                    list.iter()
                        .filter_map(Value::as_str)
                        .map(str::to_string)
                        .collect()
                })
                .unwrap_or_default();

            let process = spawn(
                app,
                window_label,
                "vs/workbench/api/node/extensionHostProcess",
                SidecarKind::ExtensionHost {
                    ext_id: ext_id.clone(),
                },
                Some((env, exec_argv)),
            )?;

            ext_hosts().insert(ext_id, process.id);

            // The renderer already listens on `responseChannel` for the
            // nonce + port (acquirePort). Deliver the port now.
            deliver_new_port(
                app,
                &process,
                &response_channel,
                &response_nonce,
                Value::String(response_nonce.clone()),
            );

            Ok(json!({ "pid": process_pid(&process) }))
        }
        "kill" => {
            let ext_id = arg_id(arg);
            if let Some(id) = ext_hosts().get(&ext_id).cloned() {
                kill_process(id);
            }
            Ok(Value::Null)
        }
        "enableInspectPort" => {
            // Plain Node does not implement Electron's SIGUSR1 inspect-port
            // dance; report "not enabled" and let the renderer fall back.
            Ok(json!(false))
        }
        "waitForExit" => {
            let ext_id = arg_id(arg);
            let max_wait = arg
                .as_array()
                .and_then(|a| a.get(1))
                .and_then(Value::as_i64)
                .unwrap_or(6000);
            if let Some(id) = ext_hosts().get(&ext_id).cloned() {
                wait_for_exit(id, max_wait);
            }
            Ok(Value::Null)
        }
        "_killAllNow" => {
            kill_all();
            Ok(Value::Null)
        }

        _ => Err(format!(
            "sidecar channel: method not found: {}",
            command
        )),
    }
}

/// The plain ipcRenderer channel Electron-main handles by spawning the pty
/// host and transferring a port back on
/// `vscode:createPtyHostMessageChannelResult`.
pub fn handle_pty_host_channel(
    app: Option<&tauri::AppHandle>,
    window_label: &str,
    args: &[Value],
) -> Result<Value, String> {
    // args[0] is the nonce the renderer passed; the response event carries
    // it back as data.
    let nonce = args
        .first()
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();

    let process = spawn(
        app,
        window_label,
        "vs/platform/terminal/node/ptyHostMain",
        SidecarKind::PtyHost,
        None,
    )?;

    deliver_new_port(
        app,
        &process,
        "vscode:createPtyHostMessageChannelResult",
        &nonce,
        Value::String(nonce.clone()),
    );

    Ok(Value::Null)
}

/// Whether a sidecar-backed channel was handled (kept for the routing
/// dispatch table — ipc.rs currently routes the two channels directly).
#[allow(dead_code)]
pub fn owns_channel(channel: &str) -> bool {
    matches!(
        channel,
        "utilityProcessWorker" | "extensionHostStarter"
    )
}

/// Kill every window-bound sidecar owned by `window_label` (window close /
/// reload parity — Electron's windowLifecycleBound).
pub fn kill_window_processes(window_label: &str) {
    let victims: Vec<u64> = processes()
        .iter()
        .filter(|(_, p)| p.owner_label == window_label)
        .map(|(id, _)| *id)
        .collect();
    for id in victims {
        kill_process(id);
    }
}

/// Kill everything (app shutdown).
pub fn kill_all() {
    let ids: Vec<u64> = processes().keys().cloned().collect();
    for id in ids {
        kill_process(id);
    }
}

/// The renderer shim posted a message on virtual port `port_id`.
pub fn port_message_from_renderer(port_id: u64, bytes: &[u8]) {
    let route = port_table().get(&port_id).cloned();
    let Some((process_id, child_port_id)) = route else {
        return;
    };
    let process = processes().get(&process_id).cloned();
    let Some(process) = process else {
        return;
    };
    let mut payload = Vec::with_capacity(bytes.len() + 8);
    payload.extend_from_slice(&child_port_id.to_le_bytes());
    payload.extend_from_slice(bytes);
    write_frame(&process, 1, &payload);
}

/// The renderer closed virtual port `port_id` (shim `port.close()`).
pub fn port_closed_from_renderer(port_id: u64) {
    let route = port_table().remove(&port_id);
    let Some((process_id, child_port_id)) = route else {
        return;
    };
    let process = processes().get(&process_id).cloned();
    if let Some(process) = process {
        let payload = serde_json::to_vec(&json!({ "t": "port-close", "portId": child_port_id }))
            .unwrap_or_default();
        write_frame(&process, 2, &payload);
        let mut ports = process.ports.lock().unwrap_or_else(|p| p.into_inner());
        ports.remove(&port_id);
    }
}

// ---------------------------------------------------------------------------
// Spawning
// ---------------------------------------------------------------------------

fn spawn(
    app: Option<&tauri::AppHandle>,
    window_label: &str,
    entry_module: &str,
    kind: SidecarKind,
    ext_opts: Option<(Value, Vec<String>)>,
) -> Result<Arc<SidecarProcess>, String> {
    let app = app.ok_or("sidecar: no app handle")?;
    let client_root = crate::protocol::client_root(app);
    let node_exe = resolve_node_exe(app, client_root)?;
    let wrapper = client_root.join("vstauri-sidecar.mjs");
    if !wrapper.is_file() {
        return Err(format!(
            "sidecar: wrapper missing from client bundle: {}",
            wrapper.display()
        ));
    }

    let id = NEXT_PROCESS_ID.fetch_add(1, Ordering::SeqCst);

    let mut cmd = Command::new(&node_exe);
    // Never flash a console window for the Node sidecar. Every spawn on
    // Windows without CREATE_NO_WINDOW opens (and closes, on exit) a
    // visible cmd host — with the extension host's crash-retry loop that
    // used to pop a console every few seconds. Applies to all helper
    // processes spawned by the shell (see also terminal_channel.rs and
    // native_host.rs).
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }
    cmd.arg(&wrapper)
        .current_dir(client_root)
        .env("VSCODE_ESM_ENTRYPOINT", entry_module)
        .env("VSCODE_SIDECAR_TRANSPORT", "stdio")
        .env("VSCODE_PARENT_PID", std::process::id().to_string())
        .env("VSCODE_CRASH_REPORTER_PROCESS_TYPE", "sidecar")
        .env("VSCODE_PIPE_LOGGING", "false")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    // NLS parity: the product bundle compiles message placeholders out and
    // reads them back from nls.messages.json at boot (bootstrap-esm.ts
    // doSetupNLS). Without VSCODE_NLS_CONFIG every localized string in the
    // sidecar renders as its raw NLS key.
    let nls_messages = client_root.join("nls.messages.json");
    if nls_messages.is_file() {
        let config = serde_json::json!({
            "locale": "en",
            "availableLanguages": {},
            "defaultMessagesFile": nls_messages.to_string_lossy(),
        });
        if let Ok(text) = serde_json::to_string(&config) {
            cmd.env("VSCODE_NLS_CONFIG", text);
        }
    }

    // Exec argv parity: keep only flags plain Node understands; unknown
    // experimental Electron flags would abort the process at boot.
    if let Some((_, exec_argv)) = &ext_opts {
        for flag in exec_argv {
            let safe = flag == "--nolazy"
                || flag == "--expose-gc"
                || flag == "--prof"
                || flag == "--dns-result-order=ipv4first"
                || flag.starts_with("--inspect")
                || flag.starts_with("--inspect-brk")
                || flag.starts_with("--inspect-port");
            if safe {
                cmd.arg(flag);
            } else {
                crate::logger::log_app(
                    "warn",
                    &format!("sidecar: dropping unsupported execArgv {}", flag),
                );
            }
        }
    }

    // Extension host env mixin (opts.env — a full process env from the
    // renderer's shellEnvironmentService + markers). VSCODE_NLS_CONFIG is
    // owned by this spawn (the renderer env never carries a valid one).
    if let Some((env, _)) = &ext_opts {
        if let Some(map) = env.as_object() {
            for (key, value) in map {
                if let Some(text) = value.as_str() {
                    if key == "VSCODE_NLS_CONFIG" {
                        continue;
                    }
                    if key.starts_with("VSCODE_") || key == "ELECTRON_RUN_AS_NODE" {
                        cmd.env(key, text);
                    }
                }
            }
        }
    }

    let mut child = cmd
        .spawn()
        .map_err(|err| format!("sidecar: cannot spawn node {}: {}", node_exe.display(), err))?;
    let pid = child.id();
    let stdin = child.stdin.take();
    let stdout = child.stdout.take();
    let stderr = child.stderr.take();

    crate::logger::log_app(
        "info",
        &format!(
            "sidecar: spawned pid {} for {} (window: {})",
            pid, entry_module, window_label
        ),
    );

    let process = Arc::new(SidecarProcess {
        id,
        kind,
        owner_label: window_label.to_string(),
        entry_module: entry_module.to_string(),
        child: Mutex::new(Some(child)),
        stdin: Mutex::new(stdin),
        ports: Mutex::new(HashMap::new()),
        exited: Mutex::new(None),
    });
    processes().insert(id, process.clone());

    // stdout reader thread: frames.
    if let Some(stdout) = stdout {
        let process_for_reader = process.clone();
        std::thread::spawn(move || read_frames(stdout, process_for_reader));
    }
    // stderr passthrough (native crashes, node boot errors).
    if let Some(stderr) = stderr {
        std::thread::spawn(move || {
            let mut buf = Vec::new();
            let mut reader = std::io::BufReader::new(stderr);
            if reader.read_to_end(&mut buf).is_ok() && !buf.is_empty() {
                let text = String::from_utf8_lossy(&buf).to_string();
                crate::logger::log_app("error", &format!("sidecar stderr: {}", text.trim()));
            }
        });
    }

    Ok(process)
}

/// Create a virtual port pair for `process`, register the routing, and
/// dispatch the renderer end into the owning webview window:
/// `window.__VSTAURI_DELIVER_PORT__(channel, nonce, portId, data)`.
fn deliver_new_port(
    app: Option<&tauri::AppHandle>,
    process: &Arc<SidecarProcess>,
    channel: &str,
    nonce: &str,
    data: Value,
) {
    let app = match app {
        Some(app) => app,
        None => return,
    };
    let renderer_port = NEXT_PORT_ID.fetch_add(1, Ordering::SeqCst);
    let child_port = NEXT_FAKE_PORT_ID.fetch_add(1, Ordering::SeqCst);

    // Register both directions.
    {
        let mut ports = process.ports.lock().unwrap_or_else(|p| p.into_inner());
        ports.insert(renderer_port, (child_port, process.owner_label.clone()));
    }
    port_table().insert(renderer_port, (process.id, child_port));

    // Tell the child its end of the connection (parentPort message event).
    let payload = serde_json::to_vec(&json!({ "t": "port", "portId": child_port, "data": data }))
        .unwrap_or_default();
    write_frame(process, 2, &payload);

    // Deliver the renderer end.
    let js = format!(
        "(window.__VSTAURI_DELIVER_PORT__||function(){{}})({}, {}, {}, null)",
        serde_json::to_string(channel).unwrap_or_default(),
        serde_json::to_string(nonce).unwrap_or_default(),
        renderer_port
    );
    eval_in_window(app, &process.owner_label, &js);

    crate::logger::log_app(
        "info",
        &format!(
            "sidecar: delivered port {} (child {}) on '{}' to window '{}'",
            renderer_port, child_port, channel, process.owner_label
        ),
    );
}

/// Write one framed message to the child's stdin:
/// [u32 body_len][u8 type][payload] where body_len = 1 + payload.len().
fn write_frame(process: &Arc<SidecarProcess>, frame_type: u8, payload: &[u8]) {
    let mut guard = process.stdin.lock().unwrap_or_else(|p| p.into_inner());
    if let Some(stdin) = guard.as_mut() {
        let len = (payload.len() as u32) + 1;
        let _ = stdin.write_all(&len.to_le_bytes());
        let _ = stdin.write_all(&[frame_type]);
        let _ = stdin.write_all(payload);
        let _ = stdin.flush();
    }
}

/// The stdout reader: parse frames and route them.
fn read_frames(mut stdout: std::process::ChildStdout, process: Arc<SidecarProcess>) {
    loop {
        let mut len_buf = [0u8; 4];
        match stdout.read_exact(&mut len_buf) {
            Ok(()) => {}
            Err(_) => break,
        }
        let len = u32::from_le_bytes(len_buf) as usize; // 1 type byte + payload
        if len == 0 || len > 64 * 1024 * 1024 {
            break;
        }
        let mut type_buf = [0u8; 1];
        if stdout.read_exact(&mut type_buf).is_err() {
            break;
        }
        let mut payload = vec![0u8; len - 1];
        if stdout.read_exact(&mut payload).is_err() {
            break;
        }

        match type_buf[0] {
            1 => {
                // PORT_MSG: [u64 child port id][message bytes]
                if payload.len() < 8 {
                    continue;
                }
                let mut port_bytes = [0u8; 8];
                port_bytes.copy_from_slice(&payload[..8]);
                let child_port = u64::from_le_bytes(port_bytes);
                let message = &payload[8..];
                route_child_message(&process, child_port, message);
            }
            2 => {
                // CTRL JSON
                if let Ok(value) = serde_json::from_slice::<Value>(&payload) {
                    route_child_ctrl(&process, &value);
                }
            }
            _ => {}
        }
    }

    // EOF: the process is gone.
    on_process_exit(&process);
}

fn route_child_message(process: &Arc<SidecarProcess>, child_port: u64, message: &[u8]) {
    // Find the renderer port mapped to this child port.
    let renderer_port = {
        let ports = process.ports.lock().unwrap_or_else(|p| p.into_inner());
        ports
            .iter()
            .find(|(_, (cp, _))| *cp == child_port)
            .map(|(rp, _)| *rp)
    };
    let Some(renderer_port) = renderer_port else {
        return;
    };
    let Some(app) = crate::ipc::current_app_handle() else {
        return;
    };
    let b64 = crate::ipc::base64_encode_public(message);
    let js = format!(
        "(window.__VSTAURI_PORT_MESSAGE__||function(){{}})({},{})",
        renderer_port, serde_json::to_string(&b64).unwrap_or_default()
    );
    eval_in_window(app, &process.owner_label, &js);
}

fn route_child_ctrl(process: &Arc<SidecarProcess>, value: &Value) {
    let kind = value.get("t").and_then(Value::as_str).unwrap_or("");
    match kind {
        "ppm" => {
            let msg = value.get("msg").cloned().unwrap_or(Value::Null);
            crate::logger::log_app(
                "info",
                &format!("sidecar[{}]: parentPort message: {}", process.entry_module, msg),
            );
            // Shared-process lifecycle / ext host message events.
            if let SidecarKind::ExtensionHost { ext_id } = &process.kind {
                crate::ipc::fire_event_with_arg(
                    "extensionHostStarter",
                    "onDynamicMessage",
                    &Value::String(ext_id.clone()),
                    &msg,
                );
            }
        }
        "stdout" => {
            let data = value.get("data").and_then(Value::as_str).unwrap_or("");
            crate::logger::log_app(
                "info",
                &format!("[sidecar:{}] {}", process.entry_module, data.trim_end()),
            );
            if let SidecarKind::ExtensionHost { ext_id } = &process.kind {
                crate::ipc::fire_event_with_arg(
                    "extensionHostStarter",
                    "onDynamicStdout",
                    &Value::String(ext_id.clone()),
                    &json!(data),
                );
            }
        }
        "stderr" => {
            let data = value.get("data").and_then(Value::as_str).unwrap_or("");
            crate::logger::log_app(
                "warn",
                &format!("[sidecar:{}] {}", process.entry_module, data.trim_end()),
            );
            if let SidecarKind::ExtensionHost { ext_id } = &process.kind {
                crate::ipc::fire_event_with_arg(
                    "extensionHostStarter",
                    "onDynamicStderr",
                    &Value::String(ext_id.clone()),
                    &json!(data),
                );
            }
        }
        "port-close" => {
            let child_port = value.get("portId").and_then(Value::as_u64).unwrap_or(0);
            let renderer_port = {
                let mut ports = process.ports.lock().unwrap_or_else(|p| p.into_inner());
                // find the renderer port bound to the closing child port
                // (collect first: the map is mutated right after)
                let found = ports
                    .iter()
                    .find(|(_, (cp, _))| *cp == child_port)
                    .map(|(rp, _)| *rp);
                match found {
                    Some(rp) => {
                        ports.remove(&rp);
                        Some(rp)
                    }
                    None => None,
                }
            };
            if let Some(rp) = renderer_port {
                port_table().remove(&rp);
            }
        }
        _ => {}
    }
}

fn on_process_exit(process: &Arc<SidecarProcess>) {
    // Harvest the exit code.
    let mut exit_code: i64 = 0;
    {
        let mut guard = process.child.lock().unwrap_or_else(|p| p.into_inner());
        if let Some(mut child) = guard.take() {
            if let Ok(status) = child.wait() {
                exit_code = status.code().unwrap_or(-1) as i64;
            }
        }
        *process.stdin.lock().unwrap_or_else(|p| p.into_inner()) = None;
    }
    let signal = "unknown".to_string();
    {
        let mut exited = process.exited.lock().unwrap_or_else(|p| p.into_inner());
        *exited = Some((exit_code, signal.clone()));
    }
    exit_ledger().insert(process.id, (exit_code, signal.clone()));

    // Drop the ports.
    {
        let ports = process.ports.lock().unwrap_or_else(|p| p.into_inner());
        let table = &mut *port_table();
        for (renderer_port, _) in ports.iter() {
            table.remove(renderer_port);
        }
    }

    crate::logger::log_app(
        "info",
        &format!(
            "sidecar: process {} ({}) exited with code {}",
            process.id, process.entry_module, exit_code
        ),
    );

    // Fire the ext host exit event.
    if let SidecarKind::ExtensionHost { ext_id } = &process.kind {
        crate::ipc::fire_event_with_arg(
            "extensionHostStarter",
            "onDynamicExit",
            &Value::String(ext_id.clone()),
            &json!({ "pid": process_pid(process), "code": exit_code, "signal": signal }),
        );
    }

    // Resolve a deferred createWorker request (onDidTerminate parity).
    let deferred = deferred_requests().remove(&process.id);
    processes().remove(&process.id);
    match &process.kind {
        SidecarKind::UtilityWorker { key } => {
            workers().remove(key);
        }
        SidecarKind::ExtensionHost { ext_id } => {
            ext_hosts().remove(ext_id);
        }
        SidecarKind::PtyHost => {}
    }
    if let Some(request_id) = deferred {
        crate::ipc::resolve_deferred_to(
            request_id,
            &process.owner_label,
            Ok(json!({ "reason": { "code": exit_code, "signal": signal } })),
        );
    }
}

fn kill_process(id: u64) {
    let process = processes().get(&id).cloned();
    let Some(process) = process else { return };
    {
        let mut guard = process.child.lock().unwrap_or_else(|p| p.into_inner());
        if let Some(mut child) = guard.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
        *process.stdin.lock().unwrap_or_else(|p| p.into_inner()) = None;
    }
    // The stdout reader thread hits EOF and finishes cleanup.
}

fn wait_for_exit(id: u64, max_wait_ms: i64) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_millis(max_wait_ms.max(0) as u64);
    while std::time::Instant::now() < deadline {
        if !processes().contains_key(&id) {
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
}

fn process_pid(process: &Arc<SidecarProcess>) -> Option<i64> {
    let guard = process.child.lock().unwrap_or_else(|p| p.into_inner());
    guard.as_ref().map(|child| child.id() as i64)
}

// ---------------------------------------------------------------------------
// Environment / paths
// ---------------------------------------------------------------------------

fn resolve_node_exe(app: &tauri::AppHandle, client_root: &std::path::Path) -> Result<std::path::PathBuf, String> {
    // 1. Explicit override (dev machines).
    if let Ok(path) = std::env::var("VSTAURI_NODE_EXE") {
        if !path.is_empty() {
            let path = std::path::PathBuf::from(path);
            if path.is_file() {
                return Ok(path);
            }
        }
    }
    // 2. Bundled runtime: <install>/resources/node/node.exe
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            let node = dir.join("resources").join("node").join("node.exe");
            if node.is_file() {
                return Ok(node);
            }
            let node = dir.join("resources").join("client").join("node").join("node.exe");
            if node.is_file() {
                return Ok(node);
            }
        }
    }
    // 3. Dev checkout: client root next to the exe.
    let node = client_root.join("node").join("node.exe");
    if node.is_file() {
        return Ok(node);
    }
    // 4. System node (best effort for development).
    let _ = app;
    if let Ok(path) = which_node() {
        return Ok(path);
    }
    Err("sidecar: node.exe not found (expected <install>/resources/node/node.exe)".to_string())
}

fn which_node() -> Result<std::path::PathBuf, String> {
    let path_var = std::env::var("PATH").unwrap_or_default();
    for dir in path_var.split(';').chain(path_var.split(':')) {
        if dir.is_empty() {
            continue;
        }
        let candidate = std::path::Path::new(dir).join("node.exe");
        if candidate.is_file() {
            return Ok(candidate);
        }
    }
    Err("node.exe not on PATH".to_string())
}

// ---------------------------------------------------------------------------
// Small helpers
// ---------------------------------------------------------------------------

fn eval_in_window(app: &tauri::AppHandle, label: &str, js: &str) {
    let window = if label.is_empty() {
        app.get_webview_window("main")
    } else {
        app.get_webview_window(label).or_else(|| app.get_webview_window("main"))
    };
    if let Some(window) = window {
        if let Err(err) = window.eval(js) {
            crate::logger::log_app("warn", &format!("sidecar: eval failed: {}", err));
        }
    }
}

fn next_ext_host_id() -> String {
    static LAST: AtomicI64 = AtomicI64::new(0);
    format!("{}", LAST.fetch_add(1, Ordering::SeqCst) + 1)
}

/// ProxyChannel `call('start', [id, opts])` passes ids as the first array
/// element; `onDynamic*` listens pass the raw id as the arg.
fn arg_id(arg: &Value) -> String {
    if let Some(id) = arg.as_str() {
        return id.to_string();
    }
    if let Some(arr) = arg.as_array() {
        if let Some(id) = arr.first().and_then(Value::as_str) {
            return id.to_string();
        }
    }
    String::new()
}

fn processes() -> std::sync::MutexGuard<'static, HashMap<u64, Arc<SidecarProcess>>> {
    PROCESSES.lock().unwrap_or_else(|p| p.into_inner())
}
fn deferred_requests() -> std::sync::MutexGuard<'static, HashMap<u64, i64>> {
    PROCESS_DEFERRED.lock().unwrap_or_else(|p| p.into_inner())
}
fn workers() -> std::sync::MutexGuard<'static, HashMap<String, u64>> {
    WORKERS.lock().unwrap_or_else(|p| p.into_inner())
}
fn ext_hosts() -> std::sync::MutexGuard<'static, HashMap<String, u64>> {
    EXT_HOSTS.lock().unwrap_or_else(|p| p.into_inner())
}
fn port_table() -> std::sync::MutexGuard<'static, HashMap<u64, (u64, u64)>> {
    PORT_TABLE.lock().unwrap_or_else(|p| p.into_inner())
}
fn exit_ledger() -> std::sync::MutexGuard<'static, HashMap<u64, (i64, String)>> {
    EXIT_LEDGER.lock().unwrap_or_else(|p| p.into_inner())
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn port_table_routes_roundtrip() {
        port_table().insert(42, (7, 99));
        assert_eq!(port_table().get(&42), Some(&(7, 99)));
        port_table().remove(&42);
    }
}
