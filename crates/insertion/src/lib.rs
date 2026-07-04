//! Text insertion into the active app (PRD 11.6).
//!
//! Clipboard-first: set the clipboard, then try to simulate a paste keystroke.
//! On Linux the paste helper is `ydotool`/XTEST; on Windows it is a synthetic
//! `SendInput` Ctrl+V. If paste simulation is unavailable or fails, the text
//! stays in the clipboard and the caller should tell the user to press Ctrl+V.

use anyhow::Result;

#[cfg_attr(target_os = "linux", path = "platform_linux.rs")]
#[cfg_attr(windows, path = "platform_windows.rs")]
#[cfg_attr(not(any(target_os = "linux", windows)), path = "platform_stub.rs")]
mod platform;

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
            if platform::send_paste() {
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
        // If a modifier is still held, many apps treat this as a different
        // shortcut.
        std::thread::sleep(std::time::Duration::from_millis(350));
        let attempts: [fn() -> bool; 3] = [
            platform::send_copy,
            platform::send_copy_insert,
            platform::send_terminal_copy,
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
            if let Some(hint) = platform::selection_capture_hint() {
                anyhow::bail!("{hint}");
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

/// Show a desktop notification; best-effort.
pub fn notify(summary: &str, body: &str) {
    platform::notify(summary, body);
}

/// True if paste simulation is available (a paste helper on Linux, always on
/// Windows via `SendInput`).
pub fn paste_simulation_available() -> bool {
    platform::paste_simulation_available()
}
