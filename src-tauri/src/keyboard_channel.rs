//! Mountain: `keyboardLayout` protocol channel.
//!
//! Implements the `INativeKeyboardLayoutService` surface
//! (src/vs/platform/keyboardLayout/common/keyboardLayoutService.ts, exposed
//! through `ProxyChannel.fromService` in app.ts):
//!
//!   getKeyboardLayoutData() -> {
//!     keyboardLayoutInfo: { name, id, text }        (IWindowsKeyboardLayoutInfo)
//!     keyboardMapping: { [code]: { vkey, value, withShift, withAltGr, withShiftAltGr } }
//!   }
//!   event: onDidChangeKeyboardLayout(IKeyboardLayoutData)
//!
//! Electron gets this data from the `native-keymap` npm module
//! (nativeKeymap.getKeyMap() + getCurrentKeyboardLayout()). This module
//! answers with the canonical **US QWERTY layout** — the default Windows
//! layout and the one every keybinding in VS Code resolves against — built
//! from the same VK tables the renderer consumes
//! (src/vs/base/common/keyCodes.ts `NATIVE_WINDOWS_KEY_CODE_TO_KEY_CODE`).
//!
//! On real Windows the exact active layout could be queried via
//! `GetKeyboardLayout`/`GetKeyboardLayoutList`; until a layout-switching
//! listener lands, serving the US table deterministically matches the
//! behavior of a US-English Windows machine.

use serde_json::{json, Map, Value};
use std::sync::OnceLock;

static LAYOUT_DATA: OnceLock<Value> = OnceLock::new();

/// Called from config::build (no async work needed).
pub fn init() {
    let _ = LAYOUT_DATA.get_or_init(build_layout_data);
}

/// Handle one `keyboardLayout` channel request.
pub fn handle(command: &str, _arg: &Value) -> Result<Value, String> {
    match command {
        "getKeyboardLayoutData" => Ok(LAYOUT_DATA
            .get()
            .cloned()
            .unwrap_or_else(|| build_layout_data())),
        other => Err(format!("keyboardLayout channel: call not found: {}", other)),
    }
}

/// The layout event never fires in this shell yet: the WebView2 keyboard
/// events already carry `code`/`key`, and the workbench's keybinding service
/// only re-maps when the OS layout actually changes. Registered listeners are
/// simply never signaled (matching a machine whose layout never changes).
#[allow(dead_code)] // wired to the OS layout-change listener in a later phase
pub fn fire_layout_change() {
    if let Some(data) = LAYOUT_DATA.get() {
        crate::ipc::fire_event("keyboardLayout", "onDidChangeKeyboardLayout", data);
    }
}

fn build_layout_data() -> Value {
    let mut mapping = Map::new();
    for entry in US_LAYOUT {
        mapping.insert(
            entry.code.to_string(),
            json!({
                "vkey": entry.vkey,
                "value": entry.value,
                "withShift": entry.with_shift,
                "withAltGr": entry.value,
                "withShiftAltGr": entry.with_shift,
            }),
        );
    }
    json!({
        "keyboardLayoutInfo": {
            "name": "00000409",
            "id": "00000409",
            "text": "United States-English"
        },
        "keyboardMapping": Value::Object(mapping),
    })
}

