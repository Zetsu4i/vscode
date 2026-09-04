use ignore::WalkBuilder;
use regex::Regex;
use serde::Serialize;
use std::fs;
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use tauri::{AppHandle, Emitter};

const MAX_HITS: usize = 5000;
const MAX_FILES: usize = 20000;
const MAX_FILE_BYTES: u64 = 5 * 1024 * 1024;
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
pub struct SearchHit {
    pub file: String,
    pub line_number: u64,
    pub text: String,
    /// (start_byte, end_byte) of each match within `text`
    pub ranges: Vec<(u32, u32)>,
}

/// Workspace-wide search running on ripgrep's engine (`regex`) and walker (`ignore`).
/// Runs in the background; streams `search-progress` and finishes with `search-done`.
#[tauri::command]
pub async fn search_workspace(
    app: AppHandle,
    root: String,
    query: String,
    is_regex: bool,
    case_sensitive: bool,
    whole_word: bool,
) -> Result<(), String> {
    if query.is_empty() {
        return Err("Empty search query".to_string());
    }

    let re = build_regex(&query, is_regex, case_sensitive, whole_word)?;

    tauri::async_runtime::spawn(async move {
        let re = Arc::new(re);
        let hits: Arc<std::sync::Mutex<Vec<SearchHit>>> =
            Arc::new(std::sync::Mutex::new(Vec::new()));
        let scanned = Arc::new(AtomicUsize::new(0));
        let truncated = Arc::new(AtomicUsize::new(0));

        let walker = WalkBuilder::new(&root)
            .hidden(true)
            .git_ignore(true)
            .git_global(true)
            .git_exclude(true)
            .build();

        for entry in walker.flatten() {
            if hits.lock().unwrap().len() >= MAX_HITS
                || scanned.load(Ordering::Relaxed) >= MAX_FILES
            {
                truncated.store(1, Ordering::Relaxed);
                break;
            }
            let p = entry.path();
            if !entry.file_type().map(|t| t.is_file()).unwrap_or(false) {
                continue;
            }
            if p.components()
                .any(|c| SKIP_DIRS.contains(&c.as_os_str().to_string_lossy().as_ref()))
            {
                continue;
            }
            let meta = match fs::metadata(p) {
                Ok(m) => m,
                Err(_) => continue,
            };
            if meta.len() > MAX_FILE_BYTES {
                continue;
            }
            let bytes = match fs::read(p) {
                Ok(b) => b,
                Err(_) => continue,
            };
            // binary sniff
            let sniff_end = bytes.len().min(8192);
            if bytes[..sniff_end].contains(&0u8) {
                continue;
            }
            scanned.fetch_add(1, Ordering::Relaxed);

            let text = String::from_utf8_lossy(&bytes);
            let file_path = p.to_string_lossy().to_string();
            let mut file_hits: Vec<SearchHit> = Vec::new();
            for (i, line) in text.lines().enumerate() {
                let ranges: Vec<(u32, u32)> = re
                    .find_iter(line)
                    .map(|m| (m.start() as u32, m.end() as u32))
                    .collect();
                if !ranges.is_empty() {
                    file_hits.push(SearchHit {
                        file: file_path.clone(),
                        line_number: (i + 1) as u64,
                        text: line.to_string(),
                        ranges,
                    });
                    if hits.lock().unwrap().len() + file_hits.len() >= MAX_HITS {
                        truncated.store(1, Ordering::Relaxed);
                        break;
                    }
                }
            }
            if !file_hits.is_empty() {
                hits.lock().unwrap().extend(file_hits);
            }

            let n = scanned.load(Ordering::Relaxed);
            if n % 200 == 0 {
                let _ = app.emit(
                    "search-progress",
                    serde_json::json!({
                        "filesScanned": n,
                        "hits": hits.lock().unwrap().len(),
                    }),
                );
            }
        }

        let _ = app.emit(
            "search-done",
            serde_json::json!({
                "hits": hits.lock().unwrap().clone(),
                "filesScanned": scanned.load(Ordering::Relaxed),
                "truncated": truncated.load(Ordering::Relaxed) == 1,
            }),
        );
    });

    Ok(())
}

/// Shared flag translation for search and replace: literal vs regex,
/// whole-word boundaries, case-insensitive prefix.
fn build_regex(query: &str, is_regex: bool, case_sensitive: bool, whole_word: bool) -> Result<Regex, String> {
    let pattern = if is_regex {
        query.to_string()
    } else {
        regex::escape(query)
    };
    let pattern = if whole_word {
        format!(r"\b(?:{})\b", pattern)
    } else {
        pattern
    };
    let pattern = if case_sensitive {
        pattern
    } else {
        format!("(?i){}", pattern)
    };
    Regex::new(&pattern).map_err(|e| format!("Invalid pattern: {}", e))
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ReplaceResult {
    pub files_changed: Vec<String>,
    pub total_replacements: usize,
}

/// Replace every match of the query across the workspace (same walker and
/// flags as search_workspace). Files are written back only when their content
/// actually changed; binary files and non-UTF-8 files are skipped untouched.
/// The replacement supports `$1`-style capture group references.
#[tauri::command]
pub async fn replace_all(
    root: String,
    query: String,
    replacement: String,
    is_regex: bool,
    case_sensitive: bool,
    whole_word: bool,
) -> Result<ReplaceResult, String> {
    if query.is_empty() {
        return Err("Empty search query".to_string());
    }
    let re = build_regex(&query, is_regex, case_sensitive, whole_word)?;

    let result = tauri::async_runtime::spawn_blocking(move || {
        let mut files_changed: Vec<String> = Vec::new();
        let mut total: usize = 0;

        let walker = WalkBuilder::new(&root)
            .hidden(true)
            .git_ignore(true)
            .git_global(true)
            .git_exclude(true)
            .build();

        for entry in walker.flatten() {
            let p = entry.path();
            if !entry.file_type().map(|t| t.is_file()).unwrap_or(false) {
                continue;
            }
            if p.components()
                .any(|c| SKIP_DIRS.contains(&c.as_os_str().to_string_lossy().as_ref()))
            {
                continue;
            }
            let meta = match fs::metadata(p) {
                Ok(m) => m,
                Err(_) => continue,
            };
            if meta.len() > MAX_FILE_BYTES {
                continue;
            }
            // Only touch valid UTF-8 text so we never corrupt binaries.
            let text = match fs::read(p).and_then(|b| String::from_utf8(b).map_err(|_| ())) {
                Ok(t) => t,
                Err(_) => continue,
            };

            let count = re.find_iter(&text).count();
            if count == 0 {
                continue;
            }
            let replaced = re.replace_all(&text, replacement.as_str()).to_string();
            if replaced == text {
                continue;
            }
            if fs::write(p, replaced).is_ok() {
                total += count;
                files_changed.push(p.to_string_lossy().to_string());
            }
        }

        ReplaceResult {
            files_changed,
            total_replacements: total,
        }
    })
    .await
    .map_err(|e| format!("replace failed: {}", e))?;

    Ok(result)
}

/// Compute the workspace-relative display path (helper mirrored on the frontend too).
#[allow(dead_code)]
fn relative_display(root: &str, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .to_string()
}
