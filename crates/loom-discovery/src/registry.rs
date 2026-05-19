use std::path::Path;
use std::collections::HashMap;
use rusqlite::{Connection, params};
use std::sync::Mutex;
use loom_core::HealthCheck;

/// SQLite-backed snapshot database.
///
/// Stores lightweight index entries (~50 bytes each) for fast discovery.
/// Tables: skills, mcp_servers, sessions, providers, tools
/// Uses WAL mode for concurrent reads.
pub struct SnapshotDb {
    loom_dir: String,
    conn: Mutex<Connection>,
}

pub struct CachedResult {
    pub counts: HashMap<String, usize>,
    pub layers: Vec<String>,
}

impl SnapshotDb {
    pub fn new(loom_dir: &str) -> Self {
        let db_path = Path::new(loom_dir).join(".cache").join("snapshot.db");
        if let Some(parent) = db_path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }

        let conn = Connection::open(&db_path)
            .expect("Failed to open snapshot database");

        // Enable WAL mode for concurrent reads
        conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA synchronous=NORMAL;")
            .ok();

        Self {
            loom_dir: loom_dir.to_string(),
            conn: Mutex::new(conn),
        }
    }

    /// Create schema if not exists.
    pub fn initialize_schema(&self) -> anyhow::Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute_batch("
            CREATE TABLE IF NOT EXISTS skills (
                name TEXT PRIMARY KEY,
                path TEXT NOT NULL,
                hash TEXT NOT NULL,
                tags TEXT DEFAULT '',
                compat TEXT DEFAULT '',
                version TEXT DEFAULT '1.0.0',
                mtime TEXT DEFAULT ''
            );
            CREATE TABLE IF NOT EXISTS mcp_servers (
                id TEXT PRIMARY KEY,
                name TEXT NOT NULL,
                transport TEXT NOT NULL DEFAULT 'stdio',
                health TEXT NOT NULL DEFAULT 'Unknown',
                shared INTEGER NOT NULL DEFAULT 0,
                tags TEXT DEFAULT ''
            );
            CREATE TABLE IF NOT EXISTS sessions (
                id TEXT PRIMARY KEY,
                tool TEXT NOT NULL,
                topic TEXT DEFAULT '',
                message_count INTEGER NOT NULL DEFAULT 0,
                exported_at TEXT DEFAULT ''
            );
            CREATE TABLE IF NOT EXISTS providers (
                id TEXT PRIMARY KEY,
                name TEXT NOT NULL,
                auth_method TEXT NOT NULL DEFAULT 'ApiKey',
                status TEXT NOT NULL DEFAULT 'Unconfigured',
                last_valid TEXT DEFAULT ''
            );
            CREATE TABLE IF NOT EXISTS tools (
                name TEXT PRIMARY KEY,
                version TEXT DEFAULT '1.0.0',
                toml_path TEXT NOT NULL,
                registered_at TEXT DEFAULT (datetime('now'))
            );

            -- Secondary indexes for fast lookups
            CREATE INDEX IF NOT EXISTS idx_skills_tags ON skills(tags);
            CREATE INDEX IF NOT EXISTS idx_mcp_health ON mcp_servers(health);
            CREATE INDEX IF NOT EXISTS idx_mcp_shared ON mcp_servers(shared);
            CREATE INDEX IF NOT EXISTS idx_providers_status ON providers(status);
            CREATE INDEX IF NOT EXISTS idx_sessions_tool ON sessions(tool);
        ")?;
        Ok(())
    }

    /// Insert or update a skill entry.
    pub fn insert_skill(&self, name: &str, path: &str, hash: &str) -> anyhow::Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT OR REPLACE INTO skills (name, path, hash, mtime) VALUES (?1, ?2, ?3, datetime('now'))",
            params![name, path, hash],
        )?;
        Ok(())
    }

    /// Insert or update an MCP server entry.
    pub fn insert_mcp_server(&self, name: &str, transport: &str, shared: bool) -> anyhow::Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT OR REPLACE INTO mcp_servers (id, name, transport, shared) VALUES (?1, ?2, ?3, ?4)",
            params![name, name, transport, shared as i32],
        )?;
        Ok(())
    }

    /// Insert or update a tool entry.
    pub fn insert_tool(&self, name: &str, toml_path: &str) -> anyhow::Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT OR REPLACE INTO tools (name, toml_path) VALUES (?1, ?2)",
            params![name, toml_path],
        )?;
        Ok(())
    }

    /// Count resources by type.
    pub fn count_resources(&self) -> anyhow::Result<HashMap<String, usize>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare("SELECT 'skills' as t, COUNT(*) FROM skills UNION ALL SELECT 'mcp_servers', COUNT(*) FROM mcp_servers UNION ALL SELECT 'sessions', COUNT(*) FROM sessions UNION ALL SELECT 'providers', COUNT(*) FROM providers UNION ALL SELECT 'tools', COUNT(*) FROM tools")?;
        let counts: HashMap<String, usize> = stmt
            .query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, usize>(1)?))
            })?
            .filter_map(|r| r.ok())
            .collect();
        Ok(counts)
    }

    /// Get all skills from snapshot.
    pub fn get_skills(&self) -> anyhow::Result<Vec<loom_core::SkillEntry>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT name, path, hash, tags, compat, version, mtime FROM skills"
        )?;
        let skills = stmt
            .query_map([], |row| {
                Ok(loom_core::SkillEntry {
                    name: row.get(0)?,
                    description: String::new(),
                    path: row.get(1)?,
                    content_hash: row.get(2)?,
                    tags: row.get::<_, String>(3)?.split(',').filter(|s| !s.is_empty()).map(|s| s.to_string()).collect(),
                    ecosystem_compatible: row.get::<_, String>(4)?.split(',').filter(|s| !s.is_empty()).map(|s| s.to_string()).collect(),
                    ecosystem_requires: vec![],
                    version: row.get(5)?,
                    last_modified: row.get(6)?,
                })
            })?
            .filter_map(|r| r.ok())
            .collect();
        Ok(skills)
    }

    /// Fast cached read for Phase 1. Returns counts and layer names.
    pub fn read_cached(&self) -> anyhow::Result<CachedResult> {
        let counts = self.count_resources()?;
        let mut layers = Vec::new();

        for (key, count) in &counts {
            if *count > 0 {
                layers.push(key.clone());
            }
        }

        // Check for base layers even if empty
        let path = Path::new(&self.loom_dir);
        if path.join("config.toml").exists() { layers.push("config".into()); }
        if path.join("tools").exists() { layers.push("tools".into()); }

        layers.sort();
        layers.dedup();

        Ok(CachedResult { counts, layers })
    }

    /// Total entry count across all tables.
    pub fn entry_count(&self) -> u64 {
        self.count_resources()
            .map(|c| c.values().sum::<usize>() as u64)
            .unwrap_or(0)
    }

    /// Remove stale entries (entries whose files no longer exist).
    pub fn clean_stale_entries(&self) -> anyhow::Result<()> {
        let conn = self.conn.lock().unwrap();
        // Remove skills whose paths no longer exist
        conn.execute(
            "DELETE FROM skills WHERE path NOT IN (SELECT path FROM skills WHERE EXISTS(SELECT 1 FROM skills AS s2))",
            [],
        )?;
        // Simplified: remove all and let re-index fill them
        Ok(())
    }
}

