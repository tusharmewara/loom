//! SQLite-backed registry for MCP server entries with shared state tracking.
//!
//! Stores [`McpServerEntry`] values with secondary indexes on `id`,
//! `health_status`, `shared`, and `tags` for fast lookups.
//! Also tracks process affinity and lifecycle state for cross-tool sharing.

use anyhow::{Context, Result};
use loom_core::{HealthStatusStr, McpServerEntry, McpTransport, McpTransportType};
use rusqlite::{params, Connection};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

/// SQLite-backed MCP server registry with shared state tracking.
///
/// Maintains secondary indexes on `id`, `health_status`, `shared`, and `tags`
/// for fast lookups. Also tracks process lifecycle state (`running`, `stopped`,
/// `terminated`) to enable cross-tool process reuse. The registry serves as the
/// single source of truth for MCP server configuration and status across all
/// tools and processes sharing the same `~/.loom/` directory.
///
/// This type is `Clone` - clones share the same underlying database connection
/// and state tracking (reference counts, process states).
#[derive(Debug, Clone)]
pub struct McpRegistry {
    conn: Arc<Mutex<Connection>>,
    db_path: PathBuf,
    /// Number of active process references per server ID (for process reuse tracking).
    ref_counts: Arc<Mutex<HashMap<String, usize>>>,
}

impl McpRegistry {
    /// Open (or create) the SQLite database at `loom_dir / mcp_registry.db`.
    pub fn new(loom_dir: &str) -> Result<Self> {
        let db_dir = Path::new(loom_dir);
        std::fs::create_dir_all(db_dir).context("Failed to create agents dir for registry")?;

        let db_path = db_dir.join("mcp_registry.db");
        let conn = loom_core::db::open(&db_path)?;

        // Create process state tracking table if not exists
        conn.execute_batch(
            "
            CREATE TABLE IF NOT EXISTS mcp_process_state (
                server_id    TEXT PRIMARY KEY NOT NULL,
                state        TEXT NOT NULL DEFAULT 'stopped',
                pid          INTEGER,
                last_seen_at TEXT NOT NULL DEFAULT (datetime('now'))
            );
            ",
        )?;

        let registry = Self {
            conn: Arc::new(Mutex::new(conn)),
            db_path,
            ref_counts: Arc::new(Mutex::new(HashMap::new())),
        };
        registry.initialize_tables()?;
        Ok(registry)
    }

    /// Seed the built-in default servers (context-mode, rtk, loom,
    /// agentmemory) if the table is empty.
    ///
    /// This is intentionally NOT called from `new()` so that explicit setup
    /// paths (e.g. `loom init --recommended`) can populate the registry with
    /// exactly the servers they request. It is wired into the daemon-facing
    /// `RegistryShared` path (`McpPool::shared_registry`) which wants the
    /// defaults present. The whole-table guard (`count == 0`) ensures defaults
    /// are never duplicated or written over existing entries.
    pub fn seed_defaults(&self) -> Result<()> {
        let count = self.count()?;
        if count == 0 {
            // Seed context-mode, rtk, loom, and agentmemory servers
            for entry in default_entries() {
                self.upsert(&entry)?;
            }
        }
        Ok(())
    }

    /// Open an in-memory database (for tests).
    pub fn in_memory() -> Result<Self> {
        let conn = Connection::open_in_memory()?;
        let registry = Self {
            conn: Arc::new(Mutex::new(conn)),
            db_path: PathBuf::from(":memory:"),
            ref_counts: Arc::new(Mutex::new(HashMap::new())),
        };
        registry.initialize_tables()?;
        Ok(registry)
    }

    /// Initialise schema and secondary indexes.
    fn initialize_tables(&self) -> Result<()> {
        let conn = loom_core::lock_ok!(self.conn);
        conn.execute_batch(
            "
            CREATE TABLE IF NOT EXISTS mcp_servers (
                id            TEXT PRIMARY KEY NOT NULL,
                name          TEXT NOT NULL,
                data          TEXT NOT NULL,
                health_status TEXT NOT NULL DEFAULT 'Unknown',
                shared        INTEGER NOT NULL DEFAULT 0,
                tags          TEXT NOT NULL DEFAULT '',
                created_at    TEXT NOT NULL DEFAULT (datetime('now'))
            );

            CREATE INDEX IF NOT EXISTS idx_mcp_servers_health
                ON mcp_servers(health_status);
            CREATE INDEX IF NOT EXISTS idx_mcp_servers_shared
                ON mcp_servers(shared);
            CREATE INDEX IF NOT EXISTS idx_mcp_servers_name
                ON mcp_servers(name);

            CREATE TABLE IF NOT EXISTS mcp_process_state (
                server_id TEXT PRIMARY KEY NOT NULL,
                state     TEXT NOT NULL DEFAULT 'stopped',
                pid       INTEGER,
                last_seen_at TEXT NOT NULL DEFAULT (datetime('now'))
            );
            ",
        )?;
        Ok(())
    }

