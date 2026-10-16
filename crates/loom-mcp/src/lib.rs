//! Loom MCP Bridge Crate.
//!
//! Provides a SQLite-backed registry for MCP server entries, a connection
//! pool managed via daemon RPC with reference counting, health checking
//! with event-driven notifications, and auto-import from tool TOML
//! `mcp_config` paths.
//!
//! The registry and pool are designed to share state via the [`RegistryShared`]
//! wrapper, enabling cross-tool process reuse. When multiple tools need the
//! same MCP server, they share the same process instance instead of spawning
//! separate ones.

pub mod agentmemory;
pub mod client;
pub mod health;
pub mod import;
pub mod known;
pub mod pool;
pub mod registry;

use anyhow::Result;
use loom_core::LoomEvent;
use loom_core::{HealthCheck, HealthStatus};
use loom_event_bus::EventBus;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::health::HealthChangeCallback;
use crate::pool::RegistryShared;
use tracing::info;

pub use agentmemory::{
    health_check as agentmemory_health, AgentMemoryManager, AGENTMEMORY_PORT,
    AGENTMEMORY_VIEWER_PORT,
};
pub use client::McpClientPool;
pub use health::{HealthCheckHandle, McpHealthChecker, DEFAULT_HEALTH_INTERVAL};
pub use pool::McpPool;
pub use registry::McpRegistry;

/// Orchestrator for all MCP bridge subsystems.
///
/// Manages the lifecycle of:
/// - [`McpRegistry`] — SQLite-backed server registry
/// - [`McpPool`] — daemon process pool with reference counting
/// - [`McpHealthChecker`] — periodic health checking
/// - [`McpClientPool`] — connection reuse
///
/// The manager coordinates between the registry and pool to enable shared
/// MCP server instances across multiple tools. The shared registry tracks
/// process state and reference counts for cross-tool coordination.
pub struct McpManager {
    /// Root directory for agent metadata (`.loom/`).
    loom_dir: PathBuf,
    /// SQLite-backed registry with shared state tracking.
    registry: McpRegistry,
    /// Shared registry wrapper for cross-component access.
    shared_registry: Arc<RegistryShared>,
    /// Process pool with registry coordination.
    pool: McpPool,
    /// Health checker.
    health_checker: McpHealthChecker,
    /// Client connection pool.
    client_pool: McpClientPool,
    /// Optional event bus for health-change events.
    event_bus: Option<Arc<EventBus>>,
    /// Creation time for uptime tracking.
    started_at: Instant,
    /// Handle to the background health-check loop.
    health_handle: Option<HealthCheckHandle>,
}

impl McpManager {
    /// Create a new `McpManager` with the given `loom_dir`.
    ///
    /// The registry database is stored at `loom_dir / mcp_registry.db`.
    /// The manager initializes a shared registry and coordinates the process
    /// pool to enable cross-tool process reuse.
    pub fn new(loom_dir: &str) -> Result<Self> {
        // Create shared registry first (this seeds defaults)
        let shared_registry = RegistryShared::new(loom_dir)?;
        let registry = shared_registry.inner().clone();
        let pool = McpPool::with_registry(Arc::downgrade(&shared_registry));

        let health_checker = McpHealthChecker::with_defaults();
        let client_pool = McpClientPool::with_defaults();

        Ok(Self {
            loom_dir: PathBuf::from(loom_dir),
            registry,
            shared_registry,
            pool,
            health_checker,
            client_pool,
            event_bus: None,
            started_at: Instant::now(),
            health_handle: None,
        })
    }

    /// Create a new `McpManager` with an event bus for change notifications.
    pub fn with_event_bus(loom_dir: &str, event_bus: Arc<EventBus>) -> Result<Self> {
        // Create shared registry first (this seeds defaults)
        let shared_registry = RegistryShared::new(loom_dir)?;
        let registry = shared_registry.inner().clone();
        let pool = McpPool::with_registry(Arc::downgrade(&shared_registry));

        // Wire up health changes to publish on the event bus
        let on_change: Option<HealthChangeCallback> = {
            let eb = event_bus.clone();
            Some(
                Box::new(move |id: &str, status: &loom_core::HealthStatusStr| {
                    let _ = eb.publish(LoomEvent::McpHealthChanged {
                        id: id.to_string(),
                        status: status.clone(),
                    });
                }) as HealthChangeCallback,
            )
        };
        let health_checker = McpHealthChecker::new(DEFAULT_HEALTH_INTERVAL, on_change);
        let client_pool = McpClientPool::with_defaults();

        Ok(Self {
            loom_dir: PathBuf::from(loom_dir),
            registry,
            shared_registry,
            pool,
            health_checker,
            client_pool,
            event_bus: Some(event_bus),
            started_at: Instant::now(),
            health_handle: None,
        })
    }

