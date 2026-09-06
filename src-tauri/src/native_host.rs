//! Mountain: `nativeHost` protocol channel.
//!
//! `ProxyChannel.fromService(INativeHostMainService)` in electron-main
//! (app.ts) exposes the full `INativeHostService` method surface
//! (src/vs/platform/native/common/native.ts) — every method becomes a
//! channel command, and the renderer's `NativeHostService` proxy passes
//! `[windowId, ...args]` (the windowId is the ProxyChannel context, not a
//! method argument).
//!
//! This module answers the subset the workbench needs during boot and
//! early usage, growing command by command (the unimplemented tail still
//! rejects like Electron's "Method not found" and lands in the contract
//! log for the next round).

use serde_json::{json, Value};
use tauri::Manager;

pub fn handle(app: Option<&tauri::AppHandle>, command: &str, arg: &Value) -> Result<Value, String> {
    // ProxyChannel arguments: [context(windowId), ...methodArgs]
    let args = arg.as_array().cloned().unwrap_or_default();
    let _window_id = args.first().and_then(Value::as_i64).unwrap_or(1);
    let _options = args.get(1);

    match command {
        // ---- identity / environment ----
        "windowId" => Ok(json!(1)),
        "getOS" => Ok(json!("Windows")),
        "getOSRelease" => Ok(json!(os_release())),
        "getOSVersion" => Ok(json!(os_version())),
        "getCacheHome" => Ok(crate::config::cache_home_uri()),
        "getOSProperties" => Ok(json!({
            "platform": "Windows",
            "release": os_release(),
            "arch": std::env::consts::ARCH,
        })),
        "getOSStatistics" => Ok(json!({
            "totalMemory": total_memory_mb(),
            "freememory": free_memory_mb(),
        })),
        "getOSVirtualMachineHint" => Ok(json!(0)),
        "getOSColorScheme" => Ok(crate::config::color_scheme()),
        "hostname" => Ok(json!(hostname())),
        "hasWSLFeatureInstalled" => Ok(json!(false)),
        "isAdmin" => Ok(json!(false)),
        "isRunningUnderARM64Translation" => Ok(json!(false)),
        "getMediaAccessStatus" => Ok(json!("unknown")),
        "getProcessMemoryInfo" => Ok(json!({ "private": 0, "residentSet": 0, "shared": 0 })),
        "getProcessId" => Ok(json!(std::process::id() as i64)),

        // ---- window state ----
        "isFocused" => Ok(json!(window_state(app, |w| w.is_focused()))),
        "isMaximized" => Ok(json!(window_state(app, |w| w.is_maximized()))),
        "isFullScreen" => Ok(json!(window_state(app, |w| w.is_fullscreen()))),
        "isWindowAlwaysOnTop" => Ok(json!(window_state(app, |w| w.is_always_on_top()))),
        "getWindows" => Ok(json!([opened_main_window()])),
        "getWindowCount" => Ok(json!(1)),
        "getActiveWindowId" => Ok(json!(1)),
        "getCursorScreenPoint" => Ok(cursor_screen_point(app)),

        // ---- window operations ----
        "focusWindow" => with_window(app, |w| {
            let _ = w.set_focus();
        }),
        "minimizeWindow" | "minimize" => with_window(app, |w| {
            let _ = w.minimize();
        }),
        "maximizeWindow" | "maximize" => with_window(app, |w| {
            let _ = w.maximize();
        }),
        "unmaximizeWindow" => with_window(app, |w| {
            let _ = w.unmaximize();
        }),
        "toggleWindowFullScreen" => {
            let fullscreen = window_state(app, |w| w.is_fullscreen());
            with_window(app, move |w| {
                let _ = w.set_fullscreen(!fullscreen);
            })
        }
        "closeWindow" => with_window(app, |w| {
            let _ = w.close();
        }),
        "moveWindowTop" => with_window(app, |w| {
            let _ = w.set_focus();
        }),
        "setWindowAlwaysOnTop" => {
            let always = args.get(1).and_then(Value::as_bool).unwrap_or(false);
            with_window(app, move |w| {
                let _ = w.set_always_on_top(always);
            })
        }
        "toggleWindowAlwaysOnTop" => {
            let current = window_state(app, |w| w.is_always_on_top());
            with_window(app, move |w| {
                let _ = w.set_always_on_top(!current);
            })
        }
        "setMinimumSize" => {
            let width = args.get(1).and_then(Value::as_i64);
            let height = args.get(2).and_then(Value::as_i64);
            if let (Some(width), Some(height)) = (width, height) {
                if width > 0 && height > 0 {
                    with_window(app, move |w| {
                        let _ = w.set_min_size(Some(tauri::LogicalSize::new(
                            width as f64,
                            height as f64,
                        )));
                    })?;
                }
            }
            Ok(Value::Null)
        }

        // ---- app lifecycle ----
        "notifyReady" => {
            crate::logger::log_app("info", "nativeHost: renderer notified ready");
            Ok(Value::Null)
        }
        "relaunch" => {
            crate::logger::log_app("info", "nativeHost: relaunch requested (restart via new process spawn + exit)");
            relaunch_app();
            Ok(Value::Null)
        }
        "reload" => {
            if let Some(app) = app {
                if let Some(window) = app.get_webview_window("main") {
                    let _ = window.eval("window.location.reload()");
                }
            }
            Ok(Value::Null)
        }
        "quit" => {
            crate::logger::log_app("info", "nativeHost: quit requested");
            if let Some(app) = app {
                app.exit(0);
            }
            Ok(Value::Null)
        }
        "exit" => {
            let code = args.first().and_then(Value::as_i64).unwrap_or(0);
            if let Some(app) = app {
                app.exit(i32::try_from(code).unwrap_or(0));
            }
            Ok(Value::Null)
        }
        "killProcess" => {
            let pid = args.first().and_then(Value::as_i64).unwrap_or(-1);
            if pid > 0 {
                kill_process(pid);
            }
            Ok(Value::Null)
        }

        // ---- devtools ----
        "openDevTools" => {
            if let Some(app) = app {
                if let Some(window) = app.get_webview_window("main") {
                    window.open_devtools();
                }
            }
            Ok(Value::Null)
        }
        "toggleDevTools" => {
            if let Some(app) = app {
                if let Some(window) = app.get_webview_window("main") {
                    if window.is_devtools_open() {
                        window.close_devtools();
                    } else {
                        window.open_devtools();
                    }
                }
            }
            Ok(Value::Null)
        }

        // ---- misc (parity stubs with benign values) ----
        // The splash is persisted by the main process so the NEXT window can
        // paint it instantly. Phase 3 will persist it to the data dir.
        "saveWindowSplash" => Ok(Value::Null),
        "setRepresentedFilename" | "setDocumentEdited" | "setApplicationBadge"
        | "setBackgroundThrottling" | "updateWindowControls" | "updateWindowAccentColor"
        | "newWindowTab" | "showPreviousWindowTab" | "showNextWindowTab"
        | "moveWindowTabToNewWindow" | "mergeAllWindowTabs" | "toggleWindowTabsBar"
        | "updateTouchBar" | "installShellCommand" | "uninstallShellCommand"
        | "openGPUInfoWindow" | "openContentTracingWindow" | "stopTracing"
        | "openDevToolsWindow" | "triggerPaste" | "syncSystemWideKeybindings" => {
            Ok(Value::Null)
        }

        other => Err(format!(
            "nativeHost channel: method not found: {} (see compat/ipc-contract.md for the full surface)",
            other
        )),
    }
}

