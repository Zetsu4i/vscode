use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct CommandContribution {
    pub command: String,
    pub title: String,
    #[serde(default)]
    pub category: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct ThemeContribution {
    pub label: String,
    pub path: String,
    /// "dark" | "light"
    #[serde(default)]
    pub kind: String,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct KeybindingContribution {
    pub command: String,
    pub key: String,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct Contributes {
    #[serde(default)]
    pub commands: Vec<CommandContribution>,
    #[serde(default)]
    pub themes: Vec<ThemeContribution>,
    #[serde(default)]
    pub keybindings: Vec<KeybindingContribution>,
}

/// `extension.json` — the VSTauri extension manifest (VSCode-inspired, Rust-native).
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct ExtensionManifest {
    /// publisher.name — unique id, must match the containing directory name
    pub id: String,
    pub name: String,
    pub publisher: String,
    pub version: String,
    #[serde(default)]
    pub description: String,
    /// Path to the WASM module executed by the runtime (Phase 2)
    #[serde(default)]
    pub main: Option<String>,
    #[serde(default)]
    pub activation_events: Vec<String>,
    #[serde(default)]
    pub contributes: Contributes,
}
