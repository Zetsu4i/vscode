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

    let pattern = if is_regex {
        query.clone()
    } else {
        regex::escape(&query)
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
    let re = Regex::new(&pattern).map_err(|e| format!("Invalid pattern: {}", e))?;

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

/// Compute the workspace-relative display path (helper mirrored on the frontend too).
#[allow(dead_code)]
fn relative_display(root: &str, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .to_string()
}
