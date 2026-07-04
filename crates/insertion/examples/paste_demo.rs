//! Manual verification for text insertion (esp. the Windows SendInput path).
//!
//! Run it, then within 3 seconds click into any text field (Notepad, a browser
//! box, etc.). FlowOSS will set the clipboard and simulate Ctrl+V, so the demo
//! text should appear where your cursor is.
//!
//!   cargo run -p flowoss-insertion --example paste_demo
//!   cargo run -p flowoss-insertion --example paste_demo -- copy   # clipboard only

use std::time::Duration;

use flowoss_insertion::{InsertOutcome, Inserter, PasteMode};

fn main() -> anyhow::Result<()> {
    let mode = match std::env::args().nth(1).as_deref() {
        Some("copy") => PasteMode::Copy,
        _ => PasteMode::Auto,
    };

    let text = "FlowOSS insertion works on this platform.";
    println!("paste simulation available: {}", flowoss_insertion::paste_simulation_available());
    println!("Focus a text field now — pasting in 3s ({mode:?})...");
    std::thread::sleep(Duration::from_secs(3));

    let mut inserter = Inserter::new()?;
    match inserter.insert(text, mode)? {
        InsertOutcome::Pasted => println!("✓ pasted: {text}"),
        InsertOutcome::Copied => println!("✓ copied (press Ctrl+V): {text}"),
    }
    Ok(())
}