    /// Create a new `McpManager` from an existing shared registry.
    /// This is used by the daemon to share state across multiple clients.
    pub fn from_shared_registry(
        loom_dir: &str,
        shared_registry: Arc<RegistryShared>,
        event_bus: Option<Arc<EventBus>>,
    ) -> Result<Self> {
        let registry = shared_registry.inner().clone();
        let pool = McpPool::with_registry(Arc::downgrade(&shared_registry));

        let on_change: Option<HealthChangeCallback> = event_bus.clone().map(|eb| {
            Box::new(move |id: &str, status: &loom_core::HealthStatusStr| {
                let _ = eb.publish(LoomEvent::McpHealthChanged {
                    id: id.to_string(),
                    status: status.clone(),
                });
            }) as HealthChangeCallback
        });
        let health_checker = McpHealthChecker::new(DEFAULT_HEALTH_INTERVAL, on_change);
        let client_pool = McpClientPool::with_defaults();

        Ok(Self {
            loom_dir: PathBuf::from(loom_dir),
            registry,
            shared_registry,
            pool,
            health_checker,
            client_pool,
            event_bus,
            started_at: Instant::now(),
            health_handle: None,
        })
    }

    /// Get a reference to the shared registry for cross-component access.
    pub fn shared_registry(&self) -> &Arc<RegistryShared> {
        &self.shared_registry
    }

    /// Start the background health-check loop.
    ///
    /// All currently registered servers (and any registered later via the
    /// health checker's `register` method) will be periodically probed.
    /// Health changes will be published on the event bus if one was set.
    pub fn start_health_checks(&mut self) {
        if self.health_handle.is_some() {
            tracing::warn!("Health checks already started");
            return;
        }
        let handle = self.health_checker.start();
        self.health_handle = Some(handle);
        info!("McpManager: health checks started");
    }

    /// Stop the background health-check loop.
    pub fn stop_health_checks(&mut self) {
        if let Some(handle) = self.health_handle.take() {
            handle.shutdown();
            info!("McpManager: health checks stopped");
        }
    }

    /// Set a custom health-check interval (only effective if called
    /// before `start_health_checks`, or after a stop/restart cycle).
    pub fn set_health_interval(&mut self, interval: Duration) {
        // Rebuild the checker — preserves the on_change callback if event_bus is set
        let on_change: Option<HealthChangeCallback> = self.event_bus.clone().map(|eb| {
            Box::new(move |id: &str, status: &loom_core::HealthStatusStr| {
                let _ = eb.publish(LoomEvent::McpHealthChanged {
                    id: id.to_string(),
                    status: status.clone(),
                });
            }) as HealthChangeCallback
        });
        self.health_checker = McpHealthChecker::new(interval, on_change);
    }

    // ── Delegated accessors ──────────────────────────────────────────

    /// Access the server registry.
    pub fn registry(&self) -> &McpRegistry {
        &self.registry
    }

    /// Access the process pool.
    pub fn pool(&self) -> &McpPool {
        &self.pool
    }

    /// Access the health checker.
    pub fn health_checker(&self) -> &McpHealthChecker {
        &self.health_checker
    }

    /// Access the client connection pool.
    pub fn client_pool(&self) -> &McpClientPool {
        &self.client_pool
    }

    /// The configured agents directory.
    pub fn loom_dir(&self) -> &Path {
        &self.loom_dir
    }
}

