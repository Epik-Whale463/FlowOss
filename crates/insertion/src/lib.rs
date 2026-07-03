//! Text insertion into the active app (PRD 11.6).
//!
//! Clipboard-first, per the Wayland baseline: set the clipboard, then try to
//! simulate a paste keystroke if a helper is available (`ydotool`). If paste
//! simulation is unavailable or fails, the text stays in the clipboard and
//! the caller should tell the user to press Ctrl+V.

use std::process::Command;

use anyhow::{Context, Result};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PasteMode {
    /// Copy to clipboard and attempt a simulated Ctrl+V.
    #[default]
    Auto,
    /// Copy to clipboard only.
    Copy,
}

impl std::str::FromStr for PasteMode {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_ascii_lowercase().as_str() {
            "auto" => Ok(Self::Auto),
            "copy" => Ok(Self::Copy),
            other => Err(format!("unknown paste mode: {other} (expected auto|copy)")),
        }
    }
}

/// What actually happened when inserting text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InsertOutcome {
    /// Clipboard set and paste keystroke sent.
    Pasted,
    /// Clipboard set; user must paste manually.
    Copied,
}

/// OS clipboard handle. Keep this alive for as long as the copied text
/// should stay available (on Wayland/X11 without a clipboard manager, the
/// contents are served by this process).
pub struct Inserter {
    clipboard: arboard::Clipboard,
}

impl Inserter {
    pub fn new() -> Result<Self> {
        let clipboard = arboard::Clipboard::new()
            .map_err(|e| anyhow::anyhow!("failed to open clipboard: {e}"))?;
        Ok(Self { clipboard })
    }

    pub fn insert(&mut self, text: &str, mode: PasteMode) -> Result<InsertOutcome> {
        self.clipboard
            .set_text(text)
            .map_err(|e| anyhow::anyhow!("failed to set clipboard: {e}"))?;
        if mode == PasteMode::Auto {
            // Give the clipboard offer time to propagate and the user time
            // to release the hotkey modifiers before we type Ctrl+V.
            std::thread::sleep(std::time::Duration::from_millis(150));
            if send_paste_keystroke() {
                return Ok(InsertOutcome::Pasted);
            }
        }
        Ok(InsertOutcome::Copied)
    }
}

/// Send a synthetic Ctrl+V to the focused window. Tries XTEST first (works
/// under XWayland — GNOME routes it to native Wayland apps too), then
/// ydotool. Returns false if neither is available.
fn send_paste_keystroke() -> bool {
    #[cfg(target_os = "linux")]
    if xtest_paste().is_ok() {
        return true;
    }
    // Fallback: ydotool (uinput; needs ydotoold running).
    // key codes: 29 = LEFTCTRL, 47 = V; :1 press, :0 release
    Command::new("ydotool")
        .args(["key", "29:1", "47:1", "47:0", "29:0"])
        .output()
        .map(|out| out.status.success())
        .unwrap_or(false)
}

/// Inject Ctrl+V through the XTEST extension.
#[cfg(target_os = "linux")]
fn xtest_paste() -> Result<()> {
    use x11rb::connection::Connection as _;
    use x11rb::protocol::xproto::ConnectionExt as _;
    use x11rb::protocol::xtest::ConnectionExt as _;
    use x11rb::rust_connection::RustConnection;

    const KEY_PRESS: u8 = 2;
    const KEY_RELEASE: u8 = 3;
    const KEYSYM_CONTROL_L: u32 = 0xFFE3;
    const KEYSYM_V: u32 = 0x0076;

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
    let ctrl = find_keycode(KEYSYM_CONTROL_L).context("no Control key")?;
    let v = find_keycode(KEYSYM_V).context("no V key")?;

    let fake = |conn: &RustConnection, kind: u8, keycode: u8| -> Result<()> {
        conn.xtest_fake_input(kind, keycode, x11rb::CURRENT_TIME, root, 0, 0, 0)?;
        Ok(())
    };
    fake(&conn, KEY_PRESS, ctrl)?;
    fake(&conn, KEY_PRESS, v)?;
    fake(&conn, KEY_RELEASE, v)?;
    fake(&conn, KEY_RELEASE, ctrl)?;
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
    Command::new("ydotool")
        .arg("--help")
        .output()
        .context("")
        .map(|out| out.status.success())
        .unwrap_or(false)
}
