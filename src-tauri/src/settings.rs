//! `settings.json` — user and workspace scopes, VSCode-compatible.
//!
//! - user scope:      `<app_config_dir>/settings.json`
//! - workspace scope: `<workspace>/.vscode/settings.json`
//!
//! Files are JSONC-tolerant on read (comments and trailing commas are
//! stripped before parsing); writes are validated the same way, so a
//! corrupt file can never be saved. The frontend stores settings with
//! dotted keys (`"editor.minimap.enabled": true`) exactly like VSCode's
//! UI does, and applies workspace-over-user merge semantics itself.

use serde::Serialize;
use std::fs;
use std::path::PathBuf;
use tauri::{AppHandle, Manager};

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SettingsFile {
    pub path: String,
    /// Sanitized, valid JSON (comments/trailing commas removed) so the
    /// frontend can `JSON.parse` directly.
    pub text: String,
    pub exists: bool,
}

fn user_settings_path(app: &AppHandle) -> Result<PathBuf, String> {
    Ok(app
        .path()
        .app_config_dir()
        .map_err(|e| e.to_string())?
        .join("settings.json"))
}

fn workspace_settings_path(root: &str) -> PathBuf {
    std::path::Path::new(root).join(".vscode").join("settings.json")
}

fn resolve_path(app: &AppHandle, scope: &str, root: Option<String>) -> Result<PathBuf, String> {
    match scope {
        "user" => user_settings_path(app),
        "workspace" => {
            let r = root.ok_or_else(|| "no workspace folder open".to_string())?;
            Ok(workspace_settings_path(&r))
        }
        other => Err(format!("unknown settings scope '{}'", other)),
    }
}

/// Strip `//` and `/* */` comments and trailing commas from JSONC while
/// preserving string contents (including escaped quotes).
pub(crate) fn strip_jsonc(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut chars = input.chars().peekable();
    let mut in_string = false;
    let mut escaped = false;

    while let Some(c) = chars.next() {
        if in_string {
            out.push(c);
            if escaped {
                escaped = false;
            } else if c == '\\' {
                escaped = true;
            } else if c == '"' {
                in_string = false;
            }
            continue;
        }
        match c {
            '"' => {
                in_string = true;
                out.push(c);
            }
            '/' => match chars.peek() {
                Some('/') => {
                    while let Some(n) = chars.next() {
                        if n == '\n' {
                            out.push('\n');
                            break;
                        }
                    }
                }
                Some('*') => {
                    chars.next(); // consume '*'
                    while let Some(n) = chars.next() {
                        if n == '*' && chars.peek() == Some(&'/') {
                            chars.next();
                            break;
                        }
                    }
                    out.push(' '); // keep tokens separated
                }
                _ => out.push('/'),
            },
            // Trailing comma: drop it if the next non-whitespace char closes
            // the current object/array.
            ',' => {
                let mut ws = String::new();
                while let Some(&n) = chars.peek() {
                    if n.is_whitespace() {
                        ws.push(n);
                        chars.next();
                    } else {
                        break;
                    }
                }
                if chars.peek() != Some(&'}') && chars.peek() != Some(&']') {
                    out.push(',');
                }
                out.push_str(&ws);
            }
            _ => out.push(c),
        }
    }
    out
}

#[tauri::command]
pub fn settings_read(
    app: AppHandle,
    scope: String,
    root: Option<String>,
) -> Result<SettingsFile, String> {
    let path = resolve_path(&app, &scope, root)?;
    let exists = path.is_file();
    let text = if exists {
        let raw = fs::read_to_string(&path).map_err(|e| e.to_string())?;
        strip_jsonc(&raw)
    } else {
        "{}".to_string()
    };
    Ok(SettingsFile {
        path: path.to_string_lossy().to_string(),
        text,
        exists,
    })
}

#[tauri::command]
pub fn settings_write(
    app: AppHandle,
    scope: String,
    root: Option<String>,
    text: String,
) -> Result<(), String> {
    let path = resolve_path(&app, &scope, root)?;
    // Validate after stripping JSONC: comments are tolerated, corruption is not.
    let stripped = strip_jsonc(&text);
    serde_json::from_str::<serde_json::Value>(&stripped)
        .map_err(|e| format!("invalid settings JSON: {}", e))?;
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    fs::write(&path, &text).map_err(|e| e.to_string())
}

/// Returns the settings file path for a scope, creating an empty file when
/// missing (matches VSCode, which opens a fresh settings.json on demand).
#[tauri::command]
pub fn settings_path(
    app: AppHandle,
    scope: String,
    root: Option<String>,
) -> Result<String, String> {
    let path = resolve_path(&app, &scope, root)?;
    if !path.is_file() {
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        }
        fs::write(&path, "{\n}\n").map_err(|e| e.to_string())?;
    }
    Ok(path.to_string_lossy().to_string())
}
