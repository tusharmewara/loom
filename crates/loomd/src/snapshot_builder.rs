//! Loom snapshot builder.
//!
//! Maintains a SQLite snapshot of all resources (skills, MCP servers, sessions, providers, hooks)
//! for fast discovery. Initial snapshot is built by walking ~/.loom/, then updated incrementally
//! via EventBus events.

use anyhow::{Context, Result};
use rusqlite::{Connection, params, Row};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::Mutex;

/// Lazy-initializable snapshot DB connection.
#[derive(Clone)]
struct SnapshotDb {
    conn: Arc<Mutex<Connection>>,
    path: PathBuf,
}

impl SnapshotDb {
    fn new(loom_dir: &str) -> Result<Self> {
        let cache_dir = Path::new(loom_dir).join(".cache");
        std::fs::create_dir_all(&cache_dir)?;
        let db_path = cache_dir.join("snapshot.db");
        let conn = loom_core::db::open(&db_path)?;
        conn.execute_batch(
            r#"
            CREATE TABLE IF NOT EXISTS skills (
                id TEXT PRIMARY KEY NOT NULL,
                name TEXT NOT NULL,
                path TEXT NOT NULL,
                content_hash TEXT NOT NULL,
                tags TEXT NOT NULL DEFAULT '',
                ecosystem_compatible TEXT NOT NULL DEFAULT '',
                ecosystem_requires TEXT NOT NULL DEFAULT '',
                version TEXT NOT NULL,
                last_modified TEXT NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_skills_name ON skills(name);
            CREATE INDEX IF NOT EXISTS idx_skills_tags ON skills(tags);

            CREATE TABLE IF NOT EXISTS mcp_servers (
                id TEXT PRIMARY KEY NOT NULL,
                name TEXT NOT NULL,
                data TEXT NOT NULL,
                health_status TEXT NOT NULL DEFAULT 'Unknown',
                shared INTEGER NOT NULL DEFAULT 0,
                tags TEXT NOT NULL DEFAULT '',
                created_at TEXT NOT NULL DEFAULT (datetime('now'))
            );
            CREATE INDEX IF NOT EXISTS idx_mcp_health ON mcp_servers(health_status);
            CREATE INDEX IF NOT EXISTS idx_mcp_shared ON mcp_servers(shared);

            CREATE TABLE IF NOT EXISTS sessions (
                session_id TEXT PRIMARY KEY NOT NULL,
                source_tool TEXT NOT NULL,
                source_version TEXT NOT NULL,
                exported_at TEXT NOT NULL,
                topic TEXT NOT NULL,
                message_count INTEGER NOT NULL DEFAULT 0,
                active_skills TEXT NOT NULL DEFAULT '',
                active_mcp_servers TEXT NOT NULL DEFAULT '',
                working_directory TEXT NOT NULL,
                model_provider TEXT NOT NULL,
                files_modified TEXT NOT NULL DEFAULT '',
                compatibility_note TEXT NOT NULL DEFAULT ''
            );

            CREATE TABLE IF NOT EXISTS providers (
                provider_id TEXT PRIMARY KEY NOT NULL,
                name TEXT NOT NULL,
                auth_method TEXT NOT NULL,
                api_base_url TEXT NOT NULL,
                models_endpoint TEXT NOT NULL,
                credential_ref TEXT NOT NULL,
                status TEXT NOT NULL DEFAULT 'Unconfigured',
                last_validated TEXT NOT NULL DEFAULT ''
            );

            CREATE TABLE IF NOT EXISTS hooks (
                id TEXT PRIMARY KEY NOT NULL,
                event_type TEXT NOT NULL,
                matcher TEXT NOT NULL DEFAULT '',
                script_path TEXT NOT NULL,
                timeout_seconds INTEGER NOT NULL DEFAULT 30,
                priority TEXT NOT NULL DEFAULT 'Ecosystem'
            );
            "#,
        )?;

        Ok(Self {
            conn: Arc::new(Mutex::new(conn)),
            path: db_path,
        })
    }

    fn conn(&self) -> rusqlite::Result<Connection> {
        Ok(self.conn.blocking_lock().clone())
    }

    /// Build initial snapshot by scanning the ~/.loom/ tree.
    pub fn build_initial(&self, loom_dir: &str) -> Result<()> {
        let skills_dir = Path::new(loom_dir).join("skills");
        if skills_dir.exists() {
            for entry in std::fs::read_dir(&skills_dir)? {
                let entry = entry?;
                let path = entry.path();
                if path.extension().and_then(|e| e.to_str()) == Some("md") {
                    if let Some(name) = path.file_stem().and_then(|s| s.to_str()) {
                        let content = std::fs::read_to_string(&path)?;
                        let hash = format!("{:x}", sha2::Sha256::digest(content.as_bytes()));
                        self.conn()
                            .execute(
                                "INSERT OR REPLACE INTO skills (id, name, path, content_hash, version, last_modified) VALUES (?, ?, ?, ?, ?, ?)",
                                params![name, name, path.to_str()?, hash, "1.0.0", loom_core::rfc3339_now()],
                            )?;
                    }
                }
            }
        }

        let mcp_dir = Path::new(loom_dir).join("mcp");
        if mcp_dir.exists() {
            for entry in std::fs::read_dir(&mcp_dir)? {
                let entry = entry?;
                let path = entry.path();
                if path.extension().and_then(|e| e.to_str()) == Some("toml") {
                    if let Ok(content) = std::fs::read_to_string(&path) {
                        // Parse MCP server entries from TOML
                        // For now, just store the path
                        self.conn()
                            .execute(
                                "INSERT OR REPLACE INTO mcp_servers (id, name, data, created_at) VALUES (?, ?, ?, ?)",
                                params![
                                    path.file_name().and_then(|n| n.to_str()).unwrap_or("unknown"),
                                    path.file_name().and_then(|n| n.to_str()).unwrap_or("unknown"),
                                    content,
                                    loom_core::rfc3339_now()
                                ],
                            )?;
                    }
                }
            }
        }

        tracing::info!("Initial snapshot built at {}", self.path.display());
        Ok(())
    }

    /// Update snapshot incrementally for a single resource.
    pub fn update_skill(&self, id: &str, name: &str, path: &str, hash: &str) -> Result<()> {
        self.conn()
            .execute(
                "INSERT OR REPLACE INTO skills (id, name, path, content_hash, last_modified) VALUES (?, ?, ?, ?, ?)",
                params![id, name, path, hash, loom_core::rfc3339_now()],
            )?;
        Ok(())
    }

    pub fn update_mcp_server(&self, id: &str, name: &str, data: &str) -> Result<()> {
        self.conn()
            .execute(
                "INSERT OR REPLACE INTO mcp_servers (id, name, data, created_at) VALUES (?, ?, ?, ?)",
                params![id, name, data, loom_core::rfc3339_now()],
            )?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_build_initial_snapshot() -> anyhow::Result<()> {
        let dir = tempdir()?;
        let loom_dir = dir.path().to_str().unwrap();

        // Create test skill
        let skills_dir = dir.path().join("skills");
        std::fs::create_dir_all(&skills_dir)?;
        let skill_file = skills_dir.join("test-skill.md");
        std::fs::write(&skill_file, "# Test Skill")?;

        let snapshot = SnapshotDb::new(loom_dir)?;
        snapshot.build_initial(loom_dir)?;

        // Verify skill was added
        let conn = snapshot.conn()?;
        let count: i32 = conn.query_row("SELECT COUNT(*) FROM skills", [], |row| row.get(0))?;
        assert_eq!(count, 1);

        Ok(())
    }

    #[test]
    fn test_update_skill() -> anyhow::Result<()> {
        let dir = tempdir()?;
        let loom_dir = dir.path().to_str().unwrap();

        let snapshot = SnapshotDb::new(loom_dir)?;
        snapshot.update_skill("test-id", "test-skill", "/path/to/skill.md", "abc123")?;

        let conn = snapshot.conn()?;
        let count: i32 = conn.query_row("SELECT COUNT(*) FROM skills WHERE id = ?", ["test-id"], |row| row.get(0))?;
        assert_eq!(count, 1);

        Ok(())
    }
}
