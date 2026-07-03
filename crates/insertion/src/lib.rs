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

    /// Capture the currently selected text by copying it through the focused
    /// app, then restore the user's previous clipboard contents.
    pub fn capture_selection(&mut self) -> Result<String> {
        let previous = self.clipboard.get_text().ok();
        let sentinel = format!(
            "__FLOWOSS_SELECTION_SENTINEL_{}_{}__",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
        );
        self.clipboard
            .set_text(sentinel.clone())
            .map_err(|e| anyhow::anyhow!("failed to prepare clipboard: {e}"))?;

        // Give the user time to release the global shortcut before Ctrl+C.
        // If Super is still held, many apps treat this as a different shortcut.
        std::thread::sleep(std::time::Duration::from_millis(350));
        let attempts: [fn() -> bool; 3] = [
            send_copy_keystroke,
            send_copy_insert_keystroke,
            send_terminal_copy_keystroke,
        ];
        let mut captured = String::new();
        let mut sent_any = false;
        for attempt in attempts {
            sent_any |= attempt();
            captured = wait_for_clipboard_text(&mut self.clipboard, &sentinel);
            if captured != sentinel && !captured.trim().is_empty() {
                break;
            }
        }

        restore_clipboard(&mut self.clipboard, previous);
        if !sent_any {
            anyhow::bail!("could not send a copy shortcut to capture the selection");
        }
        if captured == sentinel || captured.trim().is_empty() {
            if !ydotool_installed() {
                anyhow::bail!("Wayland selection capture needs ydotool; install it with: sudo apt install ydotool");
            }
            anyhow::bail!("selected text could not be copied; try clicking the app once, selecting text, then pressing assist again");
        }
        Ok(captured)
    }
}

fn wait_for_clipboard_text(clipboard: &mut arboard::Clipboard, sentinel: &str) -> String {
    let deadline = std::time::Instant::now() + std::time::Duration::from_millis(900);
    let mut captured = String::new();
    while std::time::Instant::now() < deadline {
        std::thread::sleep(std::time::Duration::from_millis(50));
        captured = clipboard.get_text().unwrap_or_default();
        if captured != sentinel && !captured.trim().is_empty() {
            break;
        }
    }
    captured
}

fn restore_clipboard(clipboard: &mut arboard::Clipboard, previous: Option<String>) {
    let _ = clipboard.set_text(previous.unwrap_or_default());
}

/// Send a synthetic Ctrl+V to the focused window. On GNOME Wayland, XTEST can
/// report success while the native focused app ignores it, so real uinput via
/// ydotool is the primary path and XTEST is only a fallback.
fn send_paste_keystroke() -> bool {
    // key codes: 29 = LEFTCTRL, 47 = V; :1 press, :0 release
    if ydotool_key(&["29:1", "47:1", "47:0", "29:0"]) {
        return true;
    }
    #[cfg(target_os = "linux")]
    if xtest_hotkey(KEYSYM_V).is_ok() {
        return true;
    }
    false
}

fn send_copy_keystroke() -> bool {
    // key codes: 29 = LEFTCTRL, 46 = C; :1 press, :0 release
    if ydotool_key(&["29:1", "46:1", "46:0", "29:0"]) {
        return true;
    }
    #[cfg(target_os = "linux")]
    if xtest_combo(&[KEYSYM_CONTROL_L], KEYSYM_C).is_ok() {
        return true;
    }
    false
}

fn send_copy_insert_keystroke() -> bool {
    // key codes: 29 = LEFTCTRL, 110 = INSERT; :1 press, :0 release
    if ydotool_key(&["29:1", "110:1", "110:0", "29:0"]) {
        return true;
    }
    #[cfg(target_os = "linux")]
    if xtest_combo(&[KEYSYM_CONTROL_L], KEYSYM_INSERT).is_ok() {
        return true;
    }
    false
}

fn send_terminal_copy_keystroke() -> bool {
    // key codes: 29 = LEFTCTRL, 42 = LEFTSHIFT, 46 = C; :1 press, :0 release
    if ydotool_key(&["29:1", "42:1", "46:1", "46:0", "42:0", "29:0"]) {
        return true;
    }
    #[cfg(target_os = "linux")]
    if xtest_combo(&[KEYSYM_CONTROL_L, KEYSYM_SHIFT_L], KEYSYM_C).is_ok() {
        return true;
    }
    false
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

#[cfg(target_os = "linux")]
const KEYSYM_C: u32 = 0x0063;
#[cfg(target_os = "linux")]
const KEYSYM_V: u32 = 0x0076;
#[cfg(target_os = "linux")]
const KEYSYM_INSERT: u32 = 0xFF63;
#[cfg(target_os = "linux")]
const KEYSYM_CONTROL_L: u32 = 0xFFE3;
#[cfg(target_os = "linux")]
const KEYSYM_SHIFT_L: u32 = 0xFFE1;

/// Inject Ctrl+<key> through the XTEST extension.
#[cfg(target_os = "linux")]
fn xtest_hotkey(keysym: u32) -> Result<()> {
    xtest_combo(&[KEYSYM_CONTROL_L], keysym)
}

/// Inject modifiers + key through the XTEST extension.
#[cfg(target_os = "linux")]
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
    Command::new("ydotool")
        .arg("--help")
        .output()
        .context("")
        .map(|out| out.status.success())
        .unwrap_or(false)
}
