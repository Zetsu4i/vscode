//! VSTauri — a from-scratch VSCode-style workbench rebuilt on Tauri 2 + Rust.
//!
//! Backend modules:
//! - `fs`       — filesystem operations (listing, reading, writing, rename/delete)
//! - `watcher`  — debounced recursive workspace file watching
//! - `pty`      — real pseudo-terminal sessions (portable-pty) with flow control
//! - `search`   — workspace-wide search on ripgrep's engine + walker (ignore/regex)
//! - `gitcmd`   — source control integration via the git CLI
//! - `lsp`      — generic LSP client (stdio transport) for diagnostics/hover/completion
//! - `ext`      — Rust-native extension system (manifests + WASM runtime scaffolding)
//! - `winstate` — window geometry persistence + restore-and-show startup
//! - `asset`    — `vstauri://` workspace asset protocol with traversal guards

mod asset;
mod ext;
mod fs;
mod gitcmd;
mod lsp;
mod pty;
mod search;
mod watcher;
mod winstate;

use tauri::Manager;

pub fn run() {
    let builder = tauri::Builder::default().plugin(tauri_plugin_dialog::init());
    let builder = asset::register(builder);

    builder
        .manage(pty::PtyState::default())
        .manage(watcher::WatcherState::default())
        .manage(lsp::LspState::default())
        .manage(asset::AssetState::default())
        .invoke_handler(tauri::generate_handler![
            // filesystem
            fs::list_dir,
            fs::read_file,
            fs::write_file,
            fs::create_file,
            fs::create_dir,
            fs::rename_path,
            fs::delete_path,
            fs::list_all_files,
            fs::file_exists,
            // watcher
            watcher::watch_folder,
            // terminal
            pty::create_pty,
            pty::write_pty,
            pty::resize_pty,
            pty::kill_pty,
            pty::pty_attach,
            pty::pty_ack,
            pty::list_shells,
            // search
            search::search_workspace,
            // git
            gitcmd::git_is_repo,
            gitcmd::git_status,
            gitcmd::git_stage,
            gitcmd::git_unstage,
            gitcmd::git_commit,
            gitcmd::git_branch,
            gitcmd::git_log,
            gitcmd::git_show_head,
            gitcmd::git_diff_file,
            // language servers
            lsp::lsp_start,
            lsp::lsp_stop,
            lsp::lsp_status,
            lsp::lsp_did_open,
            lsp::lsp_did_change,
            lsp::lsp_completion,
            lsp::lsp_hover,
            // extensions
            ext::list_extensions,
            // assets + window state
            asset::set_asset_roots,
            winstate::save_window_state,
            winstate::window_ready,
        ])
        .setup(|app| {
            // Restore saved geometry while the window is still hidden; the
            // frontend calls `window_ready` after its first painted frame.
            winstate::restore(app.handle());
            Ok(())
        })
        .on_window_event(|window, event| {
            if matches!(event, tauri::WindowEvent::CloseRequested { .. }) {
                // Authoritative geometry save, then never leave orphan shells behind.
                let _ = winstate::save_now(window);
                if let Some(state) = window.try_state::<pty::PtyState>() {
                    state.kill_all();
                }
            }
        })
        .run(tauri::generate_context!())
        .expect("error while running VSTauri");
}
