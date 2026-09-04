// tauri: Phase 0 scaffold — binary entry (see ROADMAP.md).
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    code_tauri_lib::run();
}
