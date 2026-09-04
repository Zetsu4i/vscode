use serde_json::{json, Value};
use std::collections::HashMap;
use std::path::Path;
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::{Arc, Mutex as StdMutex};
use tauri::{AppHandle, Emitter, State};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin, Command};
use tokio::sync::{oneshot, Mutex as TokioMutex};

pub struct LspServer {
    pub stdin: Arc<TokioMutex<ChildStdin>>,
    pub pending: Arc<StdMutex<HashMap<i64, oneshot::Sender<Value>>>>,
    pub next_id: AtomicI64,
    pub child: Arc<TokioMutex<Child>>,
    pub name: String,
}

#[derive(Default)]
pub struct LspState {
    pub servers: TokioMutex<HashMap<String, Arc<LspServer>>>,
}

type ServerConfig = (String, Vec<String>);

fn builtin_servers() -> HashMap<&'static str, ServerConfig> {
    let mut m: HashMap<&'static str, ServerConfig> = HashMap::new();
    m.insert("rust", ("rust-analyzer".into(), vec![]));
    m.insert(
        "typescript",
        ("typescript-language-server".into(), vec!["--stdio".into()]),
    );
    m.insert(
        "javascript",
        ("typescript-language-server".into(), vec!["--stdio".into()]),
    );
    m.insert("python", ("pylsp".into(), vec![]));
    m.insert("go", ("gopls".into(), vec![]));
    m.insert("c", ("clangd".into(), vec![]));
    m.insert("cpp", ("clangd".into(), vec![]));
    m
}

/// Workspace override: `.vstauri/lsp.json` maps language ids to a command, e.g.
/// { "rust": { "command": "rust-analyzer", "args": [] } }
fn load_server_config(root: &str, language: &str) -> Option<ServerConfig> {
    let cfg_path = Path::new(root).join(".vstauri").join("lsp.json");
    if let Ok(text) = std::fs::read_to_string(&cfg_path) {
        if let Ok(Value::Object(map)) = serde_json::from_str::<Value>(&text) {
            if let Some(entry) = map.get(language) {
                let cmd = entry.get("command").and_then(|c| c.as_str());
                if let Some(cmd) = cmd {
                    let args = entry
                        .get("args")
                        .and_then(|a| a.as_array())
                        .map(|arr| {
                            arr.iter()
                                .filter_map(|v| v.as_str().map(|s| s.to_string()))
                                .collect()
                        })
                        .unwrap_or_default();
                    return Some((cmd.to_string(), args));
                }
            }
        }
    }
    builtin_servers().remove(language)
}

fn path_to_uri(path: &str) -> String {
    let norm = path.replace('\\', "/");
    let norm = if norm.starts_with('/') {
        norm
    } else {
        format!("/{}", norm)
    };
    let mut out = String::from("file://");
    for ch in norm.chars() {
        match ch {
            'A'..='Z' | 'a'..='z' | '0'..='9' | '-' | '.' | '_' | '~' | '/' | ':' => out.push(ch),
            _ => out.push_str(&format!("%{:02X}", ch as u32 & 0xFF)),
        }
    }
    out
}

