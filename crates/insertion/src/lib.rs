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
        if mode == PasteMode::Auto && send_paste_keystroke() {
            Ok(InsertOutcome::Pasted)
        } else {
            Ok(InsertOutcome::Copied)
        }
    }
}

/// Try to send Ctrl+V via ydotool (works on Wayland through uinput, needs
/// ydotoold running). Returns false if unavailable or it failed.
fn send_paste_keystroke() -> bool {
    // key codes: 29 = LEFTCTRL, 47 = V; :1 press, :0 release
    Command::new("ydotool")
        .args(["key", "29:1", "47:1", "47:0", "29:0"])
        .output()
        .map(|out| out.status.success())
        .unwrap_or(false)
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
