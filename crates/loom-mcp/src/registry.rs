//! SQLite-backed registry for MCP server entries.
//!
//! Stores [`McpServerEntry`] values with secondary indexes on `id`,
//! `health_status`, `shared`, and `tags` for fast lookups.

use anyhow::{Context, Result};
use loom_core::{HealthStatusStr, McpServerEntry};
use rusqlite::{params, Connection};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

/// SQLite-backed MCP server registry with secondary indexes.
pub struct McpRegistry {
    conn: Mutex<Connection>,
    _db_path: PathBuf,
}

impl McpRegistry {
    /// Open (or create) the SQLite database at `loom_dir / mcp_registry.db`.
    pub fn new(loom_dir: &str) -> Result<Self> {
        let db_dir = Path::new(loom_dir);
        std::fs::create_dir_all(db_dir).context("Failed to create agents dir for registry")?;

        let db_path = db_dir.join("mcp_registry.db");
        let conn = Connection::open(&db_path)
            .with_context(|| format!("Failed to open registry DB at {:?}", db_path))?;

        // Enable WAL mode for concurrent readers
        conn.execute_batch("PRAGMA journal_mode=WAL;")?;

        let registry = Self {
            conn: Mutex::new(conn),
            _db_path: db_path,
        };
        registry.initialize_tables()?;
        Ok(registry)
    }

    /// Open an in-memory database (for tests).
    pub fn in_memory() -> Result<Self> {
        let conn = Connection::open_in_memory()?;
        let registry = Self {
            conn: Mutex::new(conn),
            _db_path: PathBuf::from(":memory:"),
        };
        registry.initialize_tables()?;
        Ok(registry)
    }

    /// Initialise schema and secondary indexes.
    fn initialize_tables(&self) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute_batch(
            "
            CREATE TABLE IF NOT EXISTS mcp_servers (
                id            TEXT PRIMARY KEY NOT NULL,
                name          TEXT NOT NULL,
                data          TEXT NOT NULL,       -- JSON-serialised McpServerEntry
                health_status TEXT NOT NULL DEFAULT 'Unknown',
                shared        INTEGER NOT NULL DEFAULT 0,
                tags          TEXT NOT NULL DEFAULT '',  -- comma-separated
                created_at    TEXT NOT NULL DEFAULT (datetime('now'))
            );

            -- Secondary indexes
            CREATE INDEX IF NOT EXISTS idx_mcp_servers_health
                ON mcp_servers(health_status);
            CREATE INDEX IF NOT EXISTS idx_mcp_servers_shared
                ON mcp_servers(shared);
            ",
        )?;
        Ok(())
    }

    // ── CRUD ──────────────────────────────────────────────────────────

    /// Insert or replace a server entry.
    pub fn upsert(&self, entry: &McpServerEntry) -> Result<()> {
        let data = serde_json::to_string(entry).context("Failed to serialise McpServerEntry")?;
        let health = serde_json::to_string(&entry.health_status)?;
        let shared = if entry.shared { 1 } else { 0 };
        let tags = entry.tags.join(",");

        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT OR REPLACE INTO mcp_servers (id, name, data, health_status, shared, tags)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![entry.id, entry.name, data, health, shared, tags],
        )?;
        Ok(())
    }

    /// Retrieve a server by id.
    pub fn get(&self, id: &str) -> Result<Option<McpServerEntry>> {
        let conn = self.conn.lock().unwrap();
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

    /// Delete a server by id.
    pub fn delete(&self, id: &str) -> Result<bool> {
        let conn = self.conn.lock().unwrap();
        let affected = conn.execute("DELETE FROM mcp_servers WHERE id = ?1", params![id])?;
        Ok(affected > 0)
    }

    /// List all registered servers.
    pub fn list(&self) -> Result<Vec<McpServerEntry>> {
        let conn = self.conn.lock().unwrap();
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
        let conn = self.conn.lock().unwrap();
        let count: usize = conn.query_row("SELECT COUNT(*) FROM mcp_servers", [], |row| {
            row.get(0)
        })?;
        Ok(count)
    }

    // ── Indexed lookups ───────────────────────────────────────────────

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
        let conn = self.conn.lock().unwrap();
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
        let conn = self.conn.lock().unwrap();
        let affected =
            conn.execute(
                "UPDATE mcp_servers SET health_status = ?1 WHERE id = ?2",
                params![health, id],
            )?;
        if affected > 0 {
            // Also update the JSON data field so the full entry stays consistent
            // Get the current data directly using the existing connection
            let mut stmt = conn.prepare("SELECT data FROM mcp_servers WHERE id = ?1")?;
            let mut rows = stmt.query(params![id])?;
            if let Some(row) = rows.next()? {
                let data: String = row.get(0)?;
                let mut entry: McpServerEntry =
                    serde_json::from_str(&data).context("Failed to deserialise McpServerEntry")?;
                entry.health_status = status.clone();
                entry.last_health_check = chrono_now();
                let full_data = serde_json::to_string(&entry)?;
                conn.execute(
                    "UPDATE mcp_servers SET data = ?1 WHERE id = ?2",
                    params![full_data, id],
                )?;
            }
        }
        Ok(affected > 0)
    }

    // ── Internals ─────────────────────────────────────────────────────

    fn query_by_column(&self, column: &str, value: &str) -> Result<Vec<McpServerEntry>> {
        let conn = self.conn.lock().unwrap();
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
}

