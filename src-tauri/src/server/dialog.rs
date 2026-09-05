//! Native file dialogs via rfd. Runs on a blocking thread (rfd blocks until
//! the dialog is dismissed) and never opens a console window.
//!
//! One bridge method covers every dialog shape the workbench needs:
//! `dialog.pick(mode, defaultPath?, title?, filters?)` with
//! mode = `folder` | `file` | `files` | `workspace` | `save`.
//! Returns a string (single selection), an array of strings (`files`) or
//! null when the user cancels.

use serde_json::Value;
use std::path::{Path, PathBuf};

fn arg_str(args: &[Value], i: usize) -> Result<String, String> {
    args.get(i)
        .and_then(Value::as_str)
        .map(str::to_string)
        .ok_or_else(|| format!("missing string arg #{i}"))
}

type Filters = Vec<(String, Vec<String>)>;

fn parse_filters(args: &[Value], i: usize) -> Filters {
    args.get(i)
        .and_then(Value::as_array)
        .map(|arr| {
            arr.iter()
                .filter_map(|f| {
                    let name = f.get(0)?.as_str()?.to_string();
                    let exts: Vec<String> = f
                        .get(1)?
                        .as_array()?
                        .iter()
                        .filter_map(|e| e.as_str().map(str::to_string))
                        .collect();
                    Some((name, exts))
                })
                .collect()
        })
        .unwrap_or_default()
}

fn apply_common(
    mut dialog: rfd::FileDialog,
    title: Option<&str>,
    filters: &Filters,
    default: Option<&str>,
) -> rfd::FileDialog {
    if let Some(t) = title {
        dialog = dialog.set_title(t);
    }
    for (name, exts) in filters {
        let refs: Vec<&str> = exts.iter().map(String::as_str).collect();
        dialog = dialog.add_filter(name, &refs);
    }
    if let Some(d) = default {
        let parent = Path::new(d).parent().map(Path::to_path_buf);
        if let Some(p) = parent {
            if p.is_dir() {
                dialog = dialog.set_directory(p);
            }
        }
    }
    dialog
}

fn path_value(p: PathBuf) -> Value {
    Value::String(p.to_string_lossy().to_string())
}

/// `dialog.pick(mode, defaultPath?, title?, filters?)` -> path | paths | null.
pub async fn pick(args: &[Value]) -> Result<Value, String> {
    let mode = arg_str(args, 0)?;
    let default = args.get(1).and_then(Value::as_str).map(str::to_string);
    let title = args.get(2).and_then(Value::as_str).map(str::to_string);
    let filters = parse_filters(args, 3);

    tokio::task::spawn_blocking(move || match mode.as_str() {
        "save" => {
            let mut dialog = apply_common(
                rfd::FileDialog::new(),
                title.as_deref(),
                &filters,
                default.as_deref(),
            );
            if let Some(d) = &default {
                if let Some(name) = Path::new(d).file_name() {
                    dialog = dialog.set_file_name(&*name.to_string_lossy());
                }
            }
            Ok(dialog.save_file().map(path_value).unwrap_or(Value::Null))
        }
        "folder" => {
            let mut dialog = apply_common(
                rfd::FileDialog::new(),
                title.as_deref(),
                &filters,
                default.as_deref(),
            );
            if let Some(d) = &default {
                let start = PathBuf::from(d);
                if start.is_dir() {
                    dialog = dialog.set_directory(start);
                }
            }
            Ok(dialog.pick_folder().map(path_value).unwrap_or(Value::Null))
        }
        "file" => {
            let dialog = apply_common(
                rfd::FileDialog::new(),
                title.as_deref(),
                &filters,
                default.as_deref(),
            );
            Ok(dialog.pick_file().map(path_value).unwrap_or(Value::Null))
        }
        "files" => {
            let dialog = apply_common(
                rfd::FileDialog::new(),
                title.as_deref(),
                &filters,
                default.as_deref(),
            );
            Ok(dialog
                .pick_files()
                .map(|paths| Value::Array(paths.into_iter().map(path_value).collect()))
                .unwrap_or(Value::Null))
        }
        "workspace" => {
            let mut filters = filters;
            if filters.is_empty() {
                filters.push(("Workspace".to_string(), vec!["code-workspace".to_string()]));
            }
            let dialog = apply_common(
                rfd::FileDialog::new(),
                title.as_deref(),
                &filters,
                default.as_deref(),
            );
            Ok(dialog.pick_file().map(path_value).unwrap_or(Value::Null))
        }
        other => Err(format!("unknown dialog mode: {other}")),
    })
    .await
    .map_err(|e| e.to_string())?
}
