// tauri: Phase 0 scaffold shell; Phase 2 adds the native service commands.
// The placeholder page (out/vscode-web) is replaced by the bundled VS Code web
// workbench via `npm run tauri:web` (see docs/tauri/ARCHITECTURE.md).

mod services;

pub fn run() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![
            services::files::fs_stat,
            services::files::fs_exists,
            services::files::fs_readdir,
            services::files::fs_read_file,
            services::files::fs_write_file,
            services::files::fs_mkdir,
            services::files::fs_rename,
            services::files::fs_delete,
        ])
        .run(tauri::generate_context!())
        .expect("failed to run the Code - Tauri shell");
}