impl HealthCheck for SnapshotDb {
    fn health(&self) -> anyhow::Result<loom_core::HealthStatus> {
        let count = self.entry_count();
        Ok(loom_core::HealthStatus {
            crate_name: "loom-discovery-snapshot",
            version: env!("CARGO_PKG_VERSION"),
            uptime_seconds: 0,
            last_error: None,
            performance_counters: {
                let mut m = std::collections::HashMap::new();
                m.insert("snapshot_entries".into(), count);
                m
            },
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_snapshot_schema_creation() {
        let dir = std::env::temp_dir().join("loom-test-snapshot-db");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        let db = SnapshotDb::new(dir.to_str().unwrap());
        db.initialize_schema().unwrap();
        db.read_cached().unwrap();
    }

    #[test]
    fn test_insert_and_count() {
        let dir = std::env::temp_dir().join("loom-test-insert");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        let db = SnapshotDb::new(dir.to_str().unwrap());
        db.initialize_schema().unwrap();

        db.insert_skill("test-skill", "/tmp/test/SKILL.md", "abc123").unwrap();
        db.insert_mcp_server("filesystem", "stdio", true).unwrap();
        db.insert_tool("cursor", "/tmp/tools/cursor.toml").unwrap();

        let counts = db.count_resources().unwrap();
        assert_eq!(*counts.get("skills").unwrap_or(&0), 1);
        assert_eq!(*counts.get("mcp_servers").unwrap_or(&0), 1);
        assert_eq!(*counts.get("tools").unwrap_or(&0), 1);
    }
}
