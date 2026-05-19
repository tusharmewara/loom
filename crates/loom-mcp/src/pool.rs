//! Daemon-managed process pool with reference counting.
//!
//! [`McpPool`] tracks how many clients are using each MCP server process.
//! `acquire()` spawns a new process (or returns an existing pid) and
//! increments the reference count. `release()` decrements the count and
//! kills the process when it reaches zero.

use anyhow::{Context, Result};
use loom_core::{McpServerEntry, McpTransportType};
use std::collections::HashMap;
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use tracing::{debug, info, warn};

/// Reference-counted handle to a running MCP server process.
struct PooledProcess {
    /// Child process handle (None after the process has been killed).
    child: Option<Child>,
    /// Number of active acquisitions.
    refcount: usize,
    /// Snapshot of the entry at spawn time.
    #[allow(dead_code)]
    entry: McpServerEntry,
}

/// Daemon-managed MCP server process pool.
///
/// Each server is identified by its `server_id`.  Calls to `acquire`
/// must be paired with a corresponding `release`.
pub struct McpPool {
    processes: Arc<Mutex<HashMap<String, PooledProcess>>>,
}

impl McpPool {
    /// Create a new, empty pool.
    pub fn new() -> Self {
        Self {
            processes: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Acquire a reference to the MCP server identified by `server_id`.
    ///
    /// If the process is already running, its reference count is
    /// incremented.  Otherwise a new process is spawned according to
    /// the provided `entry`.
    pub fn acquire(&self, server_id: &str, entry: &McpServerEntry) -> Result<()> {
        let mut map = self.processes.lock().unwrap();

        if let Some(pooled) = map.get_mut(server_id) {
            // Already running — just bump the refcount
            pooled.refcount += 1;
            debug!(
                "Pool: acquired {} (refcount now {})",
                server_id, pooled.refcount
            );
            return Ok(());
        }

        // Spawn a new process
        let child = spawn_process(entry).context("Failed to spawn MCP server process")?;
        info!(
            "Pool: spawned MCP server '{}' (pid {})",
            server_id,
            child.id()
        );

        map.insert(
            server_id.to_string(),
            PooledProcess {
                child: Some(child),
                refcount: 1,
                entry: entry.clone(),
            },
        );
        Ok(())
    }

    /// Release a reference to the MCP server identified by `server_id`.
    ///
    /// When the reference count reaches zero the process is killed.
    /// Returns `true` if the process was killed as a result of this
    /// release.
    pub fn release(&self, server_id: &str) -> Result<bool> {
        let mut map = self.processes.lock().unwrap();

        let killed = if let Some(pooled) = map.get_mut(server_id) {
            if pooled.refcount == 0 {
                warn!("Pool: release on {} with refcount already 0", server_id);
                return Ok(false);
            }
            pooled.refcount -= 1;

            if pooled.refcount == 0 {
                // Kill the process
                let mut child = pooled.child.take();
                if let Some(ref mut c) = child {
                    let pid = c.id();
                    match c.kill() {
                        Ok(_) => {
                            let _ = c.wait();
                            info!("Pool: killed MCP server '{}' (pid {})", server_id, pid);
                        }
                        Err(e) => {
                            warn!(
                                "Pool: failed to kill MCP server '{}' (pid {}): {}",
                                server_id, pid, e
                            );
                        }
                    }
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
            warn!("Pool: release on unknown server '{}'", server_id);
            false
        };

        Ok(killed)
    }

    /// Returns the current reference count for a server (or 0 if unknown).
    pub fn refcount(&self, server_id: &str) -> usize {
        let map = self.processes.lock().unwrap();
        map.get(server_id).map_or(0, |p| p.refcount)
    }

    /// Returns `true` if the server process is currently running.
    pub fn is_running(&self, server_id: &str) -> bool {
        let map = self.processes.lock().unwrap();
        map.contains_key(server_id)
    }

    /// Kill all running processes and clear the pool.
    pub fn shutdown_all(&self) -> Vec<String> {
        let mut map = self.processes.lock().unwrap();
        let ids: Vec<String> = map.keys().cloned().collect();
        for id in &ids {
            if let Some(mut pooled) = map.remove(id) {
                if let Some(ref mut child) = pooled.child {
                    let _ = child.kill();
                    let _ = child.wait();
                }
            }
        }
        ids
    }

    /// Number of distinct server processes currently running.
    pub fn running_count(&self) -> usize {
        let map = self.processes.lock().unwrap();
        map.len()
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
    fn test_acquire_and_release() {
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
    fn test_release_unknown() {
        let pool = McpPool::new();
        assert!(!pool.release("nonexistent").unwrap());
    }

    #[test]
    fn test_shutdown_all() {
        let pool = McpPool::new();
        pool.acquire("srv-a", &sample_entry("srv-a")).unwrap();
        pool.acquire("srv-b", &sample_entry("srv-b")).unwrap();
        assert_eq!(pool.running_count(), 2);

        let killed = pool.shutdown_all();
        assert_eq!(killed.len(), 2);
        assert_eq!(pool.running_count(), 0);
    }
}
