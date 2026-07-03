// Quick manual probe: cargo run -p flowoss-assist --example probe -- "query"
fn main() {
    let query = std::env::args().nth(1).unwrap_or_else(|| "rust 1.88 release notes".into());
    match flowoss_assist::debug_search(&query) {
        Ok(hits) => {
            for (title, url, snippet) in hits {
                println!("• {title}\n  {url}\n  {snippet}\n");
            }
        }
        Err(e) => eprintln!("search failed: {e}"),
    }
}
