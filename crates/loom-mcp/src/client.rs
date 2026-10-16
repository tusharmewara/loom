//! Connection pooling for MCP client connections.
//!
//! [`McpClientPool`] reuses established connections to MCP servers
//! across multiple tool invocations, avoiding redundant handshakes
//! and reducing latency.

use anyhow::Result;
use loom_core::{McpServerEntry, McpTransportType};
use std::collections::HashMap;
use std::sync::Mutex;
use tracing::{debug, info};

/// A generic MCP client connection handle.
///
/// In a full implementation this would wrap an FFI bridge to the
/// TypeScript `@modelcontextprotocol/sdk` client.  For now it is a
/// placeholder that tracks the server identity.
#[derive(Debug, Clone)]
pub struct McpClientConnection {
    /// Server id this connection is bound to.
    pub server_id: String,
    /// Server name (for diagnostics).
    pub server_name: String,
    /// Whether the transport is a persistent TCP/pipe connection.
    pub persistent: bool,
    /// Monotonically increasing connection id for diagnostics.
    pub conn_id: u64,
    /// Optional URL for HTTP/SSE transports.
    pub url: Option<String>,
}

/// Pool of reusable MCP client connections.
///
/// The pool is keyed by `server_id`.  When a connection is checked in
/// it is returned to the pool for future reuse.
pub struct McpClientPool {
    /// Available connections, keyed by server id.
    available: Mutex<HashMap<String, Vec<McpClientConnection>>>,
    /// Active (checked-out) connections, keyed by server id.
    active: Mutex<HashMap<String, Vec<McpClientConnection>>>,
    /// Global connection counter for unique ids.
    next_id: Mutex<u64>,
    /// Maximum pool size per server.
    max_per_server: usize,
}

impl McpClientPool {
    /// Create a new pool with the given max connections per server.
    pub fn new(max_per_server: usize) -> Self {
        Self {
            available: Mutex::new(HashMap::new()),
            active: Mutex::new(HashMap::new()),
            next_id: Mutex::new(1),
            max_per_server,
        }
    }

    /// Create a pool with a default max of 4 connections per server.
    pub fn with_defaults() -> Self {
        Self::new(4)
    }

    /// Acquire (or create) a connection to the given server.
    ///
    /// If an idle connection is available in the pool it is reused.
    /// Otherwise a new connection handle is created.
    pub fn acquire(&self, entry: &McpServerEntry) -> Result<McpClientConnection> {
        let mut available = loom_core::lock_ok!(self.available);

        if let Some(conns) = available.get_mut(&entry.id) {
            if let Some(conn) = conns.pop() {
                debug!("Client: reusing connection {} for '{}'", conn.conn_id, entry.id);
                self.track_active(conn.clone());
                return Ok(conn);
            }
        }

        // Create a new connection
        let id = {
            let mut n = loom_core::lock_ok!(self.next_id);
            let id = *n;
            *n += 1;
            id
        };

        let persistent = matches!(
            entry.transport.transport_type,
            McpTransportType::Stdio
        );

        let conn = McpClientConnection {
            server_id: entry.id.clone(),
            server_name: entry.name.clone(),
            persistent,
            conn_id: id,
            url: entry.transport.url.clone(),
        };

        debug!(
            "Client: created new connection {} for '{}'",
            conn.conn_id, entry.id
        );
        self.track_active(conn.clone());
        Ok(conn)
    }

    /// Return a connection to the pool for reuse.
    ///
    /// The caller should not use the connection after calling `release`.
    pub fn release(&self, conn: McpClientConnection) {
        let mut available = loom_core::lock_ok!(self.available);
        let mut active = loom_core::lock_ok!(self.active);

        // Remove from active tracking
        if let Some(conns) = active.get_mut(&conn.server_id) {
            conns.retain(|c| c.conn_id != conn.conn_id);
        }

        let pool = available.entry(conn.server_id.clone()).or_default();
        if pool.len() < self.max_per_server {
            pool.push(conn);
        }
        // If pool is full, the connection is dropped (implied drop)
    }