async fn write_frame(stdin: &Arc<TokioMutex<ChildStdin>>, value: &Value) -> Result<(), String> {
    let body = serde_json::to_vec(value).map_err(|e| e.to_string())?;
    let mut w = stdin.lock().await;
    w.write_all(format!("Content-Length: {}\r\n\r\n", body.len()).as_bytes())
        .await
        .map_err(|e| e.to_string())?;
    w.write_all(&body).await.map_err(|e| e.to_string())?;
    w.flush().await.map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
pub async fn lsp_start(
    app: AppHandle,
    state: State<'_, LspState>,
    root: String,
    language: String,
) -> Result<String, String> {
    {
        let servers = state.servers.lock().await;
        if servers.contains_key(&language) {
            return Ok(format!("{} already running", language));
        }
    }

    let (cmd, args) = load_server_config(&root, &language)
        .ok_or_else(|| format!("No language server configured for '{}'", language))?;

    let mut child = Command::new(&cmd)
        .args(&args)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map_err(|e| format!("Cannot start '{}': {}", cmd, e))?;

    let stdin = child.stdin.take().ok_or("Failed to capture server stdin")?;
    let stdout = child
        .stdout
        .take()
        .ok_or("Failed to capture server stdout")?;

    let server = Arc::new(LspServer {
        stdin: Arc::new(TokioMutex::new(stdin)),
        pending: Arc::new(StdMutex::new(HashMap::new())),
        next_id: AtomicI64::new(1),
        child: Arc::new(TokioMutex::new(child)),
        name: cmd.clone(),
    });

    // Reader task: parse LSP frames, dispatch responses + diagnostics events
    {
        let pending = server.pending.clone();
        let app_reader = app.clone();
        tokio::spawn(async move {
            let mut reader = BufReader::new(stdout);
            loop {
                let mut content_length: Option<usize> = None;
                loop {
                    let mut header = String::new();
                    match reader.read_line(&mut header).await {
                        Ok(0) => return, // EOF
                        Ok(_) => {
                            let t = header.trim();
                            if t.is_empty() {
                                break; // end of headers
                            }
                            let lower = t.to_ascii_lowercase();
                            if lower.starts_with("content-length:") {
                                if let Some(v) = lower.splitn(2, ':').nth(1) {
                                    content_length = v.trim().parse::<usize>().ok();
                                }
                            }
                        }
                        Err(_) => return,
                    }
                }
                let len = match content_length {
                    Some(l) => l,
                    None => continue,
                };
                let mut body = vec![0u8; len];
                if reader.read_exact(&mut body).await.is_err() {
                    return;
                }
                let msg: Value = match serde_json::from_slice(&body) {
                    Ok(m) => m,
                    Err(_) => continue,
                };
                if let Some(method) = msg.get("method").and_then(|m| m.as_str()) {
                    if method == "textDocument/publishDiagnostics" {
                        if let Some(params) = msg.get("params") {
                            let _ = app_reader.emit("lsp-diagnostics", params.clone());
                        }
                    }
                    // other notifications are ignored for now
                } else if let Some(id) = msg.get("id").and_then(|i| i.as_i64()) {
                    if let Some(tx) = pending.lock().unwrap().remove(&id) {
                        let _ = tx.send(msg.get("result").cloned().unwrap_or(Value::Null));
                    }
                }
            }
        });
    }

    state
        .servers
        .lock()
        .await
        .insert(language.clone(), server.clone());

    // initialize -> initialized
    let root_name = Path::new(&root)
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| root.clone());
    let init_result = send_request(
        &server,
        "initialize",
        json!({
            "processId": std::process::id(),
            "rootUri": path_to_uri(&root),
            "workspaceFolders": [{ "uri": path_to_uri(&root), "name": root_name }],
            "clientInfo": { "name": "VSTauri" },
            "capabilities": {
                "textDocument": {
                    "synchronization": { "dynamicRegistration": false, "didSave": true, "willSave": false },
                    "completion": { "completionItem": { "snippetSupport": false, "documentationFormat": ["plaintext"] } },
                    "hover": { "contentFormat": ["markdown", "plaintext"] }
                },
                "workspace": { "configuration": false, "workspaceFolders": true }
            }
        }),
        30_000,
    )
    .await
    .map_err(|e| {
        format!("'{}' did not initialize: {}", cmd, e)
    })?;

    let _server_info = init_result;
    send_notification(&server, "initialized", json!({})).await?;

    Ok(format!("{} started", cmd))
}

async fn send_request(
    server: &Arc<LspServer>,
    method: &str,
    params: Value,
    timeout_ms: u64,
) -> Result<Value, String> {
    let id = server.next_id.fetch_add(1, Ordering::SeqCst);
    let (tx, rx) = oneshot::channel::<Value>();
    server.pending.lock().unwrap().insert(id, tx);

    write_frame(
        &server.stdin,
        &json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params }),
    )
    .await?;

    match tokio::time::timeout(std::time::Duration::from_millis(timeout_ms), rx).await {
        Ok(Ok(value)) => Ok(value),
        Ok(Err(_)) => Err("server dropped the response".to_string()),
        Err(_) => {
            server.pending.lock().unwrap().remove(&id);
            Err("request timed out".to_string())
        }
    }
}

async fn send_notification(
    server: &Arc<LspServer>,
    method: &str,
    params: Value,
) -> Result<(), String> {
    write_frame(
        &server.stdin,
        &json!({ "jsonrpc": "2.0", "method": method, "params": params }),
    )
    .await
}

#[tauri::command]
pub async fn lsp_status(state: State<'_, LspState>, language: String) -> Result<String, String> {
    let servers = state.servers.lock().await;
    Ok(if servers.contains_key(&language) {
        "running".to_string()
    } else {
        "stopped".to_string()
    })
}