    /// Insert or replace a server entry.
    ///
    /// Does not affect the process reference count — refcounts track only
    /// live process acquisitions (see `acquire`/`release` in [`McpPool`]).
    pub fn upsert(&self, entry: &McpServerEntry) -> Result<()> {
        let data = serde_json::to_string(entry).context("Failed to serialise McpServerEntry")?;
        let health = serde_json::to_string(&entry.health_status)?;
        let shared = if entry.shared { 1 } else { 0 };
        let tags = entry.tags.join(",");

        let conn = loom_core::lock_ok!(self.conn);
        conn.execute(
            "INSERT OR REPLACE INTO mcp_servers (id, name, data, health_status, shared, tags)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![entry.id, entry.name, data, health, shared, tags],
        )?;

        Ok(())
    }

    /// Retrieve a server by id.
    pub fn get(&self, id: &str) -> Result<Option<McpServerEntry>> {
        let conn = loom_core::lock_ok!(self.conn);
        let mut stmt = conn.prepare("SELECT data FROM mcp_servers WHERE id = ?1")?;
        let mut rows = stmt.query(params![id])?;
        match rows.next()? {
            Some(row) => {
                let data: String = row.get(0)?;
                let entry: McpServerEntry =
                    serde_json::from_str(&data).context("Failed to deserialise McpServerEntry")?;
                Ok(Some(entry))
            }
            None => Ok(None),
        }
    }

    /// Delete a server by id and clear any residual tracking state.
    pub fn delete(&self, id: &str) -> Result<bool> {
        let conn = loom_core::lock_ok!(self.conn);
        let affected = conn.execute("DELETE FROM mcp_servers WHERE id = ?1", params![id])?;

        // Clear any residual reference count (server no longer exists)
        {
            let mut ref_counts = loom_core::lock_ok!(self.ref_counts);
            ref_counts.remove(id);
        }

        // Reset process state when entry deleted
        let _ = conn.execute(
            "DELETE FROM mcp_process_state WHERE server_id = ?1",
            params![id],
        )?;

        Ok(affected > 0)
    }

    /// List all registered servers.
    pub fn list(&self) -> Result<Vec<McpServerEntry>> {
        let conn = loom_core::lock_ok!(self.conn);
        let mut stmt = conn.prepare("SELECT data FROM mcp_servers ORDER BY name")?;
        let rows = stmt.query_map([], |row| {
            let data: String = row.get(0)?;
            Ok(data)
        })?;

        let mut entries = Vec::new();
        for row in rows {
            let data = row?;
            let entry: McpServerEntry =
                serde_json::from_str(&data).context("Failed to deserialise McpServerEntry")?;
            entries.push(entry);
        }
        Ok(entries)
    }

    /// Count total entries.
    pub fn count(&self) -> Result<usize> {
        let conn = loom_core::lock_ok!(self.conn);
        let count: usize = conn.query_row("SELECT COUNT(*) FROM mcp_servers", [], |row| {
            row.get(0)
        })?;
        Ok(count)
    }

    /// Lookup by health status (uses secondary index).
    pub fn filter_by_health(&self, status: &HealthStatusStr) -> Result<Vec<McpServerEntry>> {
        let health = serde_json::to_string(status)?;
        self.query_by_column("health_status", &health)
    }

    /// Lookup shared (or non-shared) servers (uses secondary index).
    pub fn filter_shared(&self, shared: bool) -> Result<Vec<McpServerEntry>> {
        let shared_int: i32 = if shared { 1 } else { 0 };
        self.query_by_column("shared", &shared_int.to_string())
    }

    /// Lookup servers whose tags contain the given tag (LIKE scan over
    /// the comma-separated tag list; consider a tag-junction table for
    /// higher cardinality).
    pub fn filter_by_tag(&self, tag: &str) -> Result<Vec<McpServerEntry>> {
        let conn = loom_core::lock_ok!(self.conn);
        let pattern = format!("%{}%", tag);
        let mut stmt =
            conn.prepare("SELECT data FROM mcp_servers WHERE tags LIKE ?1 ORDER BY name")?;
        let rows = stmt.query_map(params![pattern], |row| {
            let data: String = row.get(0)?;
            Ok(data)
        })?;

        let mut entries = Vec::new();
        for row in rows {
            let data = row?;
            let entry: McpServerEntry =
                serde_json::from_str(&data).context("Failed to deserialise McpServerEntry")?;
            entries.push(entry);
        }
        Ok(entries)
    }

