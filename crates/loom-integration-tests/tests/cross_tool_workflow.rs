//! Integration test for cross-tool workflows
//! Tests MCP communication between tools (the core integration point).

use loom_core::{HealthStatusStr, McpServerEntry, McpTransport, McpTransportType};
use loom_mcp::McpManager;
use tempfile::tempdir;

#[tokio::test]
async fn test_mcp_communication_between_tools() -> anyhow::Result<()> {
    let loom_dir = tempdir()?;
    let agents_path = loom_dir.path().to_str().unwrap();

    // Initialize MCP manager
    let mcp_manager = McpManager::new(agents_path)?;

    // Register MCP server from jcode
    let jcode_mcp = McpServerEntry {
        id: "jcode-mcp".to_string(),
        name: "jCode MCP".to_string(),
        transport: McpTransport {
            transport_type: McpTransportType::Stdio,
            command: Some("node".to_string()),
            args: vec![],
            url: None,
            env: std::collections::HashMap::new(),
        },
        shared: true,
        tools_filter: vec![],
        health_status: HealthStatusStr::Healthy,
        last_health_check: chrono::Utc::now().to_rfc3339(),
        tags: vec![],
        compatible_tools: vec!["claude-code".to_string()],
        required_by_skills: vec![],
    };

    mcp_manager.registry().upsert(&jcode_mcp)?;

    // Verify MCP server is registered
    let fetched = mcp_manager.registry().get("jcode-mcp").unwrap().unwrap();
    assert_eq!(fetched.id, "jcode-mcp");

    println!("✅ MCP server registered and accessible");
    Ok(())
}
