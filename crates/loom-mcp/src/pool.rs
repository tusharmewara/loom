//! Daemon-managed process pool with reference counting and registry coordination.
//!
//! [`McpPool`] tracks how many clients are using each MCP server process.
//! It coordinates with [`McpRegistry`] to reuse existing server instances
//! across multiple tools, preventing unnecessary process spawning.
//!
//! `acquire()` checks the shared registry before spawning a new process:
//! if a server is already running with a positive reference count, the
//! existing PID handle is reused. Otherwise a new process is spawned.
//! `release()` decrements the count and kills the process when it reaches zero.

use anyhow::{Context, Result};
use loom_core::{McpServerEntry, McpTransportType};
use std::collections::HashMap;
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex, Weak};
use tracing::{debug, info, warn};

/// Reference-counted handle to a running MCP server process.
pub(crate) struct PooledProcess {
    /// Child process handle (None after the process has been killed).
    child: Option<Child>,
    /// Number of active acquisitions.
    refcount: usize,
}

impl std::fmt::Debug for PooledProcess {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PooledProcess")
            .field("pid", &self.child.as_ref().map(|c| c.id()))
            .field("refcount", &self.refcount)
            .finish()
    }
}

/// Daemon-managed MCP server process pool.
///
/// Each server is identified by its `server_id`.  Calls to `acquire`
/// must be paired with a corresponding `release`.
///
/// The pool coordinates with an [`McpRegistry`] to track process state
/// across tools. On `acquire`, it first checks if the registry reports
/// the server as `running` with a positive reference count. If so,
/// it reuses the existing process handle instead of spawning a new one.
pub struct McpPool {
    processes: Arc<Mutex<HashMap<String, PooledProcess>>>,
    /// Weak reference to the registry for shared process state tracking.
    registry: Weak<RegistryShared>,
}

/// Shared registry wrapper for cross-component access.
///
/// Owns the single source of truth for live MCP server processes across
/// all pools that share this registry (i.e. all pools within a single
/// daemon process). Because the process map lives here, distinct
/// [`McpPool`] instances referencing the same `RegistryShared` reuse the
/// same spawned `Child` handles instead of spawning duplicates.
#[derive(Debug)]
pub struct RegistryShared {
    inner: crate::registry::McpRegistry,
    /// Shared live-process map, indexed by server id.
    processes: Arc<Mutex<HashMap<String, PooledProcess>>>,
}

impl RegistryShared {
    pub fn inner(&self) -> &crate::registry::McpRegistry {
        &self.inner
    }

    /// Access the shared live-process map used by all pools referencing
    /// this registry.
    pub(crate) fn shared_processes(&self) -> &Arc<Mutex<HashMap<String, PooledProcess>>> {
        &self.processes
    }

    pub fn new(loom_dir: &str) -> Result<Arc<Self>> {
        let inner = crate::registry::McpRegistry::new(loom_dir)?;
        // The daemon-facing shared registry wants the built-in defaults present.
        inner.seed_defaults()?;
        Ok(Arc::new(Self {
            inner,
            processes: Arc::new(Mutex::new(HashMap::new())),
        }))
    }
}

impl McpPool {
    /// Create a new pool that coordinates with the given registry.
    pub fn with_registry(registry: Weak<RegistryShared>) -> Self {
        Self {
            processes: Arc::new(Mutex::new(HashMap::new())),
            registry,
        }
    }

    /// Create a new, empty pool (without registry coordination).
    pub fn new() -> Self {
        Self {
            processes: Arc::new(Mutex::new(HashMap::new())),
            registry: Weak::new(),
        }
    }

    /// Resolve the effective process map.
    ///
    /// When a shared registry is available its shared process map is used,
    /// so all pools referencing the same registry operate on the same live
    /// processes. Otherwise this pool's private map is used.
    fn process_map(&self) -> Arc<Mutex<HashMap<String, PooledProcess>>> {
        match self.registry.upgrade() {
            Some(shared) => shared.shared_processes().clone(),
            None => self.processes.clone(),
        }
    }

