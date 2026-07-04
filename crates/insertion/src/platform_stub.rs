//! Fallback for platforms without a keystroke-injection backend (e.g. macOS).
//! Clipboard still works; paste/copy simulation is a no-op.

pub fn send_paste() -> bool {
    false
}

pub fn send_copy() -> bool {
    false
}

pub fn send_copy_insert() -> bool {
    false
}

pub fn send_terminal_copy() -> bool {
    false
}

pub fn selection_capture_hint() -> Option<String> {
    Some("selection capture is not supported on this platform".into())
}

pub fn notify(summary: &str, body: &str) {
    eprintln!("[notify] {summary}: {body}");
}

pub fn paste_simulation_available() -> bool {
    false
}
