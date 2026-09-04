use serde::Serialize;
use std::fs;
use std::io::Read;
use std::path::Path;

const MAX_READ_BYTES: u64 = 5 * 1024 * 1024; // refuse files > 5 MB (Monaco-safe)
const SKIP_DIRS: &[&str] = &[
    "node_modules",
    ".git",
    "target",
    "dist",
    "build",
    "out",
    "__pycache__",
    ".venv",
    "venv",
    ".next",
    "vendor",
];

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct FileEntry {
    pub name: String,
    pub path: String,
    pub is_dir: bool,
    pub size: u64,
    pub modified_ms: u64,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct FileContent {
    pub content: String,
    pub is_binary: bool,
    pub truncated: bool,
    pub size: u64,
}

fn entry_for(p: &Path) -> Option<FileEntry> {
    let meta = fs::metadata(p).ok()?;
    let name = p.file_name()?.to_string_lossy().to_string();
    let modified_ms = meta
        .modified()
        .ok()
        .and_then(|m| m.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);
    Some(FileEntry {
        name,
        path: p.to_string_lossy().to_string(),
        is_dir: meta.is_dir(),
        size: if meta.is_dir() { 0 } else { meta.len() },
        modified_ms,
    })
}

#[tauri::command]
pub fn list_dir(path: String) -> Result<Vec<FileEntry>, String> {
    let rd = fs::read_dir(&path).map_err(|e| format!("Cannot read directory '{}': {}", path, e))?;
    let mut entries: Vec<FileEntry> = Vec::new();
    for item in rd.flatten() {
        if let Some(e) = entry_for(&item.path()) {
            entries.push(e);
        }
    }
    // Folders first, then case-insensitive name order (VSCode behaviour)
    entries.sort_by(|a, b| {
        b.is_dir
            .cmp(&a.is_dir)
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });
    Ok(entries)
}

#[tauri::command]
pub fn read_file(path: String) -> Result<FileContent, String> {
    let p = Path::new(&path);
    let meta = fs::metadata(p).map_err(|e| format!("Cannot access '{}': {}", path, e))?;
    if meta.is_dir() {
        return Err(format!("'{}' is a directory", path));
    }
    let size = meta.len();
    let mut file = fs::File::open(p).map_err(|e| format!("Cannot open '{}': {}", path, e))?;
    let mut buf: Vec<u8> = Vec::new();
    let limit = size.min(MAX_READ_BYTES);
    (&mut file)
        .take(limit)
        .read_to_end(&mut buf)
        .map_err(|e| format!("Cannot read '{}': {}", path, e))?;
    let truncated = size > MAX_READ_BYTES;

    // Binary sniff: a NUL byte in the first 8 KB almost always means binary.
    let sniff_end = buf.len().min(8192);
    if buf[..sniff_end].contains(&0u8) {
        return Ok(FileContent {
            content: String::new(),
            is_binary: true,
            truncated,
            size,
        });
    }
    Ok(FileContent {
        content: String::from_utf8_lossy(&buf).to_string(),
        is_binary: false,
        truncated,
        size,
    })
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct BytesContent {
    pub data_b64: String,
    pub size: u64,
    pub truncated: bool,
}

const MAX_HEX_BYTES: u64 = 256 * 1024;

/// Raw byte read for the hex viewer: returns up to `limit` bytes (default
/// 256 KB) base64-encoded, plus the full file size so the UI can indicate
/// truncation. Never loads more than the cap into memory.
#[tauri::command]
pub fn read_file_bytes(path: String, limit: Option<u64>) -> Result<BytesContent, String> {
    let p = Path::new(&path);
    let meta = fs::metadata(p).map_err(|e| format!("Cannot access '{}': {}", path, e))?;
    if meta.is_dir() {
        return Err(format!("'{}' is a directory", path));
    }
    let size = meta.len();
    let cap = limit.unwrap_or(MAX_HEX_BYTES).min(MAX_HEX_BYTES).min(size);
    let mut file = fs::File::open(p).map_err(|e| format!("Cannot open '{}': {}", path, e))?;
    let mut buf: Vec<u8> = vec![0u8; cap as usize];
    file.read_exact(&mut buf)
        .map_err(|e| format!("Cannot read '{}': {}", path, e))?;

    use base64::Engine;
    Ok(BytesContent {
        data_b64: base64::engine::general_purpose::STANDARD.encode(&buf),
        size,
        truncated: size > cap,
    })
}

#[tauri::command]
pub fn write_file(path: String, content: String) -> Result<(), String> {
    let p = Path::new(&path);
    if let Some(parent) = p.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    fs::write(p, content).map_err(|e| format!("Cannot write '{}': {}", path, e))
}

#[tauri::command]
pub fn create_file(path: String) -> Result<(), String> {
    if Path::new(&path).exists() {
        return Err(format!("'{}' already exists", path));
    }
    fs::write(&path, b"").map_err(|e| format!("Cannot create '{}': {}", path, e))
}

#[tauri::command]
pub fn create_dir(path: String) -> Result<(), String> {
    if Path::new(&path).exists() {
        return Err(format!("'{}' already exists", path));
    }
    fs::create_dir_all(&path).map_err(|e| format!("Cannot create '{}': {}", path, e))
}

#[tauri::command]
pub fn rename_path(from: String, to: String) -> Result<(), String> {
    let target = Path::new(&to);
    if let Some(parent) = target.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    fs::rename(&from, target).map_err(|e| format!("Cannot rename '{}': {}", from, e))
}

#[tauri::command]
pub fn delete_path(path: String, recursive: bool) -> Result<(), String> {
    let p = Path::new(&path);
    if p.is_dir() {
        if recursive {
            fs::remove_dir_all(p).map_err(|e| e.to_string())
        } else {
            fs::remove_dir(p).map_err(|e| e.to_string())
        }
    } else {
        fs::remove_file(p).map_err(|e| e.to_string())
    }
}

#[tauri::command]
pub fn file_exists(path: String) -> bool {
    Path::new(&path).exists()
}

/// Full recursive file list for Quick Open (Ctrl+P).
/// Uses `ignore` — the walker from ripgrep — so .gitignore and hidden files are respected.
#[tauri::command]
pub fn list_all_files(root: String, limit: Option<u32>) -> Result<Vec<String>, String> {
    let max = limit.unwrap_or(20000) as usize;
    let walker = ignore::WalkBuilder::new(&root)
        .hidden(true)
        .git_ignore(true)
        .git_global(true)
        .git_exclude(true)
        .build();

    let root_path = Path::new(&root);
    let mut out: Vec<String> = Vec::with_capacity(2048);
    for entry in walker.flatten() {
        if out.len() >= max {
            break;
        }
        let p = entry.path();
        if p == root_path {
            continue;
        }
        // Extra safety net for un-ignored heavy directories
        let skip = p.components().any(|c| {
            let name = c.as_os_str().to_string_lossy();
            SKIP_DIRS.contains(&name.as_ref())
        });
        if skip {
            continue;
        }
        if entry.file_type().map(|t| t.is_file()).unwrap_or(false) {
            out.push(p.to_string_lossy().to_string());
        }
    }
    Ok(out)
}