    /// Number of idle (available) connections for a server.
    pub fn available_count(&self, server_id: &str) -> usize {
        let available = loom_core::lock_ok!(self.available);
        available.get(server_id).map_or(0, |v| v.len())
    }

    /// Number of active (checked-out) connections for a server.
    pub fn active_count(&self, server_id: &str) -> usize {
        let active = loom_core::lock_ok!(self.active);
        active.get(server_id).map_or(0, |v| v.len())
    }

    /// Total connections (idle + active) for a server.
    pub fn total_count(&self, server_id: &str) -> usize {
        self.available_count(server_id) + self.active_count(server_id)
    }

    /// Evict all idle connections for a server.
    pub fn evict(&self, server_id: &str) -> usize {
        let mut available = loom_core::lock_ok!(self.available);
        let count = available.remove(server_id).map_or(0, |v| v.len());
        if count > 0 {
            info!("Client: evicted {} idle connections for '{}'", count, server_id);
        }
        count
    }

    /// Drain all pools (useful during shutdown).
    pub fn drain_all(&self) -> usize {
        let mut available = loom_core::lock_ok!(self.available);
        let total: usize = available.values().map(|v| v.len()).sum();
        available.clear();
        total
    }

    /// Track a connection as active.
    fn track_active(&self, conn: McpClientConnection) {
        let mut active = loom_core::lock_ok!(self.active);
        active.entry(conn.server_id.clone()).or_default().push(conn);
    }
}

impl Default for McpClientPool {
    fn default() -> Self {
        Self::with_defaults()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use loom_core::{McpTransport, McpTransportType};
    use std::collections::HashMap;

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
            health_status: loom_core::HealthStatusStr::Unknown,
            last_health_check: String::new(),
            tags: vec![],
            compatible_tools: vec![],
            required_by_skills: vec![],
        }
    }

    #[test]
    fn test_acquire_and_release() {
        let pool = McpClientPool::with_defaults();
        let entry = sample_entry("srv-1");

        let conn = pool.acquire(&entry).unwrap();
        assert_eq!(conn.server_id, "srv-1");
        assert_eq!(pool.active_count("srv-1"), 1);
        assert_eq!(pool.available_count("srv-1"), 0);

        pool.release(conn);
        assert_eq!(pool.active_count("srv-1"), 0);
        assert_eq!(pool.available_count("srv-1"), 1);
    }

    #[test]
    fn test_reuse_connection() {
        let pool = McpClientPool::with_defaults();
        let entry = sample_entry("srv-1");

        let conn1 = pool.acquire(&entry).unwrap();
        let conn1_id = conn1.conn_id;
        pool.release(conn1);

        let conn2 = pool.acquire(&entry).unwrap();
        // Should reuse the same connection
        assert_eq!(conn2.conn_id, conn1_id);
    }

    #[test]
    fn test_max_per_server() {
        let pool = McpClientPool::new(2);
        let entry = sample_entry("srv-1");

        let c1 = pool.acquire(&entry).unwrap();
        let c2 = pool.acquire(&entry).unwrap();
        assert_eq!(pool.active_count("srv-1"), 2);

        pool.release(c1);
        pool.release(c2);

        // Pool now has 2 available
        assert_eq!(pool.available_count("srv-1"), 2);

        // A third release would drop because pool is full
        let c3 = pool.acquire(&entry).unwrap();
        pool.release(c3);
        assert_eq!(pool.available_count("srv-1"), 2); // still 2 max
    }

    #[test]
    fn test_evict() {
        let pool = McpClientPool::with_defaults();
        let entry = sample_entry("srv-1");

        let conn = pool.acquire(&entry).unwrap();
        pool.release(conn);
        assert_eq!(pool.evict("srv-1"), 1);
        assert_eq!(pool.available_count("srv-1"), 0);
    }
}