#[tauri::command]
pub async fn lsp_stop(state: State<'_, LspState>, language: String) -> Result<(), String> {
    if let Some(server) = state.servers.lock().await.remove(&language) {
        let _ = send_request(&server, "shutdown", Value::Null, 2_000).await;
        let _ = send_notification(&server, "exit", Value::Null).await;
        // tokio::sync::Mutex has no poisoning — lock() returns the guard directly.
        let mut child = server.child.lock().await;
        let _ = child.kill().await;
    }
    Ok(())
}

#[tauri::command]
pub async fn lsp_did_open(
    state: State<'_, LspState>,
    language: String,
    path: String,
    text: String,
    version: i64,
) -> Result<(), String> {
    let server = state
        .servers
        .lock()
        .await
        .get(&language)
        .cloned()
        .ok_or_else(|| format!("LSP '{}' is not running", language))?;
    send_notification(
        &server,
        "textDocument/didOpen",
        json!({
            "textDocument": {
                "uri": path_to_uri(&path),
                "languageId": language,
                "version": version,
                "text": text
            }
        }),
    )
    .await
}

#[tauri::command]
pub async fn lsp_did_change(
    state: State<'_, LspState>,
    language: String,
    path: String,
    changes: Value,
    version: i64,
) -> Result<(), String> {
    let server = state
        .servers
        .lock()
        .await
        .get(&language)
        .cloned()
        .ok_or_else(|| format!("LSP '{}' is not running", language))?;
    // `changes` is an array of LSP TextDocumentContentChangeEvent values:
    // { range?: { start: {line, character}, end: {line, character} }, text: String }
    // A missing/null range means a full-document replacement.
    let content_changes = match changes {
        Value::Array(arr) => arr,
        Value::String(text) => vec![json!({ "text": text })],
        _ => vec![],
    };
    send_notification(
        &server,
        "textDocument/didChange",
        json!({
            "textDocument": { "uri": path_to_uri(&path), "version": version },
            "contentChanges": content_changes
        }),
    )
    .await
}

#[tauri::command]
pub async fn lsp_did_save(
    state: State<'_, LspState>,
    language: String,
    path: String,
) -> Result<(), String> {
    let server = state
        .servers
        .lock()
        .await
        .get(&language)
        .cloned()
        .ok_or_else(|| format!("LSP '{}' is not running", language))?;
    send_notification(
        &server,
        "textDocument/didSave",
        json!({ "textDocument": { "uri": path_to_uri(&path) } }),
    )
    .await
}

#[tauri::command]
pub async fn lsp_completion(
    state: State<'_, LspState>,
    language: String,
    path: String,
    line: u32,
    character: u32,
) -> Result<Vec<Value>, String> {
    let server = state
        .servers
        .lock()
        .await
        .get(&language)
        .cloned()
        .ok_or_else(|| format!("LSP '{}' is not running", language))?;
    let result = send_request(
        &server,
        "textDocument/completion",
        json!({
            "textDocument": { "uri": path_to_uri(&path) },
            "position": { "line": line, "character": character }
        }),
        5_000,
    )
    .await?;
    Ok(match result {
        Value::Array(items) => items,
        Value::Object(ref o) => o
            .get("items")
            .and_then(|i| i.as_array())
            .cloned()
            .unwrap_or_default(),
        _ => vec![],
    })
}

#[tauri::command]
pub async fn lsp_hover(
    state: State<'_, LspState>,
    language: String,
    path: String,
    line: u32,
    character: u32,
) -> Result<Option<Value>, String> {
    let server = state
        .servers
        .lock()
        .await
        .get(&language)
        .cloned()
        .ok_or_else(|| format!("LSP '{}' is not running", language))?;
    let result = send_request(
        &server,
        "textDocument/hover",
        json!({
            "textDocument": { "uri": path_to_uri(&path) },
            "position": { "line": line, "character": character }
        }),
        5_000,
    )
    .await?;
    Ok(if result.is_null() { None } else { Some(result) })
}

/// `textDocument/formatting` — returns the server's edits; the client applies
/// them to the buffer before writing, enabling format-on-save.
#[tauri::command]
pub async fn lsp_format(
    state: State<'_, LspState>,
    language: String,
    path: String,
    tab_size: u32,
) -> Result<Vec<Value>, String> {
    let server = state
        .servers
        .lock()
        .await
        .get(&language)
        .cloned()
        .ok_or_else(|| format!("LSP '{}' is not running", language))?;
    let result = send_request(
        &server,
        "textDocument/formatting",
        json!({
            "textDocument": { "uri": path_to_uri(&path) },
            "options": { "tabSize": tab_size, "insertSpaces": true }
        }),
        10_000,
    )
    .await?;
    Ok(match result {
        Value::Array(edits) => edits,
        Value::Null => vec![],
        _ => vec![],
    })
}