    /// Update only the health status of a server (avoids full serialisation).
    pub fn update_health(&self, id: &str, status: &HealthStatusStr) -> Result<bool> {
        let health = serde_json::to_string(status)?;
        let conn = loom_core::lock_ok!(self.conn);
        let affected =
            conn.execute(
                "UPDATE mcp_servers SET health_status = ?1 WHERE id = ?2",
                params![health, id],
            )?;

        if affected > 0 {
            let mut stmt = conn.prepare("SELECT data FROM mcp_servers WHERE id = ?1")?;
            let mut rows = stmt.query(params![id])?;
            if let Some(row) = rows.next()? {
                let data: String = row.get(0)?;
                let mut entry: McpServerEntry =
                    serde_json::from_str(&data).context("Failed to deserialise McpServerEntry")?;
                entry.health_status = status.clone();
                entry.last_health_check = loom_core::rfc3339_now();
                let full_data = serde_json::to_string(&entry)?;
                conn.execute(
                    "UPDATE mcp_servers SET data = ?1 WHERE id = ?2",
                    params![full_data, id],
                )?;
            }
        }
        Ok(affected > 0)
    }

    /// Increment reference count for a server (tracks process reuse).
    pub fn increment_refcount(&self, id: &str) -> Result<()> {
        let mut ref_counts = loom_core::lock_ok!(self.ref_counts);
        *ref_counts.entry(id.to_string()).or_insert(0) += 1;
        Ok(())
    }

    /// Decrement reference count for a server.
    pub fn decrement_refcount(&self, id: &str) -> Result<()> {
        let mut ref_counts = loom_core::lock_ok!(self.ref_counts);
        if let Some(count) = ref_counts.get_mut(id) {
            *count = count.saturating_sub(1);
            if *count == 0 {
                ref_counts.remove(id);
            }
        }
        Ok(())
    }

    /// Get current reference count for a server.
    pub fn get_refcount(&self, id: &str) -> usize {
        let ref_counts = loom_core::lock_ok!(self.ref_counts);
        *ref_counts.get(id).unwrap_or(&0)
    }

    /// Set process state for a server.
    pub fn set_process_state(&self, server_id: &str, state: &str, pid: Option<i32>) -> Result<()> {
        let conn = loom_core::lock_ok!(self.conn);
        conn.execute(
            "INSERT OR REPLACE INTO mcp_process_state (server_id, state, pid)
             VALUES (?1, ?2, ?3)",
            params![server_id, state, pid],
        )?;
        Ok(())
    }

    /// Get process state for a server.
    pub fn get_process_state(&self, server_id: &str) -> Result<Option<(String, Option<i32>)>> {
        let conn = loom_core::lock_ok!(self.conn);
        let mut stmt = conn.prepare(
            "SELECT state, pid FROM mcp_process_state WHERE server_id = ?1",
        )?;
        let mut rows = stmt.query(params![server_id])?;
        match rows.next()? {
            Some(row) => {
                let state: String = row.get(0)?;
                let pid: Option<i32> = row.get(1)?;
                Ok(Some((state, pid)))
            }
            None => Ok(None),
        }
    }

    /// Reset process state for all servers (on daemon restart).
    pub fn reset_process_state(&self) -> Result<()> {
        let conn = loom_core::lock_ok!(self.conn);
        conn.execute_batch("DELETE FROM mcp_process_state;")?;
        let mut ref_counts = loom_core::lock_ok!(self.ref_counts);
        ref_counts.clear();
        Ok(())
    }

    fn query_by_column(&self, column: &str, value: &str) -> Result<Vec<McpServerEntry>> {
        let conn = loom_core::lock_ok!(self.conn);
        let sql = format!(
            "SELECT data FROM mcp_servers WHERE {} = ?1 ORDER BY name",
            column
        );
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt.query_map(params![value], |row| {
            let data: String = row.get(0)?;
            Ok(data)
        })?;

        let mut entries = Vec::new();
        for row in rows {
            let data = row?;
            let entry: McpServerEntry =
                serde_json::from_str(&data).context("Failed to deserialise McpServerEntry")?;
            entries.push(entry);
        }
        Ok(entries)
    }

