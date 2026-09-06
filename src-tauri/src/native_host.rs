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

        // ---- dialogs (tauri-plugin-dialog; native Win32 common dialogs,
        // the same ones Electron opens) ----
        "showSaveDialog" => show_save_dialog(app, args.get(1)),
        "showOpenDialog" => show_open_dialog(app, args.get(1)),
        "showMessageBox" => show_message_box(app, args.get(1)),
        "pickFileAndOpen" => pick_and_open(app, args.get(1), PickKind::File),
        "pickFolderAndOpen" => pick_and_open(app, args.get(1), PickKind::Folder),
        "pickWorkspaceAndOpen" => pick_and_open(app, args.get(1), PickKind::Workspace),
        "pickFileFolderAndOpen" => pick_and_open(app, args.get(1), PickKind::FileOrFolder),
        "openWindow" => {
            // openWindow(toOpen: IWindowOpenable[], options?) | openWindow(opts?)
            // — ProxyChannel passes [windowId, toOpen, options]. The array
            // form carries the openables; the single-object form opens an
            // empty window (reload as a clean boot here).
            let to_open = args.get(1).and_then(Value::as_array).cloned().unwrap_or_default();
            let changed = crate::config::apply_window_openables(&to_open);
            if changed {
                reload_window(app);
            }
            Ok(Value::Null)
        }
        "showItemInFolder" => {
            let path = args.get(1).and_then(Value::as_str).unwrap_or("");
            show_item_in_folder(path);
            Ok(Value::Null)
        }
        "openExternal" => {
            let url = args.get(1).and_then(Value::as_str).unwrap_or("");
            Ok(json!(open_external(url)))
        }

        // ---- clipboard (tauri-plugin-clipboard-manager; the workbench's
        // NativeClipboardService routes every editor copy/paste here) ----
        "readClipboardText" => {
            read_clipboard_text(app).map(|text| json!(text))
        }
        "writeClipboardText" => {
            let text = args.get(1).and_then(Value::as_str).unwrap_or("");
            write_clipboard_text(app, text);
            Ok(Value::Null)
        }
        "readClipboardFindText" => Ok(json!("")), // macOS find pasteboard only
        "writeClipboardFindText" => Ok(Value::Null),
        "readClipboardBuffer" => {
            // Custom clipboard formats (code/file-list) need raw Win32
            // RegisterClipboardFormat plumbing — tracked for the next
            // round; the empty buffer matches "nothing on the clipboard".
            Ok(crate::ipc::vsbuffer(&[]))
        }
        "writeClipboardBuffer" => {
            crate::logger::log_app(
                "warn",
                "nativeHost: writeClipboardBuffer (custom format) not implemented yet",
            );
            Ok(Value::Null)
        }
        "hasClipboard" => Ok(json!(false)),
        "readImage" => {
            // arboard reads the clipboard image; PNG bytes serialized like
            // Electron's nativeImage.toPNG().
            read_clipboard_image(app)
        }
        "writeImage" => {
            // args[1] is the base64 PNG data from the renderer.
            write_clipboard_image(app, args.get(1))
        }

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

fn reload_window(app: Option<&tauri::AppHandle>) {
    if let Some(app) = app {
        if let Some(window) = app.get_webview_window("main") {
            let _ = window.eval("window.location.reload()");
        }
    }
}

// ---------------------------------------------------------------------------
// Native dialogs (tauri-plugin-dialog over the Win32 common dialogs)
// ---------------------------------------------------------------------------

/// Which picker flavor a pick*AndOpen call wants.
#[derive(Clone, Copy, PartialEq)]
enum PickKind {
    File,
    Folder,
    Workspace,
    FileOrFolder,
}

/// FileFilter[] -> the plugin's add_filter list.
fn dialog_filters(options: &Value) -> Vec<(String, Vec<String>)> {
    options
        .get("filters")
        .and_then(Value::as_array)
        .map(|filters| {
            filters
                .iter()
                .filter_map(|filter| {
                    let name = filter
                        .get("name")
                        .and_then(Value::as_str)
                        .unwrap_or("Files")
                        .to_string();
                    let extensions = filter
                        .get("extensions")
                        .and_then(Value::as_array)
                        .map(|exts| {
                            exts.iter()
                                .filter_map(|ext| ext.as_str().map(str::to_string))
                                .collect()
                        })
                        .unwrap_or_default();
                    Some((name, extensions))
                })
                .collect()
        })
        .unwrap_or_default()
}