impl HealthCheck for McpManager {
    fn health(&self) -> Result<HealthStatus> {
        let registry_count = self.registry.count().unwrap_or(0);
        let pool_count = self.pool.running_count();
        let uptime = self.started_at.elapsed().as_secs();

        let mut perf = std::collections::HashMap::new();
        perf.insert("registry_count".into(), registry_count as u64);
        perf.insert("pool_running".into(), pool_count as u64);

        Ok(HealthStatus {
            crate_name: "loom-mcp",
            version: env!("CARGO_PKG_VERSION"),
            uptime_seconds: uptime,
            last_error: None,
            performance_counters: perf,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use loom_core::{McpServerEntry, McpTransport, McpTransportType};

    fn sample_entry(id: &str) -> McpServerEntry {
        McpServerEntry {
            id: id.to_string(),
            name: id.to_string(),
            transport: McpTransport {
                transport_type: McpTransportType::Stdio,
                command: Some("cat".to_string()),
                args: vec![],
                url: None,
                env: std::collections::HashMap::new(),
            },
            shared: true,
            tools_filter: vec![],
            health_status: loom_core::HealthStatusStr::Unknown,
            last_health_check: String::new(),
            tags: vec![],
            compatible_tools: vec![],
            required_by_skills: vec![],
        }
    }

    #[test]
    fn test_mcp_manager_new() {
        let dir = tempfile::tempdir().unwrap();
        let mgr = McpManager::new(dir.path().to_str().unwrap()).unwrap();
        assert!(mgr.loom_dir().exists());
        // Verify shared registry was created with defaults
        assert!(mgr.shared_registry().inner().count().unwrap() >= 4);
    }

    #[test]
    fn test_mcp_manager_health() {
        let dir = tempfile::tempdir().unwrap();
        let mgr = McpManager::new(dir.path().to_str().unwrap()).unwrap();
        let h = mgr.health().unwrap();
        assert_eq!(h.crate_name, "loom-mcp");
        assert!(h.performance_counters.contains_key("registry_count"));
        assert!(h.performance_counters.contains_key("pool_running"));
    }

    #[test]
    fn test_mcp_manager_registry_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let mgr = McpManager::new(dir.path().to_str().unwrap()).unwrap();
        let entry = sample_entry("srv-1");

        mgr.registry().upsert(&entry).unwrap();
        let fetched = mgr.registry().get("srv-1").unwrap().unwrap();
        assert_eq!(fetched.id, "srv-1");
    }

    #[test]
    fn test_mcp_manager_pool_acquire_release() {
        let dir = tempfile::tempdir().unwrap();
        let mgr = McpManager::new(dir.path().to_str().unwrap()).unwrap();
        let entry = sample_entry("srv-pool");

        mgr.pool().acquire("srv-pool", &entry).unwrap();
        assert!(mgr.pool().is_running("srv-pool"));

        mgr.pool().release("srv-pool").unwrap();
        assert!(!mgr.pool().is_running("srv-pool"));
    }

    #[test]
    fn test_mcp_manager_client_pool() {
        let dir = tempfile::tempdir().unwrap();
        let mgr = McpManager::new(dir.path().to_str().unwrap()).unwrap();
        let entry = sample_entry("srv-client");

        let conn = mgr.client_pool().acquire(&entry).unwrap();
        assert_eq!(conn.server_id, "srv-client");
        mgr.client_pool().release(conn);
    }

    #[tokio::test]
    async fn test_mcp_manager_health_checks_start_stop() {
        let dir = tempfile::tempdir().unwrap();
        let mut mgr = McpManager::new(dir.path().to_str().unwrap()).unwrap();
        mgr.start_health_checks();
        mgr.stop_health_checks();
    }

    #[test]
    fn test_mcp_manager_with_event_bus() {
        let dir = tempfile::tempdir().unwrap();
        let bus = Arc::new(EventBus::new(dir.path().to_str().unwrap(), 64));
        let mgr = McpManager::with_event_bus(dir.path().to_str().unwrap(), bus).unwrap();
        // Default servers (context-mode, rtk, loom, agentmemory) are seeded on creation
        assert!(mgr.registry().count().unwrap() >= 4);
    }

    #[test]
    fn test_mcp_manager_cross_tool_process_sharing() {
        let dir = tempfile::tempdir().unwrap();
        let mgr1 = McpManager::new(dir.path().to_str().unwrap()).unwrap();
        let entry = sample_entry("shared-srv");

        // First tool acquires the server
        mgr1.pool().acquire("shared-srv", &entry).unwrap();
        assert!(mgr1.pool().is_running("shared-srv"));
        assert_eq!(mgr1.pool().refcount("shared-srv"), 1);

        // Verify registry has shared state
        let proc_state = mgr1
            .shared_registry()
            .inner()
            .get_process_state("shared-srv");
        assert!(proc_state.is_ok());
        let (state, pid) = proc_state.unwrap().unwrap();
        assert_eq!(state, "running");
        assert!(pid.is_some());

        // Verify refcount tracking
        let refcount = mgr1.shared_registry().inner().get_refcount("shared-srv");
        assert_eq!(refcount, 1);
    }

    #[test]
    fn test_mcp_manager_from_shared_registry() {
        let dir = tempfile::tempdir().unwrap();
        let bus = Arc::new(EventBus::new(dir.path().to_str().unwrap(), 64));

        // Create shared registry
        let shared = pool::RegistryShared::new(dir.path().to_str().unwrap()).unwrap();

        // Create manager from shared registry (simulating daemon mode)
        let mgr = McpManager::from_shared_registry(
            dir.path().to_str().unwrap(),
            shared.clone(),
            Some(bus),
        )
        .unwrap();

        // Verify defaults are present
        assert!(mgr.registry().count().unwrap() >= 4);
    }
}