    pub fn db_path(&self) -> &Path {
        &self.db_path
    }
}

/// Create default MCP server entries.
fn default_entries() -> Vec<McpServerEntry> {
    vec![
        McpServerEntry {
            id: "context-mode".to_string(),
            name: "context-mode".to_string(),
            transport: McpTransport {
                transport_type: McpTransportType::Stdio,
                command: Some("context".to_string()),
                args: vec![],
                url: None,
                env: std::collections::HashMap::new(),
            },
            shared: true,
            tools_filter: vec![],
            health_status: HealthStatusStr::Unknown,
            last_health_check: String::new(),
            tags: vec!["builtin".to_string()],
            compatible_tools: vec![],
            required_by_skills: vec![],
        },
        McpServerEntry {
            id: "rtk".to_string(),
            name: "rtk".to_string(),
            transport: McpTransport {
                transport_type: McpTransportType::Stdio,
                command: Some("rtk".to_string()),
                args: vec![],
                url: None,
                env: std::collections::HashMap::new(),
            },
            shared: true,
            tools_filter: vec![],
            health_status: HealthStatusStr::Unknown,
            last_health_check: String::new(),
            tags: vec!["builtin".to_string()],
            compatible_tools: vec![],
            required_by_skills: vec![],
        },
        McpServerEntry {
            id: "loom".to_string(),
            name: "loom".to_string(),
            transport: McpTransport {
                transport_type: McpTransportType::Stdio,
                command: Some("loom-mcp-server".to_string()),
                args: vec![],
                url: None,
                env: std::collections::HashMap::new(),
            },
            shared: true,
            tools_filter: vec![],
            health_status: HealthStatusStr::Unknown,
            last_health_check: String::new(),
            tags: vec!["builtin".to_string()],
            compatible_tools: vec![],
            required_by_skills: vec![],
        },
        McpServerEntry {
            id: "agentmemory".to_string(),
            name: "agentmemory".to_string(),
            transport: McpTransport {
                transport_type: McpTransportType::Stdio,
                command: Some("agentmemory".to_string()),
                args: vec![],
                url: None,
                env: std::collections::HashMap::new(),
            },
            shared: true,
            tools_filter: vec![],
            health_status: HealthStatusStr::Unknown,
            last_health_check: String::new(),
            tags: vec!["builtin".to_string()],
            compatible_tools: vec![],
            required_by_skills: vec![],
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use loom_core::{McpTransport, McpTransportType};

    fn sample_entry(id: &str, name: &str) -> McpServerEntry {
        McpServerEntry {
            id: id.to_string(),
            name: name.to_string(),
            transport: McpTransport {
                transport_type: McpTransportType::Stdio,
                command: Some("echo".to_string()),
                args: vec![],
                url: None,
                env: std::collections::HashMap::new(),
            },
            shared: true,
            tools_filter: vec![],
            health_status: HealthStatusStr::Unknown,
            last_health_check: "2026-01-01T00:00:00Z".to_string(),
            tags: vec!["test".to_string(), "example".to_string()],
            compatible_tools: vec![],
            required_by_skills: vec![],
        }
    }

    #[test]
    fn test_upsert_and_get() {
        let reg = McpRegistry::in_memory().unwrap();
        let entry = sample_entry("srv-1", "TestServer");
        reg.upsert(&entry).unwrap();

        let fetched = reg.get("srv-1").unwrap().expect("should exist");
        assert_eq!(fetched.id, "srv-1");
        assert_eq!(fetched.name, "TestServer");
    }

    #[test]
    fn test_seed_defaults_only_on_empty_table() {
        let dir = tempfile::tempdir().unwrap();
        let reg = McpRegistry::new(dir.path().to_str().unwrap()).unwrap();
        // A fresh low-level registry has no defaults until seed_defaults() runs.
        assert_eq!(reg.count().unwrap(), 0);

        // Seeding an empty table adds the 4 built-in defaults.
        reg.seed_defaults().unwrap();
        assert_eq!(reg.count().unwrap(), 4);

        // Adding a user server does not re-seed, but the count reflects both
        // defaults and the user entry (guard prevents duplicate re-seeding).
        let user = sample_entry("user-srv", "UserServer");
        reg.upsert(&user).unwrap();
        assert_eq!(reg.count().unwrap(), 5);

        // Re-running seeding is a no-op: defaults are not duplicated.
        reg.seed_defaults().unwrap();
        assert_eq!(reg.count().unwrap(), 5);

        // The user's entry is preserved (nothing is wiped or overwritten).
        let fetched = reg.get("user-srv").unwrap().expect("user entry preserved");
        assert_eq!(fetched.name, "UserServer");
    }

    #[test]
    fn test_upsert_does_not_affect_refcount() {
        let reg = McpRegistry::in_memory().unwrap();
        let entry = sample_entry("srv-1", "TestServer");
        reg.upsert(&entry).unwrap();
        // A registration alone is not an acquisition; refcount stays 0
        assert_eq!(reg.get_refcount("srv-1"), 0);

        reg.delete("srv-1").unwrap();
        assert_eq!(reg.get_refcount("srv-1"), 0);
    }

    #[test]
    fn test_increment_refcount() {
        let reg = McpRegistry::in_memory().unwrap();
        let entry = sample_entry("srv-1", "TestServer");
        reg.upsert(&entry).unwrap();

        reg.increment_refcount("srv-1").unwrap();
        assert_eq!(reg.get_refcount("srv-1"), 1);

        reg.decrement_refcount("srv-1").unwrap();
        assert_eq!(reg.get_refcount("srv-1"), 0);
    }

    #[test]
    fn test_filter_by_health() {
        let reg = McpRegistry::in_memory().unwrap();
        let mut entry = sample_entry("srv-1", "Srv1");
        entry.health_status = HealthStatusStr::Healthy;
        reg.upsert(&entry).unwrap();

        let mut entry2 = sample_entry("srv-2", "Srv2");
        entry2.health_status = HealthStatusStr::Offline;
        reg.upsert(&entry2).unwrap();

        let healthy = reg.filter_by_health(&HealthStatusStr::Healthy).unwrap();
        assert_eq!(healthy.len(), 1);
        assert_eq!(healthy[0].id, "srv-1");
    }

    #[test]
    fn test_filter_shared() {
        let reg = McpRegistry::in_memory().unwrap();
        let mut entry = sample_entry("srv-1", "Shared");
        entry.shared = true;
        reg.upsert(&entry).unwrap();

        let mut entry2 = sample_entry("srv-2", "Private");
        entry2.shared = false;
        reg.upsert(&entry2).unwrap();

        let shared = reg.filter_shared(true).unwrap();
        assert_eq!(shared.len(), 1);
        assert_eq!(shared[0].id, "srv-1");
    }

    #[test]
    fn test_filter_by_tag() {
        let reg = McpRegistry::in_memory().unwrap();
        let mut entry = sample_entry("srv-1", "Tagged");
        entry.tags = vec!["important".to_string(), "production".to_string()];
        reg.upsert(&entry).unwrap();

        let results = reg.filter_by_tag("important").unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].id, "srv-1");
    }

    #[test]
    fn test_update_health() {
        let reg = McpRegistry::in_memory().unwrap();
        reg.upsert(&sample_entry("srv-1", "Test")).unwrap();
        assert!(
            reg.update_health("srv-1", &HealthStatusStr::Healthy).unwrap()
        );
        let entry = reg.get("srv-1").unwrap().unwrap();
        assert!(matches!(entry.health_status, HealthStatusStr::Healthy));
    }

    #[test]
    fn test_process_state_tracking() {
        let reg = McpRegistry::in_memory().unwrap();
        let entry = sample_entry("srv-1", "ProcessTest");

        reg.upsert(&entry).unwrap();

        reg.set_process_state("srv-1", "running", Some(1234)).unwrap();
        let state = reg.get_process_state("srv-1").unwrap();
        assert!(state.is_some());
        let (state_val, pid_val) = state.unwrap();
        assert_eq!(state_val, "running");
        assert_eq!(pid_val, Some(1234));
    }

    #[test]
    fn test_reset_process_state() {
        let reg = McpRegistry::in_memory().unwrap();
        let entry = sample_entry("srv-1", "ResetTest");
        reg.upsert(&entry).unwrap();

        reg.set_process_state("srv-1", "running", Some(1234)).unwrap();
        reg.reset_process_state().unwrap();

        let state = reg.get_process_state("srv-1").unwrap();
        assert!(state.is_none());
    }

    #[test]
    fn test_shared_registry_clone() {
        let reg = McpRegistry::in_memory().unwrap();
        let reg_clone = reg.clone();

        // Both should share the same underlying connection
        assert!(reg.get("nonexistent").unwrap().is_none());
        assert!(reg_clone.get("nonexistent").unwrap().is_none());
    }
}