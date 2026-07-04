//! Hotkey fallback for platforms without a binding backend (e.g. macOS dev).

pub fn current_binding() -> Option<String> {
    None
}

pub fn current_assist_binding() -> Option<String> {
    None
}

pub fn set_binding(_binding: &str, _trigger_command: &str) -> Result<(), String> {
    Err("global hotkeys are not supported on this platform".into())
}

pub fn set_assist_binding(_binding: &str, _trigger_command: &str) -> Result<(), String> {
    Err("global hotkeys are not supported on this platform".into())
}
