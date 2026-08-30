//! Periodic health checking with event-driven notifications.
//!
//! [`McpHealthChecker`] runs a background tokio task that probes
//! registered MCP servers every `check_interval` (default 5 min).
//! When a server's [`HealthStatusStr`] changes, a callback is invoked
//! (typically wiring to the shared [`EventBus`]).

use anyhow::Result;
use loom_core::{HealthCheck, HealthStatus, HealthStatusStr, McpServerEntry};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::Mutex;
use tokio::time::interval;
use tracing::{debug, info};

/// Default check interval (5 minutes).
pub const DEFAULT_HEALTH_INTERVAL: Duration = Duration::from_secs(300);

/// A running health-checker task handle.
#[derive(Debug)]
pub struct HealthCheckHandle {
    /// Signal to stop the background task.
    shutdown_tx: tokio::sync::watch::Sender<bool>,
}

impl HealthCheckHandle {
    /// Request the background checker to stop.
    pub fn shutdown(self) {
        let _ = self.shutdown_tx.send(true);
    }
}

/// Health status cache for a single server.
#[derive(Debug, Clone)]
struct ServerHealthState {
    id: String,
    name: String,
    status: HealthStatusStr,
    last_checked: Instant,
}

/// Callback invoked when a server's health status changes.
///
/// The `Box<dyn Fn(...)>` is [`Send`] so it can cross tokio task boundaries.
/// Note: [`Sync`] is not required because the callback is moved into the
/// spawned task via [`take`] before the loop starts, never shared.
pub type HealthChangeCallback = Box<dyn Fn(&str, &HealthStatusStr) + Send + Sync + 'static>;

/// Event-driven health checker for MCP servers.
///
/// Call [`McpHealthChecker::start`] to begin the background check
/// loop.  The handle returned can be used to stop the loop later.
pub struct McpHealthChecker {
    /// Servers being tracked, keyed by id.
    servers: Arc<Mutex<HashMap<String, ServerHealthState>>>,
    /// Optional callback fired when health status changes.
    on_change: Option<HealthChangeCallback>,
    /// Check interval.
    interval: Duration,
    /// Start time for uptime tracking.
    started_at: Instant,
}

impl McpHealthChecker {
    /// Create a new health checker with the given `check_interval`.
    ///
    /// `on_change` is called whenever a server's health changes.
    /// Use [`McpHealthCheckerBuilder`] for a fluent API.
    pub fn new(
        check_interval: Duration,
        on_change: Option<HealthChangeCallback>,
    ) -> Self {
        Self {
            servers: Arc::new(Mutex::new(HashMap::new())),
            on_change,
            interval: if check_interval.is_zero() {
                DEFAULT_HEALTH_INTERVAL
            } else {
                check_interval
            },
            started_at: Instant::now(),
        }
    }

    /// Create with default 5-min interval.
    pub fn with_defaults() -> Self {
        Self::new(DEFAULT_HEALTH_INTERVAL, None)
    }

    /// Register a server for health tracking.
    pub async fn register(&self, entry: &McpServerEntry) {
        let mut map = self.servers.lock().await;
        map.insert(
            entry.id.clone(),
            ServerHealthState {
                id: entry.id.clone(),
                name: entry.name.clone(),
                status: entry.health_status.clone(),
                last_checked: Instant::now(),
            },
        );
        debug!("Health: registered server '{}'", entry.id);
    }

    /// Unregister a server from health tracking.
    pub async fn unregister(&self, server_id: &str) {
        let mut map = self.servers.lock().await;
        map.remove(server_id);
        debug!("Health: unregistered server '{}'", server_id);
    }

    /// Start the background health-check loop.
    ///
    /// Returns a [`HealthCheckHandle`] that can be used to stop the loop.
    pub fn start(&mut self) -> HealthCheckHandle {
        let servers = self.servers.clone();
        let on_change = self.on_change.take();
        let check_interval = self.interval;

        let (shutdown_tx, mut shutdown_rx) = tokio::sync::watch::channel(false);
        let mut ticker = interval(check_interval);

        tokio::spawn(async move {
            info!(
                "Health checker started (interval = {:?})",
                check_interval
            );

            loop {
                tokio::select! {
                    _ = ticker.tick() => {
                        let snapshot = {
                            let map = servers.lock().await;
                            map.values().cloned().collect::<Vec<_>>()
                        };
                        for state in &snapshot {
                            let new_status = probe_server(state);
                            // Compare via string serialisation since HealthStatusStr
                            // does not implement PartialEq in loom-core.
                            let changed = serde_json::to_string(&new_status).ok()
                                != serde_json::to_string(&state.status).ok();
                            if changed {
                                {
                                    let mut map = servers.lock().await;
                                    if let Some(s) = map.get_mut(&state.id) {
                                        s.status = new_status.clone();
                                        s.last_checked = Instant::now();
                                    }
                                }

                                info!(
                                    "Health: server '{}' status changed from {:?} to {:?}",
                                    state.id, state.status, new_status
                                );

                                // Fire callback (Send-safe)
                                if let Some(ref cb) = on_change {
                                    cb(&state.id, &new_status);
                                }
                            }
                        }
                    }
                    _ = shutdown_rx.changed() => {
                        info!("Health checker shutting down");
                        break;
                    }
                }
            }
        });

        HealthCheckHandle { shutdown_tx }
    }

    /// Manually trigger a health check for a single server and return
    /// the updated status.  Does **not** fire the change callback.
    pub async fn check_one(&self, server_id: &str) -> Option<HealthStatusStr> {
        let state = {
            let map = self.servers.lock().await;
            map.get(server_id).cloned()
        };
        match state {
            Some(st) => {
                let status = probe_server(&st);
                let mut map = self.servers.lock().await;
                if let Some(s) = map.get_mut(server_id) {
                    s.status = status.clone();
                    s.last_checked = Instant::now();
                }
                Some(status)
            }
            None => None,
        }
    }