    /// Acquire a reference to the MCP server identified by `server_id`.
    ///
    /// If the process is already running, its reference count is
    /// incremented.  Otherwise a new process is spawned according to
    /// the provided `entry`.
    ///
    /// Pools sharing a registry share the same live processes, so a
    /// process spawned by one pool is reused by any other pool that
    /// references the same registry (i.e. within the same daemon process).
    pub fn acquire(&self, server_id: &str, entry: &McpServerEntry) -> Result<()> {
        let map = self.process_map();
        let mut map = map.lock().unwrap();

        if let Some(pooled) = map.get_mut(server_id) {
            // Already running — just bump the refcount
            pooled.refcount += 1;
            debug!(
                "Pool: acquired {} (refcount now {})",
                server_id, pooled.refcount
            );
            if let Some(reg) = self.registry.upgrade() {
                reg.inner().set_process_state(server_id, "running", pooled.child.as_ref().map(|c| c.id() as i32))?;
                reg.inner().increment_refcount(server_id)?;
            }
            return Ok(());
        }

        // Spawn a new process
        let child = spawn_process(entry)
            .with_context(|| format!("Failed to spawn MCP server '{}'", server_id))?;
        let pid = child.id();
        info!(
            "Pool: spawned MCP server '{}' (pid {})",
            server_id, pid
        );

        map.insert(
            server_id.to_string(),
            PooledProcess {
                child: Some(child),
                refcount: 1,
            },
        );

        // Update registry state
        if let Some(reg) = self.registry.upgrade() {
            reg.inner().set_process_state(server_id, "running", Some(pid as i32))?;
            reg.inner().increment_refcount(server_id)?;
        }

        Ok(())
    }

    /// Release a reference to the MCP server identified by `server_id`.
    ///
    /// When the reference count reaches zero the process is killed.
    /// Returns `true` if the process was killed as a result of this
    /// release.
    pub fn release(&self, server_id: &str) -> Result<bool> {
        let map = self.process_map();
        let mut map = map.lock().unwrap();

        let killed = if let Some(pooled) = map.get_mut(server_id) {
            if pooled.refcount == 0 {
                warn!("Pool: release on {} with refcount already 0", server_id);
                return Ok(false);
            }
            pooled.refcount -= 1;

            if let Some(reg) = self.registry.upgrade() {
                reg.inner().decrement_refcount(server_id)?;
            }

            if pooled.refcount == 0 {
                // Kill the process
                let mut child = pooled.child.take();
                let pid = child.as_ref().map(|c| c.id());

                if let Some(ref mut c) = child {
                    let pid_val = c.id();
                    match c.kill() {
                        Ok(_) => {
                            let _ = c.wait();
                            info!("Pool: killed MCP server '{}' (pid {})", server_id, pid_val);
                        }
                        Err(e) => {
                            warn!(
                                "Pool: failed to kill MCP server '{}' (pid {:?}): {}",
                                server_id, pid, e
                            );
                        }
                    }
                }

                // Update registry state
                if let Some(reg) = self.registry.upgrade() {
                    reg.inner()
                        .set_process_state(server_id, "terminated", None)?;
                }

                map.remove(server_id);
                true
            } else {
                debug!(
                    "Pool: released {} (refcount now {})",
                    server_id, pooled.refcount
                );
                false
            }
        } else {
            // Not present in the (possibly shared) process map.
            warn!("Pool: release on unknown server '{}'", server_id);
            false
        };

        Ok(killed)
    }

    /// Returns the current reference count for a server (or 0 if unknown).
    pub fn refcount(&self, server_id: &str) -> usize {
        let map = self.process_map();
        let map = map.lock().unwrap();
        map.get(server_id).map_or(0, |p| p.refcount)
    }

    /// Returns `true` if the server process is currently running.
    pub fn is_running(&self, server_id: &str) -> bool {
        let map = self.process_map();
        let map = map.lock().unwrap();
        map.get(server_id).is_some_and(|p| p.child.is_some())
    }