/// (code, vkey, value, withShift) — US QWERTY. AltGr variants mirror the
/// unshifted/shifted values as on a genuine US layout.
struct KeyEntry {
    code: &'static str,
    vkey: &'static str,
    value: &'static str,
    with_shift: &'static str,
}
const US_LAYOUT: &[KeyEntry] = &[
    KeyEntry { code: "Backquote", vkey: "VK_OEM_3", value: "`", with_shift: "~" },
    KeyEntry { code: "Digit1", vkey: "VK_1", value: "1", with_shift: "!" },
    KeyEntry { code: "Digit2", vkey: "VK_2", value: "2", with_shift: "@" },
    KeyEntry { code: "Digit3", vkey: "VK_3", value: "3", with_shift: "#" },
    KeyEntry { code: "Digit4", vkey: "VK_4", value: "4", with_shift: "$" },
    KeyEntry { code: "Digit5", vkey: "VK_5", value: "5", with_shift: "%" },
    KeyEntry { code: "Digit6", vkey: "VK_6", value: "6", with_shift: "^" },
    KeyEntry { code: "Digit7", vkey: "VK_7", value: "7", with_shift: "&" },
    KeyEntry { code: "Digit8", vkey: "VK_8", value: "8", with_shift: "*" },
    KeyEntry { code: "Digit9", vkey: "VK_9", value: "9", with_shift: "(" },
    KeyEntry { code: "Digit0", vkey: "VK_0", value: "0", with_shift: ")" },
    KeyEntry { code: "Minus", vkey: "VK_OEM_MINUS", value: "-", with_shift: "_" },
    KeyEntry { code: "Equal", vkey: "VK_OEM_PLUS", value: "=", with_shift: "+" },
    KeyEntry { code: "Backspace", vkey: "VK_BACK", value: "", with_shift: "" },
    KeyEntry { code: "Tab", vkey: "VK_TAB", value: "", with_shift: "" },
    KeyEntry { code: "KeyQ", vkey: "VK_Q", value: "q", with_shift: "Q" },
    KeyEntry { code: "KeyW", vkey: "VK_W", value: "w", with_shift: "W" },
    KeyEntry { code: "KeyE", vkey: "VK_E", value: "e", with_shift: "E" },
    KeyEntry { code: "KeyR", vkey: "VK_R", value: "r", with_shift: "R" },
    KeyEntry { code: "KeyT", vkey: "VK_T", value: "t", with_shift: "T" },
    KeyEntry { code: "KeyY", vkey: "VK_Y", value: "y", with_shift: "Y" },
    KeyEntry { code: "KeyU", vkey: "VK_U", value: "u", with_shift: "U" },
    KeyEntry { code: "KeyI", vkey: "VK_I", value: "i", with_shift: "I" },
    KeyEntry { code: "KeyO", vkey: "VK_O", value: "o", with_shift: "O" },
    KeyEntry { code: "KeyP", vkey: "VK_P", value: "p", with_shift: "P" },
    KeyEntry { code: "BracketLeft", vkey: "VK_OEM_4", value: "[", with_shift: "{" },
    KeyEntry { code: "BracketRight", vkey: "VK_OEM_6", value: "]", with_shift: "}" },
    KeyEntry { code: "Backslash", vkey: "VK_OEM_5", value: "\\", with_shift: "|" },
    KeyEntry { code: "CapsLock", vkey: "VK_CAPITAL", value: "", with_shift: "" },
    KeyEntry { code: "KeyA", vkey: "VK_A", value: "a", with_shift: "A" },
    KeyEntry { code: "KeyS", vkey: "VK_S", value: "s", with_shift: "S" },
    KeyEntry { code: "KeyD", vkey: "VK_D", value: "d", with_shift: "D" },
    KeyEntry { code: "KeyF", vkey: "VK_F", value: "f", with_shift: "F" },
    KeyEntry { code: "KeyG", vkey: "VK_G", value: "g", with_shift: "G" },
    KeyEntry { code: "KeyH", vkey: "VK_H", value: "h", with_shift: "H" },
    KeyEntry { code: "KeyJ", vkey: "VK_J", value: "j", with_shift: "J" },
    KeyEntry { code: "KeyK", vkey: "VK_K", value: "k", with_shift: "K" },
    KeyEntry { code: "KeyL", vkey: "VK_L", value: "l", with_shift: "L" },
    KeyEntry { code: "Semicolon", vkey: "VK_OEM_1", value: ";", with_shift: ":" },
    KeyEntry { code: "Quote", vkey: "VK_OEM_7", value: "'", with_shift: "\"" },
    KeyEntry { code: "Enter", vkey: "VK_RETURN", value: "", with_shift: "" },
    KeyEntry { code: "ShiftLeft", vkey: "VK_SHIFT", value: "", with_shift: "" },
    KeyEntry { code: "KeyZ", vkey: "VK_Z", value: "z", with_shift: "Z" },
    KeyEntry { code: "KeyX", vkey: "VK_X", value: "x", with_shift: "X" },
    KeyEntry { code: "KeyC", vkey: "VK_C", value: "c", with_shift: "C" },
    KeyEntry { code: "KeyV", vkey: "VK_V", value: "v", with_shift: "V" },
    KeyEntry { code: "KeyB", vkey: "VK_B", value: "b", with_shift: "B" },
    KeyEntry { code: "KeyN", vkey: "VK_N", value: "n", with_shift: "N" },
    KeyEntry { code: "KeyM", vkey: "VK_M", value: "m", with_shift: "M" },
    KeyEntry { code: "Comma", vkey: "VK_OEM_COMMA", value: ",", with_shift: "<" },
    KeyEntry { code: "Period", vkey: "VK_OEM_PERIOD", value: ".", with_shift: ">" },
    KeyEntry { code: "Slash", vkey: "VK_OEM_2", value: "/", with_shift: "?" },
    KeyEntry { code: "ShiftRight", vkey: "VK_SHIFT", value: "", with_shift: "" },
    KeyEntry { code: "ControlLeft", vkey: "VK_CONTROL", value: "", with_shift: "" },
    KeyEntry { code: "MetaLeft", vkey: "VK_LWIN", value: "", with_shift: "" },
    KeyEntry { code: "AltLeft", vkey: "VK_MENU", value: "", with_shift: "" },
    KeyEntry { code: "Space", vkey: "VK_SPACE", value: " ", with_shift: " " },
    KeyEntry { code: "AltRight", vkey: "VK_MENU", value: "", with_shift: "" },
    KeyEntry { code: "MetaRight", vkey: "VK_RWIN", value: "", with_shift: "" },
    KeyEntry { code: "ContextMenu", vkey: "VK_APPS", value: "", with_shift: "" },
    KeyEntry { code: "ControlRight", vkey: "VK_CONTROL", value: "", with_shift: "" },
    KeyEntry { code: "ArrowLeft", vkey: "VK_LEFT", value: "", with_shift: "" },
    KeyEntry { code: "ArrowUp", vkey: "VK_UP", value: "", with_shift: "" },
    KeyEntry { code: "ArrowDown", vkey: "VK_DOWN", value: "", with_shift: "" },
    KeyEntry { code: "ArrowRight", vkey: "VK_RIGHT", value: "", with_shift: "" },
    KeyEntry { code: "NumLock", vkey: "VK_NUMLOCK", value: "", with_shift: "" },
    KeyEntry { code: "NumpadDivide", vkey: "VK_DIVIDE", value: "/", with_shift: "/" },
    KeyEntry { code: "NumpadMultiply", vkey: "VK_MULTIPLY", value: "*", with_shift: "*" },
    KeyEntry { code: "NumpadSubtract", vkey: "VK_SUBTRACT", value: "-", with_shift: "-" },
    KeyEntry { code: "NumpadAdd", vkey: "VK_ADD", value: "+", with_shift: "+" },
    KeyEntry { code: "NumpadEnter", vkey: "VK_RETURN", value: "", with_shift: "" },
    KeyEntry { code: "Numpad1", vkey: "VK_NUMPAD1", value: "1", with_shift: "" },
    KeyEntry { code: "Numpad2", vkey: "VK_NUMPAD2", value: "2", with_shift: "" },
    KeyEntry { code: "Numpad3", vkey: "VK_NUMPAD3", value: "3", with_shift: "" },
    KeyEntry { code: "Numpad4", vkey: "VK_NUMPAD4", value: "4", with_shift: "" },
    KeyEntry { code: "Numpad5", vkey: "VK_NUMPAD5", value: "5", with_shift: "" },
    KeyEntry { code: "Numpad6", vkey: "VK_NUMPAD6", value: "6", with_shift: "" },
    KeyEntry { code: "Numpad7", vkey: "VK_NUMPAD7", value: "7", with_shift: "" },
    KeyEntry { code: "Numpad8", vkey: "VK_NUMPAD8", value: "8", with_shift: "" },
    KeyEntry { code: "Numpad9", vkey: "VK_NUMPAD9", value: "9", with_shift: "" },
    KeyEntry { code: "Numpad0", vkey: "VK_NUMPAD0", value: "0", with_shift: "" },
    KeyEntry { code: "NumpadDecimal", vkey: "VK_DECIMAL", value: ".", with_shift: "" },
    KeyEntry { code: "Escape", vkey: "VK_ESCAPE", value: "", with_shift: "" },
    KeyEntry { code: "Delete", vkey: "VK_DELETE", value: "", with_shift: "" },
    KeyEntry { code: "End", vkey: "VK_END", value: "", with_shift: "" },
    KeyEntry { code: "Home", vkey: "VK_HOME", value: "", with_shift: "" },
    KeyEntry { code: "Insert", vkey: "VK_INSERT", value: "", with_shift: "" },
    KeyEntry { code: "PageDown", vkey: "VK_NEXT", value: "", with_shift: "" },
    KeyEntry { code: "PageUp", vkey: "VK_PRIOR", value: "", with_shift: "" },
    KeyEntry { code: "F1", vkey: "VK_F1", value: "", with_shift: "" },
    KeyEntry { code: "F2", vkey: "VK_F2", value: "", with_shift: "" },
    KeyEntry { code: "F3", vkey: "VK_F3", value: "", with_shift: "" },
    KeyEntry { code: "F4", vkey: "VK_F4", value: "", with_shift: "" },
    KeyEntry { code: "F5", vkey: "VK_F5", value: "", with_shift: "" },
    KeyEntry { code: "F6", vkey: "VK_F6", value: "", with_shift: "" },
    KeyEntry { code: "F7", vkey: "VK_F7", value: "", with_shift: "" },
    KeyEntry { code: "F8", vkey: "VK_F8", value: "", with_shift: "" },
    KeyEntry { code: "F9", vkey: "VK_F9", value: "", with_shift: "" },
    KeyEntry { code: "F10", vkey: "VK_F10", value: "", with_shift: "" },
    KeyEntry { code: "F11", vkey: "VK_F11", value: "", with_shift: "" },
    KeyEntry { code: "F12", vkey: "VK_F12", value: "", with_shift: "" },
    KeyEntry { code: "F13", vkey: "VK_F13", value: "", with_shift: "" },
    KeyEntry { code: "F14", vkey: "VK_F14", value: "", with_shift: "" },
    KeyEntry { code: "F15", vkey: "VK_F15", value: "", with_shift: "" },
    KeyEntry { code: "F16", vkey: "VK_F16", value: "", with_shift: "" },
    KeyEntry { code: "F17", vkey: "VK_F17", value: "", with_shift: "" },
    KeyEntry { code: "F18", vkey: "VK_F18", value: "", with_shift: "" },
    KeyEntry { code: "F19", vkey: "VK_F19", value: "", with_shift: "" },
    KeyEntry { code: "PrintScreen", vkey: "VK_SNAPSHOT", value: "", with_shift: "" },
    KeyEntry { code: "ScrollLock", vkey: "VK_SCROLL", value: "", with_shift: "" },
    KeyEntry { code: "Pause", vkey: "VK_PAUSE", value: "", with_shift: "" },
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layout_data_shape_matches_ikblayoutdata() {
        let data = build_layout_data();
        let info = data.get("keyboardLayoutInfo").expect("info");
        assert!(info.get("name").and_then(Value::as_str).is_some());
        assert!(info.get("id").and_then(Value::as_str).is_some());
        assert!(info.get("text").and_then(Value::as_str).is_some());

        let mapping = data.get("keyboardMapping").and_then(Value::as_object).expect("mapping");
        assert!(mapping.len() >= 90, "expected a full keyboard table, got {}", mapping.len());
        for (code, entry) in mapping {
            for field in ["vkey", "value", "withShift", "withAltGr", "withShiftAltGr"] {
                assert!(entry.get(field).is_some(), "{} missing {}", code, field);
            }
        }
    }

    #[test]
    fn key_letters_have_shift_pairs() {
        let data = build_layout_data();
        let mapping = data.get("keyboardMapping").and_then(Value::as_object).unwrap();
        let a = mapping.get("KeyA").unwrap();
        assert_eq!(a.get("value").and_then(Value::as_str), Some("a"));
        assert_eq!(a.get("withShift").and_then(Value::as_str), Some("A"));
        assert_eq!(a.get("vkey").and_then(Value::as_str), Some("VK_A"));
    }

    #[test]
    fn handle_returns_the_data() {
        init();
        let data = handle("getKeyboardLayoutData", &serde_json::json!([])).expect("data");
        assert!(data.get("keyboardMapping").is_some());
        assert!(handle("nope", &serde_json::json!([])).is_err());
    }
}
