pub mod types;
pub mod config;
pub mod constants;
pub mod error;
pub mod plugin_registry;

pub use types::*;
pub use config::*;
pub use constants::*;
pub use error::*;

use anyhow::Result;

/// Health status reported by all crates, enabling unified monitoring.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct HealthStatus {
    pub crate_name: &'static str,
    pub version: &'static str,
    pub uptime_seconds: u64,
    pub last_error: Option<String>,
    pub performance_counters: std::collections::HashMap<String, u64>,
}

/// Every crate implements this trait for daemon health monitoring.
pub trait HealthCheck {
    fn health(&self) -> Result<HealthStatus>;
}