// ---------------------------------------------------------------------------
// helpers
// ---------------------------------------------------------------------------

fn with_window(
    app: Option<&tauri::AppHandle>,
    op: impl FnOnce(&tauri::WebviewWindow),
) -> Result<Value, String> {
    if let Some(app) = app {
        if let Some(window) = app.get_webview_window("main") {
            op(&window);
        }
    }
    Ok(Value::Null)
}

fn window_state(app: Option<&tauri::AppHandle>, read: impl FnOnce(&tauri::WebviewWindow) -> tauri::Result<bool>) -> bool {
    if let Some(app) = app {
        if let Some(window) = app.get_webview_window("main") {
            return read(&window).unwrap_or(false);
        }
    }
    false
}

fn opened_main_window() -> Value {
    json!({
        "id": 1,
        "workspace": null,
        "folderUri": null,
        "remoteAuthority": null,
        "title": "Visual Studio Code",
        "lastFocusTime": crate::util::unix_timestamp().parse::<f64>().unwrap_or(0.0),
        "openedViaUrl": false,
    })
}

fn cursor_screen_point(app: Option<&tauri::AppHandle>) -> Value {
    // Monitor layout is queried through the Tauri window when available.
    if let Some(app) = app {
        if let Some(window) = app.get_webview_window("main") {
            if let (Ok(mouse), Ok(position), Ok(size)) =
                (window.cursor_position(), window.outer_position(), window.inner_size())
            {
                let scale = window.scale_factor().unwrap_or(1.0);
                let display = json!({
                    "x": position.x,
                    "y": position.y,
                    "width": size.width,
                    "height": size.height,
                });
                return json!({
                    "point": { "x": mouse.x / scale, "y": mouse.y / scale },
                    "display": display,
                });
            }
        }
    }
    json!({
        "point": { "x": 0.0, "y": 0.0 },
        "display": { "x": 0, "y": 0, "width": 1280, "height": 800 },
    })
}

