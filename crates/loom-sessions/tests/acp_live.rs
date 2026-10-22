use loom_sessions::SessionManager;
use reqwest::Client;

/// Live ACP registry discovery test.
///
/// This test hits the public ACP registry CDN, so it is **skipped by default**:
/// - It only runs when `LOOM_ACP_LIVE=1` is set.
/// - It prints `skipped: ...` and returns successfully when not requested.
#[tokio::test]
async fn test_acp_registry_live_discovery() {
    if std::env::var("LOOM_ACP_LIVE").as_deref() != Ok("1") {
        println!("skipped: LOOM_ACP_LIVE != 1 (live ACP registry test not requested)");
        return;
    }
    // First, dump the raw registry JSON to understand its structure
    let client = Client::new();
    let url = "https://cdn.agentclientprotocol.com/registry/v1/latest/registry.json";
    let resp = client.get(url).send().await.expect("Request failed");
    let text = resp.text().await.expect("Failed to read body");
    println!(
        "Raw registry JSON (first 1000 chars):\n{}",
        &text[..text.len().min(1000)]
    );

    let mgr = SessionManager::new_with_acp(
        "/tmp/loom-acp-test",
        "https://cdn.agentclientprotocol.com/registry",
    )
    .await
    .expect("Failed to create ACP session manager");

    let agents = mgr
        .discover_acp_agents()
        .await
        .expect("Failed to discover ACP agents");

    println!("Discovered {} ACP agents", agents.len());
    if !agents.is_empty() {
        println!("First agent: {} — {}", agents[0].id, agents[0].name);
    }
}
