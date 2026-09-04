//! System service: shell defaults, terminal profiles, process environment.

use serde_json::Value;

fn arg_opt_str(args: &[Value], i: usize) -> Option<String> {
    args.get(i).and_then(Value::as_str).map(str::to_string)
}

fn find_in_path(binary: &str) -> Option<std::path::PathBuf> {
    let path = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path) {
        let candidate = dir.join(binary);
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    None
}

pub fn default_shell(args: &[Value]) -> Result<Value, String> {
    let os = arg_opt_str(args, 0);
    let shell = if os.as_deref() == Some("Windows") || cfg!(target_os = "windows") {
        let ps = r"C:\Windows\System32\WindowsPowerShell\v1.0\powershell.exe";
        if std::path::Path::new(ps).exists() {
            ps.to_string()
        } else {
            std::env::var("COMSPEC").unwrap_or_else(|_| "cmd.exe".to_string())
        }
    } else if os.as_deref() == Some("MacOS") {
        std::env::var("SHELL").unwrap_or_else(|_| "/bin/zsh".to_string())
    } else {
        std::env::var("SHELL").unwrap_or_else(|_| "/bin/bash".to_string())
    };
    Ok(Value::String(shell))
}

/// Terminal profiles mirroring VSCode's auto-detected ones.
pub fn terminal_profiles(_args: &[Value]) -> Result<Value, String> {
    let mut profiles: Vec<Value> = Vec::new();

    let push =
        |name: &str, path: &str, args: Vec<&str>, is_default: bool, profiles: &mut Vec<Value>| {
            let mut p =
                serde_json::json!({ "profileName": name, "path": path, "isDefault": is_default });
            if !args.is_empty() {
                p["args"] = Value::Array(
                    args.into_iter()
                        .map(|a| Value::String(a.to_string()))
                        .collect(),
                );
            }
            profiles.push(p);
        };

    if cfg!(target_os = "windows") {
        let ps = r"C:\Windows\System32\WindowsPowerShell\v1.0\powershell.exe";
        if std::path::Path::new(ps).exists() {
            push("PowerShell", ps, vec!["-NoLogo"], true, &mut profiles);
        }
        if let Some(pwsh) = find_in_path("pwsh.exe") {
            push(
                "PowerShell (pwsh)",
                &pwsh.to_string_lossy(),
                vec!["-NoLogo"],
                false,
                &mut profiles,
            );
        }
        if let Ok(comspec) = std::env::var("COMSPEC") {
            push("Command Prompt", &comspec, vec!["/K"], false, &mut profiles);
        }
        for git_bash in [
            r"C:\Program Files\Git\bin\bash.exe",
            r"C:\Program Files (x86)\Git\bin\bash.exe",
        ] {
            if std::path::Path::new(git_bash).exists() {
                push("Git Bash", git_bash, vec!["-l"], false, &mut profiles);
                break;
            }
        }
    } else {
        for (name, bin) in [("bash", "bash"), ("zsh", "zsh"), ("fish", "fish")] {
            if let Some(p) = find_in_path(bin) {
                push(
                    name,
                    &p.to_string_lossy(),
                    vec![],
                    name == "bash",
                    &mut profiles,
                );
            }
        }
        if profiles.is_empty() {
            push("bash", "/bin/bash", vec![], true, &mut profiles);
        }
    }

    Ok(Value::Array(profiles))
}

pub fn env() -> Result<Value, String> {
    let map: serde_json::Map<String, Value> = std::env::vars()
        .map(|(k, v)| (k, Value::String(v)))
        .collect();
    Ok(Value::Object(map))
}