    /// Return the current cached health status for all tracked servers.
    pub async fn status_snapshot(&self) -> Vec<(String, String, HealthStatusStr)> {
        let map = self.servers.lock().await;
        map.values()
            .map(|s| (s.id.clone(), s.name.clone(), s.status.clone()))
            .collect()
    }

    /// Return the cached status for a single server.
    pub async fn status_of(&self, server_id: &str) -> Option<HealthStatusStr> {
        let map = self.servers.lock().await;
        map.get(server_id).map(|s| s.status.clone())
    }
}

/// Fluent builder for [`McpHealthChecker`].
pub struct McpHealthCheckerBuilder {
    interval: Duration,
    on_change: Option<HealthChangeCallback>,
}

impl McpHealthCheckerBuilder {
    pub fn new() -> Self {
        Self {
            interval: DEFAULT_HEALTH_INTERVAL,
            on_change: None,
        }
    }

    pub fn interval(mut self, interval: Duration) -> Self {
        self.interval = interval;
        self
    }

    pub fn on_change<F>(mut self, callback: F) -> Self
    where
        F: Fn(&str, &HealthStatusStr) + Send + Sync + 'static,
    {
        self.on_change = Some(Box::new(callback));
        self
    }

    pub fn build(self) -> McpHealthChecker {
        McpHealthChecker::new(self.interval, self.on_change)
    }
}

impl Default for McpHealthCheckerBuilder {
    fn default() -> Self {
        Self::new()
    }
}

impl HealthCheck for McpHealthChecker {
    fn health(&self) -> Result<HealthStatus> {
        Ok(HealthStatus {
            crate_name: "loom-mcp",
            version: env!("CARGO_PKG_VERSION"),
            uptime_seconds: self.started_at.elapsed().as_secs(),
            last_error: None,
            performance_counters: {
                let mut h = std::collections::HashMap::new();
                h.insert(
                    "check_interval_secs".into(),
                    self.interval.as_secs(),
                );
                h
            },
        })
    }
}

/// Probe a single server's health.
///
/// For a real implementation this would make an HTTP request or check
/// the process's liveness.  The current stub returns `Healthy` for any
/// server that was recently checked.
fn probe_server(state: &ServerHealthState) -> HealthStatusStr {
    let elapsed = state.last_checked.elapsed();
    let max_age = DEFAULT_HEALTH_INTERVAL * 2;

    if elapsed > max_age {
        return HealthStatusStr::Unknown;
    }

    match state.status {
        HealthStatusStr::Offline => HealthStatusStr::Offline,
        _ => HealthStatusStr::Healthy,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use loom_core::{McpTransport, McpTransportType};
    use std::collections::HashMap;
    use std::sync::atomic::{AtomicUsize, Ordering};

    fn sample_entry(id: &str) -> McpServerEntry {
        McpServerEntry {
            id: id.to_string(),
            name: id.to_string(),
            transport: McpTransport {
                transport_type: McpTransportType::Stdio,
                command: Some("cat".to_string()),
                args: vec![],
                url: None,
                env: HashMap::new(),
            },
            shared: true,
            tools_filter: vec![],
            health_status: HealthStatusStr::Unknown,
            last_health_check: String::new(),
            tags: vec![],
            compatible_tools: vec![],
            required_by_skills: vec![],
        }
    }

    #[tokio::test]
    async fn test_register_and_status() {
        let checker = McpHealthChecker::with_defaults();
        checker.register(&sample_entry("srv-1")).await;
        let snapshot = checker.status_snapshot().await;
        assert_eq!(snapshot.len(), 1);
        assert_eq!(snapshot[0].0, "srv-1");
    }

    #[tokio::test]
    async fn test_check_one() {
        let checker = McpHealthChecker::with_defaults();
        checker.register(&sample_entry("srv-1")).await;
        let status = checker.check_one("srv-1").await;
        assert!(status.is_some());
        assert!(matches!(status.unwrap(), HealthStatusStr::Healthy));
    }

    #[tokio::test]
    async fn test_unregister() {
        let checker = McpHealthChecker::with_defaults();
        checker.register(&sample_entry("srv-1")).await;
        checker.unregister("srv-1").await;
        assert!(checker.status_of("srv-1").await.is_none());
    }

    #[tokio::test]
    async fn test_start_and_shutdown() {
        let mut checker = McpHealthChecker::with_defaults();
        checker.register(&sample_entry("srv-1")).await;
        let handle = checker.start();
        tokio::time::sleep(Duration::from_millis(50)).await;
        handle.shutdown();
    }

    #[tokio::test]
    async fn test_on_change_callback() {
        let call_count = Arc::new(AtomicUsize::new(0));
        let count_clone = call_count.clone();

        let mut checker = McpHealthChecker::new(
            Duration::from_millis(20),
            Some(Box::new(move |_id, _status| {
                count_clone.fetch_add(1, Ordering::SeqCst);
            })),
        );
        checker.register(&sample_entry("cb-srv")).await;
        let handle = checker.start();
        tokio::time::sleep(Duration::from_millis(80)).await;
        handle.shutdown();

        let count = call_count.load(Ordering::SeqCst);
        // At least one health-change event (Unknown -> Healthy)
        assert!(count >= 1, "Expected at least 1 callback, got {}", count);
    }

    #[test]
    fn test_health_trait() {
        let checker = McpHealthChecker::with_defaults();
        let h = checker.health().unwrap();
        assert_eq!(h.crate_name, "loom-mcp");
    }
}
