use notify::{RecursiveMode, Watcher};
use notify_debouncer_full::{new_debouncer, DebounceEventResult, Debouncer, FileIdMap};
use std::path::Path;
use std::sync::Mutex;
use std::time::Duration;
use tauri::{AppHandle, Emitter, State};

pub struct WatcherState {
    inner: Mutex<Option<Debouncer<notify::RecommendedWatcher, FileIdMap>>>,
}

impl Default for WatcherState {
    fn default() -> Self {
        Self {
            inner: Mutex::new(None),
        }
    }
}

/// Watch the workspace recursively (replaces any previous watch).
/// Changes are debounced (300 ms) and emitted as `fs-changed` with affected paths.
#[tauri::command]
pub fn watch_folder(
    app: AppHandle,
    state: State<'_, WatcherState>,
    root: String,
) -> Result<(), String> {
    let app_handle = app.clone();
    let mut debouncer = new_debouncer(
        Duration::from_millis(300),
        None,
        move |result: DebounceEventResult| {
            if let Ok(events) = result {
                let paths: Vec<String> = events
                    .iter()
                    .flat_map(|e| e.event.paths.iter())
                    .map(|p| p.to_string_lossy().to_string())
                    .collect();
                if !paths.is_empty() {
                    let _ = app_handle.emit("fs-changed", serde_json::json!({ "paths": paths }));
                }
            }
        },
    )
    .map_err(|e| e.to_string())?;

    debouncer
        .watcher()
        .watch(Path::new(&root), RecursiveMode::Recursive)
        .map_err(|e| format!("Cannot watch '{}': {}", root, e))?;

    *state.inner.lock().map_err(|_| "watcher lock poisoned")? = Some(debouncer);
    Ok(())
}
