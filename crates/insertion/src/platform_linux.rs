//! Linux keystroke injection and notifications.
//!
//! On GNOME Wayland, XTEST can report success while the native focused app
//! ignores it, so real uinput via `ydotool` is the primary path and XTEST is
//! only a fallback.

use std::process::Command;

use anyhow::{Context, Result};

/// Send a synthetic Ctrl+V to the focused window.
pub fn send_paste() -> bool {
    // key codes: 29 = LEFTCTRL, 47 = V; :1 press, :0 release
    if ydotool_key(&["29:1", "47:1", "47:0", "29:0"]) {
        return true;
    }
    xtest_combo(&[KEYSYM_CONTROL_L], KEYSYM_V).is_ok()
}

pub fn send_copy() -> bool {
    // key codes: 29 = LEFTCTRL, 46 = C; :1 press, :0 release
    if ydotool_key(&["29:1", "46:1", "46:0", "29:0"]) {
        return true;
    }
    xtest_combo(&[KEYSYM_CONTROL_L], KEYSYM_C).is_ok()
}

pub fn send_copy_insert() -> bool {
    // key codes: 29 = LEFTCTRL, 110 = INSERT; :1 press, :0 release
    if ydotool_key(&["29:1", "110:1", "110:0", "29:0"]) {
        return true;
    }
    xtest_combo(&[KEYSYM_CONTROL_L], KEYSYM_INSERT).is_ok()
}

pub fn send_terminal_copy() -> bool {
    // key codes: 29 = LEFTCTRL, 42 = LEFTSHIFT, 46 = C; :1 press, :0 release
    if ydotool_key(&["29:1", "42:1", "46:1", "46:0", "42:0", "29:0"]) {
        return true;
    }
    xtest_combo(&[KEYSYM_CONTROL_L, KEYSYM_SHIFT_L], KEYSYM_C).is_ok()
}

/// Hint shown when selection capture fails, if a helper tool is missing.
pub fn selection_capture_hint() -> Option<String> {
    if !ydotool_installed() {
        return Some(
            "Wayland selection capture needs ydotool; install it with: sudo apt install ydotool"
                .into(),
        );
    }
    None
}

fn ydotool_key(keys: &[&str]) -> bool {
    let mut args = vec!["key"];
    args.extend_from_slice(keys);
    Command::new("ydotool")
        .args(args)
        .output()
        .map(|out| out.status.success())
        .unwrap_or(false)
}

fn ydotool_installed() -> bool {
    Command::new("ydotool")
        .arg("--help")
        .output()
        .map(|out| out.status.success())
        .unwrap_or(false)
}

const KEYSYM_C: u32 = 0x0063;
const KEYSYM_V: u32 = 0x0076;
const KEYSYM_INSERT: u32 = 0xFF63;
const KEYSYM_CONTROL_L: u32 = 0xFFE3;
const KEYSYM_SHIFT_L: u32 = 0xFFE1;

/// Inject modifiers + key through the XTEST extension.
fn xtest_combo(modifier_keysyms: &[u32], key_keysym: u32) -> Result<()> {
    use x11rb::connection::Connection as _;
    use x11rb::protocol::xproto::ConnectionExt as _;
    use x11rb::protocol::xtest::ConnectionExt as _;
    use x11rb::rust_connection::RustConnection;

    const KEY_PRESS: u8 = 2;
    const KEY_RELEASE: u8 = 3;

    let (conn, screen_num) = x11rb::connect(None).context("no X display")?;
    let setup = conn.setup();
    let (min_kc, max_kc) = (setup.min_keycode, setup.max_keycode);
    let root = setup.roots[screen_num].root;

    let mapping = conn
        .get_keyboard_mapping(min_kc, max_kc - min_kc + 1)?
        .reply()
        .context("keyboard mapping failed")?;
    let per = mapping.keysyms_per_keycode as usize;
    let find_keycode = |keysym: u32| -> Option<u8> {
        mapping
            .keysyms
            .chunks(per)
            .position(|syms| syms.contains(&keysym))
            .map(|i| min_kc + i as u8)
    };
    let modifiers = modifier_keysyms
        .iter()
        .map(|keysym| find_keycode(*keysym).context("no requested modifier key"))
        .collect::<Result<Vec<_>>>()?;
    let key = find_keycode(key_keysym).context("no requested key")?;

    let fake = |conn: &RustConnection, kind: u8, keycode: u8| -> Result<()> {
        conn.xtest_fake_input(kind, keycode, x11rb::CURRENT_TIME, root, 0, 0, 0)?;
        Ok(())
    };
    for modifier in &modifiers {
        fake(&conn, KEY_PRESS, *modifier)?;
    }
    fake(&conn, KEY_PRESS, key)?;
    fake(&conn, KEY_RELEASE, key)?;
    for modifier in modifiers.iter().rev() {
        fake(&conn, KEY_RELEASE, *modifier)?;
    }
    conn.flush()?;
    Ok(())
}

/// Show a desktop notification; best-effort.
pub fn notify(summary: &str, body: &str) {
    let _ = Command::new("notify-send")
        .args(["-a", "FlowOSS", "-t", "2500", summary, body])
        .spawn();
}

/// True if a ydotool daemon appears to be available for paste simulation.
pub fn paste_simulation_available() -> bool {
    ydotool_installed()
}
