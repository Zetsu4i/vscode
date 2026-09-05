use serde::Serialize;
use std::process::Command;

fn run_git(root: &str, args: &[&str]) -> Result<String, String> {
    let out = Command::new("git")
        .args(args)
        .current_dir(root)
        .env("GIT_OPTIONAL_LOCKS", "0")
        .output()
        .map_err(|e| format!("Cannot run git: {}", e))?;
    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr).trim().to_string();
        return Err(if err.is_empty() {
            format!("git {} failed", args.join(" "))
        } else {
            err
        });
    }
    Ok(String::from_utf8_lossy(&out.stdout).to_string())
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct GitChange {
    /// X (staged) column from porcelain, e.g. 'M', 'A', 'D', 'R', '?'
    pub x: String,
    /// Y (unstaged) column
    pub y: String,
    pub path: String,
    pub orig_path: Option<String>,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct GitStatus {
    pub branch: Option<String>,
    pub changes: Vec<GitChange>,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct LogEntry {
    pub hash: String,
    pub subject: String,
}

#[tauri::command]
pub fn git_is_repo(root: String) -> bool {
    matches!(
        run_git(&root, &["rev-parse", "--is-inside-work-tree"]),
        Ok(s) if s.trim() == "true"
    )
}

#[tauri::command]
pub fn git_status(root: String) -> Result<GitStatus, String> {
    let out = run_git(
        &root,
        &[
            "status",
            "--porcelain=v1",
            "--branch",
            "--untracked-files=all",
        ],
    )?;
    let mut branch = None;
    let mut changes = Vec::new();
    for line in out.lines() {
        if let Some(rest) = line.strip_prefix("## ") {
            // e.g. "main...origin/main [ahead 1]" or "main" or "No commits yet on main"
            let name = rest.split("...").next().unwrap_or(rest);
            let name = name.strip_prefix("No commits yet on ").unwrap_or(name);
            branch = Some(name.trim().to_string());
            continue;
        }
        if line.len() < 4 {
            continue;
        }
        let (xy, rest) = line.split_at(2);
        let mut path = rest.trim_start().to_string();
        let mut orig_path = None;
        // renames: "XY old -> new"
        if let Some(idx) = path.find(" -> ") {
            let old = path[..idx].to_string();
            let newp = path[idx + 4..].to_string();
            orig_path = Some(old);
            path = newp;
        }
        let mut chars = xy.chars();
        let x = chars.next().unwrap_or(' ').to_string();
        let y = chars.next().unwrap_or(' ').to_string();
        changes.push(GitChange {
            x,
            y,
            path,
            orig_path,
        });
    }
    Ok(GitStatus { branch, changes })
}

#[tauri::command]
pub fn git_stage(root: String, paths: Vec<String>) -> Result<(), String> {
    if paths.is_empty() {
        return Ok(());
    }
    let mut args = vec!["add", "--"];
    args.extend(paths.iter().map(|s| s.as_str()));
    run_git(&root, &args).map(|_| ())
}

#[tauri::command]
pub fn git_unstage(root: String, paths: Vec<String>) -> Result<(), String> {
    if paths.is_empty() {
        return Ok(());
    }
    let mut args = vec!["reset", "-q", "HEAD", "--"];
    args.extend(paths.iter().map(|s| s.as_str()));
    if run_git(&root, &args).is_err() {
        // Initial commit (no HEAD yet): fall back to rm --cached
        let mut rm = vec!["rm", "--cached", "-q", "--"];
        rm.extend(paths.iter().map(|s| s.as_str()));
        run_git(&root, &rm).map(|_| ())?;
    }
    Ok(())
}

#[tauri::command]
pub fn git_commit(root: String, message: String) -> Result<String, String> {
    if message.trim().is_empty() {
        return Err("Please provide a commit message".to_string());
    }
    run_git(&root, &["commit", "-m", &message])
}

#[tauri::command]
pub fn git_branch(root: String) -> Option<String> {
    run_git(&root, &["rev-parse", "--abbrev-ref", "HEAD"])
        .ok()
        .map(|s| s.trim().to_string())
}

#[tauri::command]
pub fn git_log(root: String, limit: Option<u32>) -> Result<Vec<LogEntry>, String> {
    let n = limit.unwrap_or(50).to_string();
    let out = run_git(&root, &["log", "--oneline", &format!("--max-count={}", n)])?;
    Ok(out
        .lines()
        .filter_map(|l| {
            let mut parts = l.splitn(2, ' ');
            let hash = parts.next()?.to_string();
            let subject = parts.next().unwrap_or("").to_string();
            Some(LogEntry { hash, subject })
        })
        .collect())
}

/// Content of a file at HEAD (empty string when the file is new/untracked).
#[tauri::command]
pub fn git_show_head(root: String, path: String) -> Result<String, String> {
    let rel = path.replace('\\', "/");
    match run_git(&root, &["show", &format!("HEAD:{}", rel)]) {
        Ok(s) => Ok(s),
        Err(_) => Ok(String::new()),
    }
}

/// Unified diff text for one file (working tree vs index when `staged` is false).
#[tauri::command]
pub fn git_diff_file(root: String, path: String, staged: bool) -> Result<String, String> {
    let rel = path.replace('\\', "/");
    let mut args: Vec<&str> = vec!["diff", "--unified=3", "--no-color"];
    if staged {
        args.push("--cached");
    }
    args.push("--");
    args.push(&rel);
    run_git(&root, &args)
}
