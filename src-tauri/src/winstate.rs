//! Window-state persistence.
//!
//! - Restored in `setup` while the window is still hidden (`visible: false`
//!   in tauri.conf.json), so the app reopens exactly where it was left and
//!   the first frame the user sees is already the workbench — no white
//!   flash, no geometry jump.
//! - Saved authoritatively on `CloseRequested`, plus debounced saves from
//!   the frontend on resize/move for crash resilience.
//! - Geometry is stored in logical units so it survives DPI changes;
//!   `position_plausible` rejects absurd/off-screen coordinates.

use serde::{Deserialize, Serialize};
use std::fs;
use tauri::{AppHandle, Manager, Runtime, Window};

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct WindowState {
    pub width: f64,
    pub height: f64,
    pub x: f64,
    pub y: f64,
    pub maximized: bool,
}

fn state_path<R: Runtime>(app: &AppHandle<R>) -> Option<std::path::PathBuf> {
    Some(app.path().app_config_dir().ok()?.join("window-state.json"))
}

/// Restore the saved geometry at startup (window still hidden).
pub fn restore<R: Runtime>(app: &AppHandle<R>) {
    let Some(window) = app.get_webview_window("main") else {
        return;
    };
    let Some(path) = state_path(app) else {
        return;
    };
    let Ok(raw) = fs::read_to_string(&path) else {
        return;
    };
    let Ok(st) = serde_json::from_str::<WindowState>(&raw) else {
        return;
    };

    if st.width.is_finite() && st.height.is_finite() && st.width >= 400.0 && st.height >= 300.0 {
        let _ = window.set_size(tauri::LogicalSize::new(st.width, st.height));
    }
    if position_plausible(&st) {
        let _ = window.set_position(tauri::LogicalPosition::new(st.x, st.y));
    }
    if st.maximized {
        let _ = window.maximize();
    }
}

fn position_plausible(st: &WindowState) -> bool {
    st.x.is_finite()
        && st.y.is_finite()
        && st.x > -4000.0
        && st.y > -4000.0
        && st.x < 30000.0
        && st.y < 30000.0
}

/// Capture the given window's current geometry to disk (logical units).
pub fn save_now<R: Runtime>(window: &Window<R>) -> Result<(), String> {
    let scale = window.scale_factor().map_err(|e| e.to_string())?;
    if !scale.is_finite() || scale <= 0.0 {
        return Err("bad scale factor".into());
    }
    let size = window.outer_size().map_err(|e| e.to_string())?;
    let pos = window.outer_position().map_err(|e| e.to_string())?;
    let maximized = window.is_maximized().unwrap_or(false);

    let st = WindowState {
        width: size.width as f64 / scale,
        height: size.height as f64 / scale,
        x: pos.x as f64 / scale,
        y: pos.y as f64 / scale,
        maximized,
    };
    write_state(window.app_handle(), &st)
}

fn write_state<R: Runtime>(app: &AppHandle<R>, st: &WindowState) -> Result<(), String> {
    let Some(path) = state_path(app) else {
        return Err("no config dir".into());
    };
    if let Some(dir) = path.parent() {
        let _ = fs::create_dir_all(dir);
    }
    let json = serde_json::to_string(st).map_err(|e| e.to_string())?;
    fs::write(&path, json).map_err(|e| e.to_string())
}

/// Debounced saves from the frontend while the app runs. The frontend skips
/// saves while maximized, so the stored geometry is always the last *normal*
/// (restorable) geometry; `maximized: true` here is only meaningful for the
/// authoritative close-time save.
#[tauri::command]
pub fn save_window_state(app: AppHandle, state: WindowState) -> Result<(), String> {
    if state.maximized {
        return Ok(());
    }
    write_state(&app, &state)
}

/// Called by the frontend after the first painted frame. Shows and focuses
/// the window (restore-and-show; kills the startup white flash).
#[tauri::command]
pub fn window_ready(app: AppHandle) -> Result<(), String> {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.show();
        let _ = w.set_focus();
    }
    Ok(())
}
