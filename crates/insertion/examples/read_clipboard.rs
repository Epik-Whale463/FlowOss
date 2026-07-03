//! Dev utility: print current clipboard text (for testing insertion).
fn main() {
    match arboard::Clipboard::new().and_then(|mut c| c.get_text()) {
        Ok(text) => println!("{text}"),
        Err(e) => {
            eprintln!("clipboard read failed: {e}");
            std::process::exit(1);
        }
    }
}
