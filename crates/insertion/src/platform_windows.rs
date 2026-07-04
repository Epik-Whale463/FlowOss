//! Windows keystroke injection and notifications.
//!
//! Unlike Wayland, Windows lets any process synthesize input into the focused
//! window via `SendInput`, so paste/copy simulation is always available and
//! needs no external helper.

use windows::Win32::UI::Input::KeyboardAndMouse::{
    SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYBD_EVENT_FLAGS, KEYEVENTF_KEYUP,
    VIRTUAL_KEY, VK_C, VK_CONTROL, VK_INSERT, VK_SHIFT, VK_V,
};

/// Send a synthetic Ctrl+V to the focused window.
pub fn send_paste() -> bool {
    send_combo(&[VK_CONTROL], VK_V)
}

pub fn send_copy() -> bool {
    send_combo(&[VK_CONTROL], VK_C)
}

pub fn send_copy_insert() -> bool {
    send_combo(&[VK_CONTROL], VK_INSERT)
}

pub fn send_terminal_copy() -> bool {
    // Ctrl+Shift+C — the copy chord in Windows Terminal and most consoles.
    send_combo(&[VK_CONTROL, VK_SHIFT], VK_C)
}

/// Windows copies via a universal Ctrl+C; no helper tool to point users at.
pub fn selection_capture_hint() -> Option<String> {
    None
}

/// Press `modifiers` (in order), tap `key`, then release everything (reverse
/// order). Returns true if every synthetic event was accepted.
fn send_combo(modifiers: &[VIRTUAL_KEY], key: VIRTUAL_KEY) -> bool {
    let mut inputs: Vec<INPUT> = Vec::with_capacity(modifiers.len() * 2 + 2);
    for m in modifiers {
        inputs.push(key_event(*m, false));
    }
    inputs.push(key_event(key, false));
    inputs.push(key_event(key, true));
    for m in modifiers.iter().rev() {
        inputs.push(key_event(*m, true));
    }
    let sent = unsafe { SendInput(&inputs, std::mem::size_of::<INPUT>() as i32) };
    sent as usize == inputs.len()
}

fn key_event(vk: VIRTUAL_KEY, keyup: bool) -> INPUT {
    INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: vk,
                wScan: 0,
                dwFlags: if keyup {
                    KEYEVENTF_KEYUP
                } else {
                    KEYBD_EVENT_FLAGS(0)
                },
                time: 0,
                dwExtraInfo: 0,
            },
        },
    }
}

/// Show a desktop notification; best-effort.
///
/// TODO(windows): surface a real WinRT toast. For now the desktop overlay is
/// the primary status surface, so we log to stderr for the CLI daemon path.
pub fn notify(summary: &str, body: &str) {
    eprintln!("[notify] {summary}: {body}");
}

/// `SendInput` is always available on Windows.
pub fn paste_simulation_available() -> bool {
    true
}
