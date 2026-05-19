fn main() {
    let rt = tokio::runtime::Runtime::new().expect("Failed to create tokio runtime");
    rt.block_on(async {
        let loom_dir = std::env::var("LOOM_AGENTS_DIR").unwrap_or_else(|_| ".loom".to_string());
        let daemon = loomd::Loomd::new(&loom_dir).expect("failed to initialize loomd");
        if let Err(e) = daemon.run().await {
            eprintln!("Daemon error: {}", e);
            std::process::exit(1);
        }
    });
}