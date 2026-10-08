///! Integration test for MCP health monitoring.
///! Tests that MCP health monitoring works correctly, including status tracking.

use loom_core::{HealthStatusStr, McpServerEntry};
use loom_mcp::McpHealthChecker;
use std::time::Duration;
use tokio::time::sleep;

#[tokio::test]
async fn test_mcp_health_monitor_tracks_status_changes() -> anyhow::Result<()> {
    let checker = loom_mcp::McpHealthChecker::with_defaults();

    let server_entry = McpServerEntry {
        id: "health-test-server".to_string(),
        name: "Health Test Server".to_string(),
        transport: loom_core::McpTransport {
            transport_type: loom_core::McpTransportType::Stdio,
            command: Some("true".to_string()),
            args: vec![],
            url: None,
            env: std::collections::HashMap::new(),
        },
        shared: true,
        tools_filter: vec![],
        health_status: HealthStatusStr::Unknown,
        last_health_check: String::new(),
        tags: vec![],
        compatible_tools: vec!["test-tool".to_string()],
        required_by_skills: vec![],
    };

    // Register server
    checker.register(&server_entry).await;

    // Initial status should be Unknown
    let status = checker.status_of("health-test-server").await;
    assert!(status.is_some());
    assert_eq!(status.unwrap(), HealthStatusStr::Unknown);

    // Manually trigger a health check
    let status_after = checker.check_one("health-test-server").await;
    assert!(status_after.is_some());
    // After a single check, status should become Healthy
    assert_eq!(status_after.unwrap(), HealthStatusStr::Healthy);

    println!("✅ MCP health monitoring correctly tracks status changes");
    Ok(())
}

#[tokio::test]
async fn test_mcp_health_monitor_multiple_servers() -> anyhow::Result<()> {
    let checker = loom_mcp::McpHealthChecker::with_defaults();

    let servers = vec![
        McpServerEntry {
            id: "multi-server-1".to_string(),
            name: "Multi Server 1".to_string(),
            transport: loom_core::McpTransport {
                transport_type: loom_core::McpTransportType::Stdio,
                command: Some("true".to_string()),
                args: vec![],
                url: None,
                env: std::collections::HashMap::new(),
            },
            shared: true,
            tools_filter: vec![],
            health_status: HealthStatusStr::Unknown,
            last_health_check: String::new(),
            tags: vec![],
            compatible_tools: vec![],
            required_by_skills: vec![],
        },
        McpServerEntry {
            id: "multi-server-2".to_string(),
            name: "Multi Server 2".to_string(),
            transport: loom_core::McpTransport {
                transport_type: loom_core::McpTransportType::Stdio,
                command: Some("true".to_string()),
                args: vec![],
                url: None,
                env: std::collections::HashMap::new(),
            },
            shared: true,
            tools_filter: vec![],
            health_status: HealthStatusStr::Unknown,
            last_health_check: String::new(),
            tags: vec![],
            compatible_tools: vec![],
            required_by_skills: vec![],
        },
    ];

    // Register servers
    for server in &servers {
        checker.register(server).await;
    }

    // Check all are registered
    let snapshot = checker.status_snapshot().await;
    assert_eq!(snapshot.len(), 2);

    // Check all are Healthy after checks
    for server in &servers {
        let status = checker.check_one(&server.id).await;
        assert!(status.is_some(), "Server {} should be checkable", server.id);
        assert_eq!(status.unwrap(), HealthStatusStr::Healthy, "Server {} should be Healthy after check", server.id);
    }

    println!("✅ MCP health monitoring correctly handles multiple servers");
    Ok(())
}

#[tokio::test]
async fn test_mcp_health_monitor_unregister_removes_from_tracking() -> anyhow::Result<()> {
    let checker = loom_mcp::McpHealthChecker::with_defaults();

    let server_entry = McpServerEntry {
        id: "unregister-test-server".to_string(),
        name: "Unregister Test Server".to_string(),
        transport: loom_core::McpTransport {
            transport_type: loom_core::McpTransportType::Stdio,
            command: Some("true".to_string()),
            args: vec![],
            url: None,
            env: std::collections::HashMap::new(),
        },
        shared: true,
        tools_filter: vec![],
        health_status: HealthStatusStr::Unknown,
        last_health_check: String::new(),
        tags: vec![],
        compatible_tools: vec![],
        required_by_skills: vec![],
    };

    checker.register(&server_entry).await;

    // Verify registered
    let status_before = checker.status_of("unregister-test-server").await;
    assert!(status_before.is_some(), "Server should be found before unregistering");

    // Unregister
    checker.unregister("unregister-test-server").await;

    // Verify unregistered
    let status_after = checker.status_of("unregister-test-server").await;
    assert!(status_after.is_none(), "Server should not be found after unregistering");

    println!("✅ MCP health monitoring correctly unregisters servers");
    Ok(())
}

#[tokio::test]
async fn test_mcp_health_monitor_status_snapshot() -> anyhow::Result<()> {
    let checker = loom_mcp::McpHealthChecker::with_defaults();

    let server_entry = McpServerEntry {
        id: "snapshot-test-server".to_string(),
        name: "Snapshot Test Server".to_string(),
        transport: loom_core::McpTransport {
            transport_type: loom_core::McpTransportType::Stdio,
            command: Some("true".to_string()),
            args: vec![],
            url: None,
            env: std::collections::HashMap::new(),
        },
        shared: true,
        tools_filter: vec![],
        health_status: HealthStatusStr::Healthy,
        last_health_check: String::new(),
        tags: vec![],
        compatible_tools: vec![],
        required_by_skills: vec![],
    };

    checker.register(&server_entry).await;

    // Status snapshot should contain the registered server
    let snapshot = checker.status_snapshot().await;
    assert_eq!(snapshot.len(), 1);
    let snapshot_entry = snapshot.iter().next().unwrap();
    assert_eq!(snapshot_entry.0, "snapshot-test-server");

    println!("✅ MCP health monitoring status snapshot works correctly");
    Ok(())
}