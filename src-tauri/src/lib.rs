// tauri: Phase 0 scaffold — placeholder shell window.
// Phase 1 replaces the placeholder page with the bundled VS Code web workbench
// (see docs/tauri/ARCHITECTURE.md and docs/tauri/adr/ADR-0001-integration-seam.md).

pub fn run() {
    tauri::Builder::default()
        .run(tauri::generate_context!())
        .expect("failed to run the Code - Tauri shell");
}
