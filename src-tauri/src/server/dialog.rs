//! Native file dialogs via rfd. Runs on a blocking thread (rfd blocks until
//! the dialog is dismissed) and never opens a console window.

use serde_json::Value;
use std::path::Path;

fn arg_str(args: &[Value], i: usize) -> Result<String, String> {
    args.get(i)
        .and_then(Value::as_str)
        .map(str::to_string)
        .ok_or_else(|| format!("missing string arg #{i}"))
}

/// `dialog.pick(mode, defaultPath)` -> absolute path or null when cancelled.
pub async fn pick(args: &[Value]) -> Result<Value, String> {
    let mode = arg_str(args, 0)?;
    let default = args.get(1).and_then(Value::as_str).map(str::to_string);

    tokio::task::spawn_blocking(move || {
        let mut dialog = rfd::FileDialog::new();
        if let Some(dir) = default {
            // for file picks, start in the parent folder
            let start = if mode == "file" {
                Path::new(&dir)
                    .parent()
                    .map(|p| p.to_path_buf())
                    .unwrap_or_default()
            } else {
                std::path::PathBuf::from(&dir)
            };
            if start.is_dir() {
                dialog = dialog.set_directory(start);
            }
        }

        let picked = if mode == "folder" {
            dialog.pick_folder()
        } else {
            dialog.pick_file()
        };

        Ok(match picked {
            Some(p) => Value::String(p.to_string_lossy().to_string()),
            None => Value::Null,
        })
    })
    .await
    .map_err(|e| e.to_string())?
}