/// Simple UTC timestamp string (ISO-8601-like).
fn chrono_now() -> String {
    // Avoid pulling in chrono just for a timestamp.
    // Use std::time + manual formatting (millisecond precision).
    let dur = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    let secs = dur.as_secs();
    // Compute YMD HMS from secs since epoch (simple leap-year-aware algorithm).
    let (y, m, d, hh, mm, ss) = secs_to_datetime(secs);
    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z",
        y, m, d, hh, mm, ss
    )
}

fn secs_to_datetime(secs: u64) -> (u64, u64, u64, u64, u64, u64) {
    // Days since epoch
    let days = secs / 86400;
    let time_secs = secs % 86400;
    let hh = time_secs / 3600;
    let mm = (time_secs % 3600) / 60;
    let ss = time_secs % 60;

    // Year/month/day from days (civil date algorithm)
    let mut y = 1970i64;
    let mut d = days as i64;

    // Approx year
    y += d / 365;
    d %= 365;

    // Adjust for leap years
    loop {
        let days_this_year = if is_leap(y) { 366 } else { 365 };
        if d < days_this_year {
            break;
        }
        d -= days_this_year;
        y += 1;
    }

    let days_in_months = if is_leap(y) {
        [31, 29, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
    } else {
        [31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
    };

    let mut m = 0u64;
    for (i, &dim) in days_in_months.iter().enumerate() {
        if d < dim {
            m = (i + 1) as u64;
            break;
        }
        d -= dim;
    }
    if m == 0 {
        m = 12;
    }

    (
        y as u64,
        m,
        (d + 1) as u64,
        hh,
        mm,
        ss,
    )
}

fn is_leap(y: i64) -> bool {
    (y % 4 == 0 && y % 100 != 0) || y % 400 == 0
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
    fn test_delete() {
        let reg = McpRegistry::in_memory().unwrap();
        reg.upsert(&sample_entry("srv-1", "Test")).unwrap();
        assert!(reg.delete("srv-1").unwrap());
        assert!(!reg.delete("srv-1").unwrap());
    }

    #[test]
    fn test_list() {
        let reg = McpRegistry::in_memory().unwrap();
        reg.upsert(&sample_entry("a", "Alpha")).unwrap();
        reg.upsert(&sample_entry("b", "Beta")).unwrap();
        assert_eq!(reg.list().unwrap().len(), 2);
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
            reg.update_health("srv-1", &HealthStatusStr::Healthy)
                .unwrap()
        );
        let entry = reg.get("srv-1").unwrap().unwrap();
        assert!(matches!(entry.health_status, HealthStatusStr::Healthy));
    }
}