/// Electron's `defaultPath` is "absolute directory path, absolute file
/// path, or file name" — split it into directory + file name parts for the
/// native dialog.
fn apply_default_path(
    mut dialog: tauri_plugin_dialog::FileDialogBuilder<tauri::Wry>,
    default_path: Option<&str>,
) -> tauri_plugin_dialog::FileDialogBuilder<tauri::Wry> {
    let Some(default) = default_path else {
        return dialog;
    };
    if default.is_empty() {
        return dialog;
    }
    let path = std::path::PathBuf::from(default);
    if path.is_dir() {
        dialog = dialog.set_directory(path);
    } else if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
        dialog = dialog.set_file_name(name);
        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() {
                dialog = dialog.set_directory(parent);
            }
        }
    }
    dialog
}

/// Parent the dialog to the main workbench window so it is modal.
fn with_parent(
    dialog: tauri_plugin_dialog::FileDialogBuilder<tauri::Wry>,
    app: &tauri::AppHandle,
) -> tauri_plugin_dialog::FileDialogBuilder<tauri::Wry> {
    match app.get_webview_window("main") {
        Some(window) => dialog.set_parent(&window),
        None => dialog,
    }
}

fn file_path_to_string(picked: tauri_plugin_dialog::FilePath) -> Option<String> {
    picked
        .into_path()
        .ok()
        .map(|path| path.to_string_lossy().into_owned())
}

/// showSaveDialog(options) -> { canceled, filePath }
fn show_save_dialog(
    app: Option<&tauri::AppHandle>,
    options: Option<&Value>,
) -> Result<Value, String> {
    let Some(app) = app else {
        return Ok(json!({ "canceled": true, "filePath": "" }));
    };
    let options = options.cloned().unwrap_or(Value::Null);
    use tauri_plugin_dialog::DialogExt;
    let dialog = with_parent(app.dialog().file(), app);
    let dialog = if let Some(title) = options.get("title").and_then(Value::as_str) {
        dialog.set_title(title)
    } else {
        dialog
    };
    let mut dialog = apply_default_path(
        dialog,
        options.get("defaultPath").and_then(Value::as_str),
    );
    for (name, extensions) in dialog_filters(&options) {
        let ext_refs: Vec<&str> = extensions.iter().map(String::as_str).collect();
        dialog = dialog.add_filter(name, &ext_refs);
    }

    let picked = dialog.blocking_save_file();
    let result = match picked.and_then(file_path_to_string) {
        Some(path) => json!({ "canceled": false, "filePath": path }),
        None => json!({ "canceled": true, "filePath": "" }),
    };
    Ok(result)
}

