fn main() {
    let rt = tokio::runtime::Runtime::new().expect("Failed to create tokio runtime");
    rt.block_on(async {
        // Same resolution the CLI uses: `LOOM_AGENTS_DIR` wins, otherwise
        // `~/.loom`. The previous fallback was a relative `.loom`, which put
        // the daemon in a different directory than the CLI it serves.
        let loom_dir = loom_core::loom_dir();
        let daemon = loomd::Loomd::new(&loom_dir).expect("failed to initialize loomd");
        if let Err(e) = daemon.run().await {
            eprintln!("Daemon error: {}", e);
            std::process::exit(1);
        }
    });
}
