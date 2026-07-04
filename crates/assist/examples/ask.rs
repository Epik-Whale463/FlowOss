// Manual end-to-end probe of the tool loop:
// cargo run -p flowoss-assist --example ask -- <model>
fn main() {
    let model = std::env::args().nth(1).unwrap_or_default();
    let config = flowoss_assist::AssistConfig {
        provider: flowoss_assist::Provider::Ollama,
        model,
        base_url: String::new(),
        api_key: String::new(),
        web_search: true,
    };
    let progress = |status: &str| eprintln!("[progress] {status}");
    match flowoss_assist::ask(
        &config,
        "The Linux kernel is the core of the Linux operating system.",
        "what is the latest stable kernel version right now",
        &progress,
    ) {
        Ok(answer) => {
            println!("ANSWER: {}", answer.text);
            for s in answer.sources {
                println!("SOURCE: {} — {}", s.title, s.url);
            }
        }
        Err(e) => eprintln!("FAILED: {e}"),
    }
}
