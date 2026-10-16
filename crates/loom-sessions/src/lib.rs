use loom_core::HealthCheck;
use std::collections::HashMap;

pub mod agent_provider;
pub mod compression;
pub mod environment;
pub mod exporter;
pub mod importer;
pub mod memory;
pub mod migrate;
pub mod store;

pub use exporter::{export_session, write_export, SessionExport};
pub use importer::{import_session, import_session_json};
pub use migrate::migrate_if_needed;

pub mod acp_client;
pub mod acp_discovery;
pub mod acp_handshake;
pub mod acp_protocol;
pub mod acp_server;
pub use acp_client::AcpClient;
pub use acp_client::AgentMetadata;
pub use acp_discovery::{AcpPeer, PeerDiscovery};
pub use acp_handshake::{
    complete_initiator, initiate_handshake, perform_responder_handshake, AcpHandshake, AcpKeyPair,
};
pub use acp_server::{
    AcpDispatchServer, AcpServerConfig, DispatchFn, DispatchRequest, DispatchResponse,
};

pub struct SessionManager {
    loom_dir: String,
    store: store::SessionStore,
    acp_client: Option<AcpClient>,
    custom_agents: HashMap<String, Box<dyn agent_provider::AgentProvider>>,
}

impl SessionManager {
    pub fn new(loom_dir: &str) -> Self {
        Self {
            loom_dir: loom_dir.to_string(),
            store: store::SessionStore::new(loom_dir),
            acp_client: None,
            custom_agents: Default::default(),
        }
    }

    pub async fn new_with_acp(loom_dir: &str, registry_url: &str) -> anyhow::Result<Self> {
        let client = AcpClient::new(registry_url, std::time::Duration::from_secs(300)).await?;
        Ok(Self {
            loom_dir: loom_dir.to_string(),
            store: store::SessionStore::new(loom_dir),
            acp_client: Some(client),
            custom_agents: Default::default(),
        })
    }

    pub fn add_custom_agent(&mut self, name: &str, agent: Box<dyn agent_provider::AgentProvider>) {
        self.custom_agents.insert(name.to_string(), agent);
    }

    pub fn loom_dir(&self) -> &str {
        &self.loom_dir
    }

    pub fn store(&self) -> &store::SessionStore {
        &self.store
    }

    pub fn acp_client(&self) -> Option<&AcpClient> {
        self.acp_client.as_ref()
    }

    pub fn custom_agents(&self) -> &HashMap<String, Box<dyn agent_provider::AgentProvider>> {
        &self.custom_agents
    }

    /// Run legacy JSON → JSONL migration.
    pub fn migrate(&self) -> anyhow::Result<usize> {
        migrate::migrate_if_needed(&self.loom_dir)
    }

    pub async fn discover_acp_agents(&self) -> anyhow::Result<Vec<AgentMetadata>> {
        if let Some(client) = &self.acp_client {
            client
                .discover_agents()
                .await
                .map_err(|e| anyhow::anyhow!(e))
        } else {
            Err(anyhow::anyhow!("ACP client not configured"))
        }
    }

    /// List all agent names (ACP + custom).
    pub async fn list_all_agents(&self) -> anyhow::Result<Vec<String>> {
        let mut names = Vec::new();

        // Add ACP agent IDs
        if let Ok(acp_agents) = self.discover_acp_agents().await {
            names.extend(acp_agents.into_iter().map(|a| a.id));
        }

        // Add custom agent names
        names.extend(self.custom_agents.keys().cloned());

        Ok(names)
    }

    /// Refresh the ACP registry cache.
    pub async fn refresh_acp_registry(&self) -> anyhow::Result<()> {
        if let Some(client) = &self.acp_client {
            client.refresh().await.map_err(|e| anyhow::anyhow!(e))
        } else {
            Err(anyhow::anyhow!("ACP client not configured"))
        }
    }
}

impl HealthCheck for SessionManager {
    fn health(&self) -> anyhow::Result<loom_core::HealthStatus> {
        let count = self.store.list().len();
        Ok(loom_core::HealthStatus {
            crate_name: "loom-sessions",
            version: env!("CARGO_PKG_VERSION"),
            uptime_seconds: 0,
            last_error: None,
            performance_counters: {
                let mut m = std::collections::HashMap::new();
                m.insert("sessions".into(), count as u64);
                m
            },
        })
    }
}
