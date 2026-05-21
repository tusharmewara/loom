#[tokio::main]
async fn main() -> anyhow::Result<()> {
    loom_tui::run().await
}