    /// Kill all running processes and clear the pool.
    pub fn shutdown_all(&self) -> Vec<String> {
        let map = self.process_map();
        let mut map = map.lock().unwrap();
        let ids: Vec<String> = map.keys().cloned().collect();

        let mut had_registry = false;
        for id in &ids {
            if let Some(mut pooled) = map.remove(id) {
                if let Some(ref mut child) = pooled.child {
                    let _ = child.kill();
                    let _ = child.wait();
                }

                // Mark this process terminated in the shared registry.
                if let Some(reg) = self.registry.upgrade() {
                    had_registry = true;
                    let _ = reg
                        .inner()
                        .set_process_state(id, "terminated", None);
                }
            }
        }

        // After all processes are terminated, clear the shared refcounts once.
        // Doing this inside the loop would wipe each victim's "terminated"
        // marker on the next iteration, leaving no durable termination signal.
        if had_registry {
            if let Some(reg) = self.registry.upgrade() {
                let _ = reg.inner().reset_process_state();
            }
        }
        ids
    }

    /// Number of distinct server processes currently running.
    pub fn running_count(&self) -> usize {
        let map = self.process_map();
        let map = map.lock().unwrap();
        map.len()
    }

    /// Get shared registry reference for cross-tool state coordination.
    pub fn shared_registry(loom_dir: &str) -> Result<Arc<RegistryShared>> {
        RegistryShared::new(loom_dir)
    }
}

impl Default for McpPool {
    fn default() -> Self {
        Self::new()
    }
}

// ─── Process spawning ─────────────────────────────────────────────────

