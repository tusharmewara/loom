use anyhow::Result;
use reqwest::Client;
use serde::Deserialize;
use serde_json::Value;
use std::sync::Arc;
use std::time::{Duration, SystemTime};
use tokio::sync::RwLock;

#[derive(Debug, Deserialize, Clone)]
pub struct AgentMetadata {
    pub id: String,
    pub name: String,
    pub description: String,
    #[serde(default)]
    pub auth_methods: Vec<String>,
    pub distribution: Value,
    #[serde(default)]
    pub platforms: Vec<String>,
    #[serde(default)]
    pub tags: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct RegistryResponse {
    agents: Vec<AgentMetadata>,
}

pub struct AcpClient {
    client: Client,
    base_url: String,
    cache: Arc<RwLock<Option<Vec<AgentMetadata>>>>,
    last_refresh: Arc<RwLock<SystemTime>>,
    refresh_interval: Duration,
}

impl AcpClient {
    pub async fn new(registry_url: &str, refresh_interval: Duration) -> Result<Self> {
        let client = Client::new();
        let base_url = registry_url.to_string();
        Ok(Self {
            client,
            base_url,
            cache: Arc::new(RwLock::new(None)),
            last_refresh: Arc::new(RwLock::new(SystemTime::UNIX_EPOCH)),
            refresh_interval,
        })
    }

    async fn needs_refresh(&self) -> bool {
        let last = *self.last_refresh.read().await;
        last.elapsed()
            .unwrap_or_else(|_| Duration::from_secs(u64::MAX))
            > self.refresh_interval
    }

    pub async fn discover_agents(&self) -> Result<Vec<AgentMetadata>> {
        if self.needs_refresh().await {
            let url = format!("{}/v1/latest/registry.json", self.base_url);
            let response = self.client.get(&url).send().await?;
            let wrapper: RegistryResponse = response.json().await?;
            let metadata = wrapper.agents;
            let mut cache = self.cache.write().await;
            *cache = Some(metadata.clone());
            *self.last_refresh.write().await = SystemTime::now();
            Ok(metadata)
        } else {
            let cache = self.cache.read().await;
            Ok(cache.clone().unwrap_or_default())
        }
    }

    pub async fn refresh(&self) -> Result<()> {
        let url = format!("{}/v1/latest/registry.json", self.base_url);
        let response = self.client.get(&url).send().await?;
        let wrapper: RegistryResponse = response.json().await?;
        let metadata = wrapper.agents;
        let mut cache = self.cache.write().await;
        *cache = Some(metadata);
        *self.last_refresh.write().await = SystemTime::now();
        Ok(())
    }
}
