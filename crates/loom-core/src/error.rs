use thiserror::Error;

#[derive(Error, Debug)]
pub enum LoomError {
    #[error("Config error: {0}")]
    Config(String),

    #[error("Discovery error: {0}")]
    Discovery(String),

    #[error("MCP error: {0}")]
    Mcp(String),

    #[error("Skills error: {0}")]
    Skills(String),

    #[error("Auth error: {0}")]
    Auth(String),

    #[error("Session error: {0}")]
    Session(String),

    #[error("Hooks error: {0}")]
    Hooks(String),

    #[error("Workflow error: {0}")]
    Workflow(String),

    #[error("Routing error: {0}")]
    Routing(String),

    #[error("Daemon error: {0}")]
    Daemon(String),

    #[error("Not found: {0}")]
    NotFound(String),

    #[error("Already exists: {0}")]
    AlreadyExists(String),

    #[error("Permission denied: {0}")]
    PermissionDenied(String),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Serialization error: {0}")]
    Serde(#[from] serde_json::Error),

    #[error(transparent)]
    Other(#[from] anyhow::Error),
}
