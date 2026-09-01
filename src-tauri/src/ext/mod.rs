pub mod manifest;

use manifest::ExtensionManifest;
use serde::Serialize;
use std::path::PathBuf;

/// A minimal, Rust-native extension runtime.
///
/// Phase 1 (this commit): extension discovery + manifest parsing.
/// Extensions live in `~/.vstauri/extensions/<publisher.name>/extension.json`.
///
/// Phase 2 (roadmap): a WASM runtime (wasmtime) executes extension `main`
/// modules inside a capability-based sandbox. The host exposes a narrow API
/// surface via host functions (commands, editor mutations, fs-scoped access),
/// mirroring the `vstauri` namespace described in docs/ARCHITECTURE.md.
pub trait ExtensionRuntime {
    fn activate(&mut self, id: &str) -> Result<(), String>;
    fn deactivate(&mut self, id: &str);
    fn execute_command(
        &mut self,
        command: &str,
        args: serde_json::Value,
    ) -> Result<serde_json::Value, String>;
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct InstalledExtension {
    pub manifest: ExtensionManifest,
    pub dir: String,
}

fn extension_dirs(root: Option<String>) -> Vec<PathBuf> {
    let mut v = Vec::new();
    if let Some(home) = dirs::home_dir() {
        v.push(home.join(".vstauri").join("extensions"));
    }
    if let Some(r) = root {
        v.push(PathBuf::from(r).join(".vstauri").join("extensions"));
    }
    v
}

fn scan_dir(dir: &PathBuf) -> Vec<InstalledExtension> {
    let mut out = Vec::new();
    let Ok(rd) = std::fs::read_dir(dir) else {
        return out;
    };
    for entry in rd.flatten() {
        let manifest_path = entry.path().join("extension.json");
        if let Ok(text) = std::fs::read_to_string(&manifest_path) {
            if let Ok(m) = serde_json::from_str::<ExtensionManifest>(&text) {
                out.push(InstalledExtension {
                    manifest: m,
                    dir: entry.path().to_string_lossy().to_string(),
                });
            }
        }
    }
    out
}

#[tauri::command]
pub fn list_extensions(root: Option<String>) -> Vec<InstalledExtension> {
    let mut all = Vec::new();
    for dir in extension_dirs(root) {
        all.extend(scan_dir(&dir));
    }
    all
}