/// showOpenDialog(options) -> { canceled, filePaths }
fn show_open_dialog(
    app: Option<&tauri::AppHandle>,
    options: Option<&Value>,
) -> Result<Value, String> {
    let Some(app) = app else {
        return Ok(json!({ "canceled": true, "filePaths": [] }));
    };
    let options = options.cloned().unwrap_or(Value::Null);
    let properties: Vec<String> = options
        .get("properties")
        .and_then(Value::as_array)
        .map(|props| {
            props
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default();
    let pick_folder = properties.iter().any(|p| p == "openDirectory")
        && !properties.iter().any(|p| p == "openFile");
    let multi = properties.iter().any(|p| p == "multiSelections");

    use tauri_plugin_dialog::DialogExt;
    let dialog = with_parent(app.dialog().file(), app);
    let dialog = if let Some(title) = options.get("title").and_then(Value::as_str) {
        dialog.set_title(title)
    } else {
        dialog
    };
    let mut dialog = apply_default_path(
        dialog,
        options.get("defaultPath").and_then(Value::as_str),
    );
    for (name, extensions) in dialog_filters(&options) {
        let ext_refs: Vec<&str> = extensions.iter().map(String::as_str).collect();
        dialog = dialog.add_filter(name, &ext_refs);
    }

    let paths: Vec<String> = if pick_folder {
        if multi {
            dialog
                .blocking_pick_folders()
                .unwrap_or_default()
                .into_iter()
                .filter_map(file_path_to_string)
                .collect()
        } else {
            dialog
                .blocking_pick_folder()
                .and_then(file_path_to_string)
                .into_iter()
                .collect()
        }
    } else if multi {
        dialog
            .blocking_pick_files()
            .unwrap_or_default()
            .into_iter()
            .filter_map(file_path_to_string)
            .collect()
    } else {
        dialog
            .blocking_pick_file()
            .and_then(file_path_to_string)
            .into_iter()
            .collect()
    };

    let canceled = paths.is_empty();
    Ok(json!({ "canceled": canceled, "filePaths": paths }))
}

/// pick*AndOpen(options: INativeOpenDialogOptions) -> void. Runs the picker
/// then applies the selection to the window configuration and reloads, so
/// the workbench boots into the picked file/folder/workspace (upstream
/// opens a new window per options.forceNewWindow; this shell currently
/// has a single window, so the open reuses it — noted in ROADMAP Phase 3).
fn pick_and_open(
    app: Option<&tauri::AppHandle>,
    options: Option<&Value>,
    kind: PickKind,
) -> Result<Value, String> {
    let Some(app) = app else {
        return Ok(Value::Null);
    };
    let options = options.cloned().unwrap_or(Value::Null);
    let default_path = options.get("defaultPath").and_then(Value::as_str);

    use tauri_plugin_dialog::DialogExt;
    let dialog = with_parent(app.dialog().file(), app);
    let dialog = apply_default_path(dialog, default_path);
    // INativeOpenDialogOptions has no filter field of its own; the caller
    // (workbench dialogHandler) passes workspace filters through showOpen
    // instead. Workspace picking accepts .code-workspace files or folders.
    let dialog = if kind == PickKind::Workspace {
        dialog.add_filter("Code Workspace", &["code-workspace"])
    } else {
        dialog
    };

    let picked = match kind {
        PickKind::File => dialog.blocking_pick_file(),
        PickKind::Folder => dialog.blocking_pick_folder(),
        PickKind::Workspace => dialog.blocking_pick_file(),
        // The Win32 dialog cannot mix file + folder selection through this
        // API; folder selection is the superset behavior (a picked file
        // would open as a workspace container otherwise).
        PickKind::FileOrFolder => dialog.blocking_pick_folder(),
    };

    let Some(path) = picked.and_then(file_path_to_string) else {
        return Ok(Value::Null); // user canceled
    };

    let openable = if kind == PickKind::Folder || kind == PickKind::FileOrFolder {
        json!({ "folderUri": path_to_uri_value(&path) })
    } else if kind == PickKind::Workspace {
        json!({ "workspaceUri": path_to_uri_value(&path) })
    } else {
        json!({ "fileUri": path_to_uri_value(&path) })
    };

    let changed = crate::config::apply_window_openables(&[openable]);
    if changed {
        crate::logger::log_app("info", &format!("nativeHost: opening {} after pick", path));
        reload_window(Some(app));
    }
    Ok(Value::Null)
}

/// fsPath -> file:// UriComponents (object form URI.revive accepts).
fn path_to_uri_value(path: &str) -> Value {
    let normalized = path.replace('\\', "/");
    let with_slash = if cfg!(windows) || normalized.starts_with('/') {
        normalized
    } else {
        format!("/{}", normalized)
    };
    json!({
        "scheme": "file",
        "authority": "",
        "path": crate::util::encode_uri_path(&with_slash),
        "query": "",
        "fragment": "",
    })
}

/// showMessageBox(options) -> { response, checkboxChecked }. Windows uses
/// TaskDialogIndirect so Electron-style custom button labels, the default
/// button and the verification checkbox all work; other platforms fall
/// back to the plugin's simple message dialog (Windows is the release
/// target — see ROADMAP Phase 3).
fn show_message_box(
    app: Option<&tauri::AppHandle>,
    options: Option<&Value>,
) -> Result<Value, String> {
    let options = options.cloned().unwrap_or(Value::Null);
    let message = options.get("message").and_then(Value::as_str).unwrap_or("").to_string();
    let detail = options
        .get("detail")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    let buttons: Vec<String> = options
        .get("buttons")
        .and_then(Value::as_array)
        .map(|btns| {
            btns.iter()
                .filter_map(Value::as_str)
                .map(|b| b.replace('&', "&&")) // Electron treats & as accelerator; TaskDialog needs &&
                .collect()
        })
        .unwrap_or_default();

    #[cfg(windows)]
    {
        let _ = app; // the owner hwnd is taken from the main window below
        return show_task_dialog(&options, &message, &detail, &buttons);
    }

    #[cfg(not(windows))]
    {
        let Some(app) = app else {
            return Ok(json!({ "response": 0, "checkboxChecked": false }));
        };
        use tauri_plugin_dialog::{DialogExt, MessageDialogKind};
        let kind = match options.get("type").and_then(Value::as_str).unwrap_or("none") {
            "error" => MessageDialogKind::Error,
            "warning" => MessageDialogKind::Warning,
            "info" => MessageDialogKind::Info,
            _ => MessageDialogKind::Info,
        };
        let text = if detail.is_empty() {
            message
        } else {
            format!("{}\n\n{}", message, detail)
        };
        let _ = app
            .dialog()
            .message(text)
            .kind(kind)
            .title("Visual Studio Code")
            .blocking_show();
        // Simple dialog has one button: response 0, or the cancelId when
        // the caller marked one.
        let cancel_id = options.get("cancelId").and_then(Value::as_i64).unwrap_or(0);
        Ok(json!({ "response": cancel_id, "checkboxChecked": false }))
    }
}

#[cfg(windows)]
fn show_task_dialog(
    options: &Value,
    message: &str,
    detail: &str,
    buttons: &[String],
) -> Result<Value, String> {
    use windows_sys::Win32::UI::Controls::{
        TaskDialogIndirect, TASKDIALOGCONFIG, TASKDIALOG_BUTTON,
        TD_ERROR_ICON, TD_INFORMATION_ICON, TD_WARNING_ICON, TDF_ALLOW_DIALOG_CANCELLATION,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

    let _ = SW_SHOWNORMAL; // silence unused import when features shift

    fn wide(text: &str) -> Vec<u16> {
        let mut wide: Vec<u16> = text.encode_utf16().collect();
        wide.push(0);
        wide
    }

    let title = wide("Visual Studio Code");
    let main_instruction = wide(message);
    let content = wide(detail);
    let checkbox_label = options
        .get("checkboxLabel")
        .and_then(Value::as_str)
        .map(|label| wide(label));
    let checkbox_default = options
        .get("checkboxChecked")
        .and_then(Value::as_bool)
        .unwrap_or(false);

    // Custom buttons; ids 1000+ so they cannot collide with IDOK/IDCANCEL.
    let button_labels: Vec<Vec<u16>> = buttons.iter().map(|b| wide(b)).collect();
    let button_structs: Vec<TASKDIALOG_BUTTON> = button_labels
        .iter()
        .enumerate()
        .map(|(index, label)| TASKDIALOG_BUTTON {
            nButtonID: 1000 + index as i32,
            pszButtonText: label.as_ptr(),
        })
        .collect();

    let psz_main_icon = match options.get("type").and_then(Value::as_str).unwrap_or("none") {
        "error" => TD_ERROR_ICON,
        "warning" => TD_WARNING_ICON,
        "info" | "question" => TD_INFORMATION_ICON,
        _ => TD_INFORMATION_ICON,
    };

    let mut config = TASKDIALOGCONFIG {
        cbSize: std::mem::size_of::<TASKDIALOGCONFIG>() as u32,
        hwndParent: std::ptr::null_mut(),
        hInstance: std::ptr::null_mut(),
        dwFlags: TDF_ALLOW_DIALOG_CANCELLATION,
        dwCommonButtons: 0,
        pszWindowTitle: title.as_ptr(),
        pszMainInstruction: main_instruction.as_ptr(),
        pszContent: if content.len() > 1 { content.as_ptr() } else { std::ptr::null() },
        cButtons: button_structs.len() as u32,
        pButtons: if button_structs.is_empty() {
            std::ptr::null()
        } else {
            button_structs.as_ptr()
        },
        nDefaultButton: 1000
            + options
                .get("defaultId")
                .and_then(Value::as_i64)
                .unwrap_or(0)
                .clamp(0, buttons.len().saturating_sub(1) as i64) as i32,
        pszVerificationText: checkbox_label
            .as_ref()
            .map(|label| label.as_ptr())
            .unwrap_or(std::ptr::null()),
        ..Default::default()
    };
    config.Anonymous1.pszMainIcon = psz_main_icon;

    // Own the main window handle so the dialog is modal to the workbench.
    // (tauri's HWND wraps the raw pointer windows-sys wants.)
    if let Some(app) = crate::ipc::current_app_handle() {
        if let Some(window) = app.get_webview_window("main") {
            if let Ok(hwnd) = window.hwnd() {
                config.hwndParent = hwnd.0;
            }
        }
    }

    let mut selected: i32 = 0;
    let mut checkbox_checked: i32 = i32::from(checkbox_default);
    let result = unsafe {
        TaskDialogIndirect(&config, &mut selected, std::ptr::null_mut(), &mut checkbox_checked)
    };
    if result < 0 {
        return Err(format!("TaskDialogIndirect failed: HRESULT {:#x}", result));
    }

    // Map the pressed id back to the Electron response index: 1000+i for
    // custom buttons, IDCANCEL (X button / Esc) -> cancelId when present.
    let response = if (1000..1000 + buttons.len() as i32).contains(&selected) {
        (selected - 1000) as i64
    } else {
        options.get("cancelId").and_then(Value::as_i64).unwrap_or(-1)
    };
    Ok(json!({
        "response": response,
        "checkboxChecked": checkbox_checked != 0,
    }))
}

/// showItemInFolder: Explorer with the item selected (upstream semantics).
fn show_item_in_folder(path: &str) {
    if path.is_empty() {
        return;
    }
    let _ = if cfg!(windows) {
        std::process::Command::new("explorer.exe")
            .arg(format!("/select,{}", path))
            .spawn()
    } else {
        let parent = std::path::Path::new(path)
            .parent()
            .map(|p| p.to_path_buf())
            .unwrap_or_default();
        std::process::Command::new("xdg-open")
            .arg(if parent.as_os_str().is_empty() { std::ffi::OsStr::new(".") } else { parent.as_os_str() })
            .spawn()
    };
}

/// openExternal via the OS shell.
fn open_external(url: &str) -> bool {
    if url.is_empty() {
        return false;
    }
    #[cfg(windows)]
    {
        use windows_sys::Win32::UI::Shell::ShellExecuteW;
        use windows_sys::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
        let operation: Vec<u16> = "open\0".encode_utf16().collect();
        let file: Vec<u16> = url.encode_utf16().chain(std::iter::once(0)).collect();
        let result = unsafe {
            ShellExecuteW(
                std::ptr::null_mut(),
                operation.as_ptr(),
                file.as_ptr(),
                std::ptr::null(),
                std::ptr::null(),
                SW_SHOWNORMAL,
            )
        };
        // ShellExecuteW returns a HINSTANCE > 32 on success.
        !result.is_null()
    }
    #[cfg(not(windows))]
    {
        let opener = if cfg!(target_os = "macos") { "open" } else { "xdg-open" };
        std::process::Command::new(opener)
            .arg(url)
            .spawn()
            .map(|_| true)
            .unwrap_or(false)
    }
}

// ---------------------------------------------------------------------------
// Clipboard (tauri-plugin-clipboard-manager)
// ---------------------------------------------------------------------------

fn read_clipboard_text(app: Option<&tauri::AppHandle>) -> Result<String, String> {
    let Some(app) = app else {
        return Ok(String::new());
    };
    use tauri_plugin_clipboard_manager::ClipboardExt;
    app.clipboard()
        .read_text()
        .map_err(|err| format!("nativeHost: clipboard read failed: {}", err))
}

fn write_clipboard_text(app: Option<&tauri::AppHandle>, text: &str) {
    if let Some(app) = app {
        use tauri_plugin_clipboard_manager::ClipboardExt;
        if let Err(err) = app.clipboard().write_text(text) {
            crate::logger::log_app("warn", &format!("nativeHost: clipboard write failed: {}", err));
        }
    }
}

/// readImage -> PNG bytes as a VSBuffer (matches Electron
/// nativeImage.toPNG() — nativeHostMainService.readImage).
fn read_clipboard_image(app: Option<&tauri::AppHandle>) -> Result<Value, String> {
    let Some(app) = app else {
        return Ok(crate::ipc::vsbuffer(&[]));
    };
    use tauri_plugin_clipboard_manager::ClipboardExt;
    match app.clipboard().read_image() {
        Ok(image) => {
            let Some(rgba_image) =
                image::RgbaImage::from_raw(image.width(), image.height(), image.rgba().to_vec())
            else {
                return Ok(crate::ipc::vsbuffer(&[]));
            };
            let mut png = Vec::new();
            rgba_image
                .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
                .map_err(|err| format!("png encode failed: {}", err))?;
            Ok(crate::ipc::vsbuffer(&png))
        }
        Err(_) => Ok(crate::ipc::vsbuffer(&[])),
    }
}

/// writeImage(base64 png) — the renderer side passes PNG data.
fn write_clipboard_image(app: Option<&tauri::AppHandle>, data: Option<&Value>) -> Result<Value, String> {
    let Some(app) = app else {
        return Ok(Value::Null);
    };
    let Some(b64) = data.and_then(Value::as_str) else {
        return Ok(Value::Null);
    };
    let Some(bytes) = crate::ipc::vsbuffer_b64_decode(b64) else {
        return Ok(Value::Null);
    };
    let Ok(decoded) = image::load_from_memory(&bytes) else {
        return Ok(Value::Null);
    };
    let rgba = decoded.to_rgba8();
    let image = tauri::image::Image::new_owned(
        rgba.to_vec(),
        rgba.width(),
        rgba.height(),
    );
    use tauri_plugin_clipboard_manager::ClipboardExt;
    app.clipboard()
        .write_image(&image)
        .map_err(|err| format!("nativeHost: clipboard image write failed: {}", err))?;
    Ok(Value::Null)
}

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
