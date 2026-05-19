use anyhow::Result;
use async_trait::async_trait;

#[async_trait]
pub trait AgentProvider: Send + Sync {
    /// Returns provider name for identification.
    fn name(&self) -> String;
    /// Returns metadata about available features.
    fn discover_features(&self) -> Result<Vec<FeatureMetadata>>;
    /// Creates a new session handle for this agent.
    async fn create_session(&self) -> Result<SessionHandle>;
    /// Validates configuration and connectivity.
    fn validate(&self) -> Result<()>;
}

/// Lightweight metadata for agent features.
#[derive(Debug, Clone)]
pub struct FeatureMetadata {
    pub name: String,
    pub category: String,
    pub description: String,
    pub capabilities: Vec<String>,
}

/// Handle to an active agent session.
#[derive(Debug)]
pub struct SessionHandle {
    pub session_id: String,
    pub agent_name: String,
    pub capabilities: Vec<String>,
    pub created_at: std::time::SystemTime,
}

/// Default implementation for simple providers.
pub struct SimpleAgentProvider {
    name: String,
    features: Vec<FeatureMetadata>,
    session_factory: Box<dyn Fn() -> Result<SessionHandle> + Send + Sync>,
}

impl SimpleAgentProvider {
    pub fn new<F>(name: String, features: Vec<FeatureMetadata>, session_factory: F) -> Self
    where
        F: Fn() -> Result<SessionHandle> + Send + Sync + 'static,
    {
        Self {
            name,
            features,
            session_factory: Box::new(session_factory),
        }
    }
}

#[async_trait]
impl AgentProvider for SimpleAgentProvider {
    fn name(&self) -> String {
        self.name.clone()
    }

    fn discover_features(&self) -> Result<Vec<FeatureMetadata>> {
        Ok(self.features.clone())
    }

    async fn create_session(&self) -> Result<SessionHandle> {
        (self.session_factory)()
    }

    fn validate(&self) -> Result<()> {
        Ok(())
    }
}