/// Spawn an MCP server process based on the transport configuration.
fn spawn_process(entry: &McpServerEntry) -> Result<Child> {
    match &entry.transport.transport_type {
        McpTransportType::Stdio => {
            let command = entry
                .transport
                .command
                .as_deref()
                .ok_or_else(|| anyhow::anyhow!("Stdio transport requires 'command'"))?;

            let mut cmd = Command::new(command);
            cmd.args(&entry.transport.args)
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped());

            // Set environment variables
            for (k, v) in &entry.transport.env {
                cmd.env(k, v);
            }

            let child = cmd.spawn().with_context(|| {
                format!(
                    "Failed to spawn MCP server '{}' command '{}'",
                    entry.id, command
                )
            })?;

            Ok(child)
        }
        McpTransportType::Http | McpTransportType::Sse => {
            // HTTP/SSE transports are external services — we register a
            // placeholder that the health checker can probe.  No local
            // process is spawned.
            anyhow::bail!(
                "Process pool does not support {:?} transport for '{}'; \
                 use an external URL-based connection instead",
                entry.transport.transport_type,
                entry.id
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use loom_core::{McpTransport, McpTransportType};
    use std::collections::HashMap;

    fn sample_entry(id: &str) -> McpServerEntry {
        // Use a known-safe command that exits cleanly but stays alive
        McpServerEntry {
            id: id.to_string(),
            name: id.to_string(),
            transport: McpTransport {
                transport_type: McpTransportType::Stdio,
                command: Some("cat".to_string()), // cat waits on stdin
                args: vec![],
                url: None,
                env: HashMap::new(),
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
    fn test_pool_without_registry() {
        let pool = McpPool::new();
        let entry = sample_entry("test-srv");

        pool.acquire("test-srv", &entry).unwrap();
        assert!(pool.is_running("test-srv"));
        assert_eq!(pool.refcount("test-srv"), 1);

        // Second acquire bumps refcount
        pool.acquire("test-srv", &entry).unwrap();
        assert_eq!(pool.refcount("test-srv"), 2);

        // First release: still alive
        let killed = pool.release("test-srv").unwrap();
        assert!(!killed);
        assert_eq!(pool.refcount("test-srv"), 1);

        // Second release: kills process
        let killed = pool.release("test-srv").unwrap();
        assert!(killed);
        assert!(!pool.is_running("test-srv"));
    }

    #[test]
    fn test_release_unknown_server() {
        let pool = McpPool::new();
        // Should not error on unknown server
        assert!(!pool.release("nonexistent").unwrap());
    }

    #[test]
    fn test_shared_registry_creation() {
        let dir = tempfile::tempdir().unwrap();
        let shared = McpPool::shared_registry(dir.path().to_str().unwrap()).unwrap();
        assert_eq!(shared.inner().count().unwrap(), 4); // 4 default servers
    }

    #[test]
    fn test_pool_with_shared_registry() {
        let dir = tempfile::tempdir().unwrap();
        let shared = McpPool::shared_registry(dir.path().to_str().unwrap()).unwrap();
        let entry = sample_entry("registry-srv");

        // Register entry
        shared.inner().upsert(&entry).unwrap();

        let pool = McpPool::with_registry(Arc::downgrade(&shared));
        pool.acquire("registry-srv", &entry).unwrap();
        assert!(pool.is_running("registry-srv"));

        // Verify registry recorded the process state
        let state = shared.inner().get_process_state("registry-srv").unwrap();
        assert!(state.is_some());
        assert_eq!(state.unwrap().0, "running");

        // Second acquire should reuse
        pool.acquire("registry-srv", &entry).unwrap();
        assert_eq!(pool.refcount("registry-srv"), 2);

        // Release
        pool.release("registry-srv").unwrap();
        assert_eq!(pool.refcount("registry-srv"), 1);
    }

    #[test]
    fn test_two_pools_share_single_process() {
        let dir = tempfile::tempdir().unwrap();
        let shared = McpPool::shared_registry(dir.path().to_str().unwrap()).unwrap();
        let entry = sample_entry("cross-srv");

        shared.inner().upsert(&entry).unwrap();

        // Two distinct pools referencing the same shared registry
        let pool_a = McpPool::with_registry(Arc::downgrade(&shared));
        let pool_b = McpPool::with_registry(Arc::downgrade(&shared));

        // Pool A spawns the process
        pool_a.acquire("cross-srv", &entry).unwrap();
        assert!(pool_a.is_running("cross-srv"));
        assert_eq!(pool_a.refcount("cross-srv"), 1);

        // Pool B must reuse the same process, not spawn a duplicate
        pool_b.acquire("cross-srv", &entry).unwrap();
        assert!(pool_b.is_running("cross-srv"));
        assert_eq!(pool_b.refcount("cross-srv"), 2);
        // Both observe the same single running handle
        assert!(pool_a.is_running("cross-srv"));
        assert_eq!(pool_a.refcount("cross-srv"), 2);

        // Releasing from one pool keeps the process alive for the other
        assert!(!pool_a.release("cross-srv").unwrap());
        assert!(pool_b.is_running("cross-srv"));
        assert_eq!(pool_b.refcount("cross-srv"), 1);

        // Only the last release kills the process
        assert!(pool_b.release("cross-srv").unwrap());
        assert!(!pool_a.is_running("cross-srv"));
        assert!(!pool_b.is_running("cross-srv"));

        // Registry state reflects the final termination
        let state = shared.inner().get_process_state("cross-srv").unwrap();
        assert_eq!(state.unwrap().0, "terminated");
    }

    #[test]
    fn test_shutdown_all_clears_registry() {
        let dir = tempfile::tempdir().unwrap();
        let shared = McpPool::shared_registry(dir.path().to_str().unwrap()).unwrap();
        let entry = sample_entry("shutdown-test");

        shared.inner().upsert(&entry).unwrap();

        let pool = McpPool::with_registry(Arc::downgrade(&shared));
        pool.acquire("shutdown-test", &entry).unwrap();
        assert!(pool.is_running("shutdown-test"));

        let killed = pool.shutdown_all();
        assert!(killed.contains(&"shutdown-test".to_string()));
        assert!(!pool.is_running("shutdown-test"));

        // Registry state should be reset
        let state = shared.inner().get_process_state("shutdown-test").unwrap();
        assert!(state.is_none());
    }
}