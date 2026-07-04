//! Dev utility: set clipboard text, then read it back from the same process.
fn main() {
    let mut cb = arboard::Clipboard::new().expect("open clipboard");
    cb.set_text("flowoss-roundtrip-test").expect("set clipboard");
    std::thread::sleep(std::time::Duration::from_millis(300));
    let back = cb.get_text().expect("get clipboard");
    println!("roundtrip: {back}");
}
