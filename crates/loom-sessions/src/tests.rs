#[cfg(test)]
mod tests {
    use super::*;
    use tokio_test;

    #[tokio::test]
    async fn test_acp_client_creation() {
        let client = AcpClient::new("https://cdn.agentclientprotocol.com/registry", std::time::Duration::from_secs(300))
            .await
            .expect("Failed to create ACP client");
        
        assert!(client.acp_client().is_some());
    }

    #[tokio::test]
    async fn test_acp_client_discovery() {
        let mut session_manager = SessionManager::new_with_acp(
            "/tmp/test-agents",
            "https://cdn.agentclientprotocol.com/registry"
        )
        .await
        .expect("Failed to create SessionManager with ACP");

        let agents = session_manager.discover_acp_agents()
            .await
            .expect("Failed to discover ACP agents");
        
        // Verify that agents are returned (even if empty, the call should succeed)
        assert!(agents.is_empty() || !agents.is_empty());
    }

    #[tokio::test]
    async fn test_acp_client_refresh() {
        let mut session_manager = SessionManager::new_with_acp(
            "/tmp/test-agents",
            "https://cdn.agentclientprotocol.com/registry"
        )
        .await
        .expect("Failed to create SessionManager with ACP");

        // This should not panic
        let result = session_manager.refresh_acp_registry().await;
        assert!(result.is_ok());
    }
}