fn relaunch_app() {
    if let Ok(exe) = std::env::current_exe() {
        let mut command = std::process::Command::new(exe);
        command
            .env("VSTAURI_RELAXED_SPAWN", "1")
            .spawn()
            .map(|mut child| {
                // Detach: the parent exits right after; the child outlives it.
                let _ = child.wait();
            })
            .map_err(|err| {
                crate::logger::log_app("error", &format!("relaunch spawn failed: {}", err));
                err
            })
            .ok();
        std::process::exit(0);
    }
}

fn kill_process(pid: i64) {
    // Windows: taskkill /T /F /PID. Unix fallback: kill -9.
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        let _ = std::process::Command::new("taskkill")
            .args(["/T", "/F", "/PID"])
            .arg(pid.to_string())
            .creation_flags(0x0800_0000) // CREATE_NO_WINDOW
            .spawn();
    }
    #[cfg(not(windows))]
    {
        let _ = std::process::Command::new("kill")
            .arg("-9")
            .arg(pid.to_string())
            .spawn();
    }
}

fn os_release() -> String {
    "10.0.0".to_string()
}

fn os_version() -> String {
    "Windows".to_string()
}

fn hostname() -> String {
    std::env::var("COMPUTERNAME")
        .or_else(|_| std::env::var("HOSTNAME"))
        .unwrap_or_else(|_| "localhost".to_string())
}

fn total_memory_mb() -> i64 {
    #[cfg(windows)]
    {
        // MEMORYSTATUSEX via the windows crate would be exact; sysinfo-free
        // approximation: the pagefile-backed commit limit reported by
        // GlobalMemoryStatusEx. Until the windows-sys dependency lands,
        // report a static sane value (the value feeds the "memory pressure"
        // hints only).
        8_192
    }
    #[cfg(not(windows))]
    {
        if let Ok(meminfo) = std::fs::read_to_string("/proc/meminfo") {
            for line in meminfo.lines() {
                if let Some(rest) = line.strip_prefix("MemTotal:") {
                    if let Some(kb) = rest.trim().trim_end_matches(" kB").parse::<i64>().ok() {
                        return kb / 1_024;
                    }
                }
            }
        }
        8_192
    }
}

fn free_memory_mb() -> i64 {
    #[cfg(not(windows))]
    {
        if let Ok(meminfo) = std::fs::read_to_string("/proc/meminfo") {
            for line in meminfo.lines() {
                if let Some(rest) = line.strip_prefix("MemAvailable:") {
                    if let Some(kb) = rest.trim().trim_end_matches(" kB").parse::<i64>().ok() {
                        return kb / 1_024;
                    }
                }
            }
        }
    }
    4_096
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unknown_methods_reject_like_electron() {
        assert!(handle(None, "definitelyNotAMethod", &json!([1])).is_err());
    }

    #[test]
    fn known_stubs_resolve() {
        assert_eq!(handle(None, "getOS", &json!([1])).unwrap(), json!("Windows"));
        assert_eq!(handle(None, "getWindowCount", &json!([1])).unwrap(), json!(1));
        assert!(handle(None, "notifyReady", &json!([1])).is_ok());
        assert_eq!(handle(None, "getOSVirtualMachineHint", &json!([1])).unwrap(), json!(0));
    }

    #[test]
    fn cursor_point_has_display_fallback() {
        let point = cursor_screen_point(None);
        assert!(point.get("point").is_some());
        assert!(point.get("display").is_some());
    }
}
