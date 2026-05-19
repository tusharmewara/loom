use loom_core::SkillEntry;
use rusqlite::{params, Connection, Result};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, Mutex, RwLock};
use tracing::debug;

/// Skill registry using SQLite with secondary indexes
pub struct SkillRegistry {
    /// Connection pool - we'll use a simple Arc<Mutex<Connection>> for now
    /// In production, this could be replaced with a proper connection pool
    db: Arc<Mutex<Connection>>,
    /// Cache for faster lookups
    cache: RwLock<HashMap<String, SkillEntry>>,
}

impl SkillRegistry {
    /// Create a new skill registry pointing to the given agents directory
    pub fn new(loom_dir: &str) -> anyhow::Result<Self> {
        let skills_dir = Path::new(loom_dir).join("skills");
        std::fs::create_dir_all(&skills_dir)?;
        
        let db_path = skills_dir.join("skills.db");
        let conn = Connection::open(db_path)?;
        
        // Enable foreign keys and other pragmas
        conn.pragma_update(None, "foreign_keys", true.to_string())?;
        conn.pragma_update(None, "journal_mode", "WAL".to_string())?;
        conn.pragma_update(None, "synchronous", "NORMAL".to_string())?;
        
        // Create tables if they don't exist
        conn.execute_batch(
            r#"
            CREATE TABLE IF NOT EXISTS skills (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                name TEXT UNIQUE NOT NULL,
                description TEXT NOT NULL,
                path TEXT NOT NULL,
                content_hash TEXT NOT NULL,
                tags TEXT, -- JSON array
                ecosystem_compatible TEXT, -- JSON array
                ecosystem_requires TEXT, -- JSON array
                version TEXT NOT NULL,
                last_modified TEXT NOT NULL,
                created_at DATETIME DEFAULT CURRENT_TIMESTAMP,
                updated_at DATETIME DEFAULT CURRENT_TIMESTAMP
            );
            
            CREATE INDEX IF NOT EXISTS idx_skills_name ON skills(name);
            CREATE INDEX IF NOT EXISTS idx_skills_tags ON skills(tags);
            CREATE INDEX IF NOT EXISTS idx_skills_compat ON skills(ecosystem_compatible);
            CREATE INDEX IF NOT EXISTS idx_skills_requires ON skills(ecosystem_requires);
            CREATE INDEX IF NOT EXISTS idx_skills_updated ON skills(updated_at);
            
            CREATE TABLE IF NOT EXISTS skill_content (
                skill_id INTEGER PRIMARY KEY,
                content TEXT NOT NULL,
                FOREIGN KEY (skill_id) REFERENCES skills(id) ON DELETE CASCADE
            );
            
            CREATE INDEX IF NOT EXISTS idx_skill_content_id ON skill_content(skill_id);
        "#,
        )?;
        
        let registry = SkillRegistry {
            db: Arc::new(Mutex::new(conn)),
            cache: RwLock::new(HashMap::new()),
        };
        
        // Load existing skills into cache
        registry.refresh_cache()?;
        
        Ok(registry)
    }
    
    /// Refresh the in-memory cache from database
    pub fn refresh_cache(&self) -> anyhow::Result<()> {
        let mut cache = self.cache.write().unwrap();
        cache.clear();
        
        let lock = self.db.lock().unwrap();
        let mut stmt = lock.prepare(
            r#"
            SELECT s.name, s.description, s.path, s.content_hash, 
                   s.tags, s.ecosystem_compatible, s.ecosystem_requires,
                   s.version, s.last_modified
            FROM skills s
            ORDER BY s.name
        "#,
        )?;
        
        let skill_iter = stmt.query_map([], |row| {
            Ok(SkillEntry {
                name: row.get(0)?,
                description: row.get(1)?,
                path: row.get(2)?,
                content_hash: row.get(3)?,
                tags: serde_json::from_str(&row.get::<_, String>(4)?).unwrap_or_default(),
                ecosystem_compatible: serde_json::from_str(&row.get::<_, String>(5)?).unwrap_or_default(),
                ecosystem_requires: serde_json::from_str(&row.get::<_, String>(6)?).unwrap_or_default(),
                version: row.get(7)?,
                last_modified: row.get(8)?,
            })
        })?;
        
        for skill in skill_iter.flatten() {
            cache.insert(skill.name.clone(), skill);
        }
        
        debug!(cache_size = cache.len(), "Refreshed skill registry cache");
        Ok(())
    }
    
    /// List all skills
    pub fn list(&self) -> Vec<SkillEntry> {
        let cache = self.cache.read().unwrap();
        cache.values().cloned().collect()
    }
    
    /// Get a skill by name
    pub fn get(&self, name: &str) -> Option<SkillEntry> {
        let cache = self.cache.read().unwrap();
        cache.get(name).cloned()
    }
    
    /// Search skills by tag
    pub fn search_by_tag(&self, tag: &str) -> Vec<SkillEntry> {
        let cache = self.cache.read().unwrap();
        cache
            .values()
            .filter(|skill| skill.tags.contains(&tag.to_string()))
            .cloned()
            .collect()
    }
    
    /// Search skills by ecosystem compatibility
    pub fn search_by_compatibility(&self, ecosystem: &str) -> Vec<SkillEntry> {
        let cache = self.cache.read().unwrap();
        cache
            .values()
            .filter(|skill| skill.ecosystem_compatible.contains(&ecosystem.to_string()))
            .cloned()
            .collect()
    }
    
    /// Search skills by ecosystem requirement
    pub fn search_by_requirement(&self, requirement: &str) -> Vec<SkillEntry> {
        let cache = self.cache.read().unwrap();
        cache
            .values()
            .filter(|skill| skill.ecosystem_requires.contains(&requirement.to_string()))
            .cloned()
            .collect()
    }
    
    /// Add or update a skill
    pub fn upsert(&self, mut skill: SkillEntry) -> anyhow::Result<()> {
        // Calculate content hash if not provided
        if skill.content_hash.is_empty() && !skill.path.is_empty() {
            if let Ok(content) = std::fs::read_to_string(&skill.path) {
                let mut hasher = Sha256::new();
                hasher.update(content.as_bytes());
                skill.content_hash = format!("{:x}", hasher.finalize());
            }
        }
        
        skill.last_modified = rfc3339_now();

        let mut lock = self.db.lock().unwrap();
        let tx = lock.transaction()?;
        
        // Upsert into skills table
        tx.execute(
            r#"
            INSERT INTO skills (
                name, description, path, content_hash, 
                tags, ecosystem_compatible, ecosystem_requires,
                version, last_modified
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)
            ON CONFLICT(name) DO UPDATE SET
                description = excluded.description,
                path = excluded.path,
                content_hash = excluded.content_hash,
                tags = excluded.tags,
                ecosystem_compatible = excluded.ecosystem_compatible,
                ecosystem_requires = excluded.ecosystem_requires,
                version = excluded.version,
                last_modified = excluded.last_modified,
                updated_at = CURRENT_TIMESTAMP
            "#,
            params![
                skill.name,
                skill.description,
                skill.path,
                skill.content_hash,
                serde_json::to_string(&skill.tags)?,
                serde_json::to_string(&skill.ecosystem_compatible)?,
                serde_json::to_string(&skill.ecosystem_requires)?,
                skill.version,
                skill.last_modified
            ],
        )?;
        
        // Get the skill ID
        let skill_id: i64 = tx.query_row(
            "SELECT id FROM skills WHERE name = ?",
            params![skill.name],
            |row| row.get(0),
        )?;
        
        // Upsert content (we'll store full content for now, could be split later)
        tx.execute(
            r#"
            INSERT INTO skill_content (skill_id, content)
            VALUES (?, ?)
            ON CONFLICT(skill_id) DO UPDATE SET
                content = excluded.content
            "#,
            params![skill_id, skill.description], // Store description as content for now
        )?;
        
        tx.commit()?;
        
        // Update cache
        let mut cache = self.cache.write().unwrap();
        cache.insert(skill.name.clone(), skill);
        
        Ok(())
    }
    
    /// Remove a skill by name
    pub fn remove(&self, name: &str) -> anyhow::Result<()> {
        let mut lock = self.db.lock().unwrap();
        let tx = lock.transaction()?;
        
        tx.execute(
            "DELETE FROM skills WHERE name = ?",
            params![name],
        )?;
        
        tx.commit()?;
        
        // Update cache
        let mut cache = self.cache.write().unwrap();
        cache.remove(name);
        
        Ok(())
    }
    
    /// Get skills modified since a given timestamp
    pub fn get_modified_since(&self, since: &str) -> Vec<SkillEntry> {
        let lock = self.db.lock().unwrap();
        let mut stmt = lock.prepare(
            r#"
            SELECT s.name, s.description, s.path, s.content_hash, 
                   s.tags, s.ecosystem_compatible, s.ecosystem_requires,
                   s.version, s.last_modified
            FROM skills s
            WHERE s.updated_at > ?
            ORDER BY s.updated_at DESC
        "#,
        ).unwrap();
        
        let skill_iter = stmt.query_map(params![since], |row| {
            Ok(SkillEntry {
                name: row.get(0)?,
                description: row.get(1)?,
                path: row.get(2)?,
                content_hash: row.get(3)?,
                tags: serde_json::from_str(&row.get::<_, String>(4)?).unwrap_or_default(),
                ecosystem_compatible: serde_json::from_str(&row.get::<_, String>(5)?).unwrap_or_default(),
                ecosystem_requires: serde_json::from_str(&row.get::<_, String>(6)?).unwrap_or_default(),
                version: row.get(7)?,
                last_modified: row.get(8)?,
            })
        }).unwrap();
        
        skill_iter.filter_map(Result::ok).collect()
    }
    
    /// Get total count of skills
    pub fn count(&self) -> usize {
        let lock = self.db.lock().unwrap();
        lock.query_row("SELECT COUNT(*) FROM skills", [], |row| row.get::<_, i64>(0))
            .unwrap_or(0) as usize
    }
}

impl Drop for SkillRegistry {
    fn drop(&mut self) {
        // The Arc<Mutex<Connection>> will be cleaned up automatically
        // when all clones are dropped
    }
}

fn rfc3339_now() -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    let secs = now.as_secs();
    let days = secs / 86400;
    let time_secs = secs % 86400;
    let hours = time_secs / 3600;
    let minutes = (time_secs % 3600) / 60;
    let seconds = time_secs % 60;
    let (year, month, day) = days_to_date(days as i64 + 719468);
    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z",
        year, month, day, hours, minutes, seconds
    )
}

fn days_to_date(z: i64) -> (i64, u32, u32) {
    let z = z - 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    (y, m as u32, d as u32)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;
    
    #[test]
    fn test_registry_creation() -> anyhow::Result<()> {
        let dir = tempdir()?;
        let registry = SkillRegistry::new(dir.path().to_str().unwrap())?;
        assert_eq!(registry.count(), 0);
        Ok(())
    }
    
    #[test]
    fn test_upsert_and_get() -> anyhow::Result<()> {
        let dir = tempdir()?;
        let registry = SkillRegistry::new(dir.path().to_str().unwrap())?;
        
        let skill = SkillEntry {
            name: "test-skill".to_string(),
            description: "A test skill".to_string(),
            path: "/tmp/test-skill.md".to_string(),
            content_hash: "abc123".to_string(),
            tags: vec!["test".to_string(), "example".to_string()],
            ecosystem_compatible: vec!["loom".to_string()],
            ecosystem_requires: vec![],
            version: "1.0.0".to_string(),
            last_modified: "2023-01-01T00:00:00Z".to_string(),
        };
        
        registry.upsert(skill.clone())?;
        
        let retrieved = registry.get("test-skill").unwrap();
        assert_eq!(retrieved.name, "test-skill");
        assert_eq!(retrieved.description, "A test skill");
        assert_eq!(retrieved.tags, vec!["test", "example"]);
        assert_eq!(retrieved.ecosystem_compatible, vec!["loom"]);
        
        Ok(())
    }
    
    #[test]
    fn test_search_by_tag() -> anyhow::Result<()> {
        let dir = tempdir()?;
        let registry = SkillRegistry::new(dir.path().to_str().unwrap())?;
        
        let skill1 = SkillEntry {
            name: "skill1".to_string(),
            description: "First skill".to_string(),
            path: "/tmp/skill1.md".to_string(),
            content_hash: "hash1".to_string(),
            tags: vec!["rust".to_string(), "backend".to_string()],
            ecosystem_compatible: vec![],
            ecosystem_requires: vec![],
            version: "1.0.0".to_string(),
            last_modified: "2023-01-01T00:00:00Z".to_string(),
        };
        
        let skill2 = SkillEntry {
            name: "skill2".to_string(),
            description: "Second skill".to_string(),
            path: "/tmp/skill2.md".to_string(),
            content_hash: "hash2".to_string(),
            tags: vec!["python".to_string(), "backend".to_string()],
            ecosystem_compatible: vec![],
            ecosystem_requires: vec![],
            version: "1.0.0".to_string(),
            last_modified: "2023-01-01T00:00:00Z".to_string(),
        };
        
        registry.upsert(skill1)?;
        registry.upsert(skill2)?;
        
        let rust_skills = registry.search_by_tag("rust");
        assert_eq!(rust_skills.len(), 1);
        assert_eq!(rust_skills[0].name, "skill1");
        
        let backend_skills = registry.search_by_tag("backend");
        assert_eq!(backend_skills.len(), 2);
        
        Ok(())
    }
    
    #[test]
    fn test_remove() -> anyhow::Result<()> {
        let dir = tempdir()?;
        let registry = SkillRegistry::new(dir.path().to_str().unwrap())?;
        
        let skill = SkillEntry {
            name: "removable-skill".to_string(),
            description: "To be removed".to_string(),
            path: "/tmp/removable.md".to_string(),
            content_hash: "hash".to_string(),
            tags: vec![],
            ecosystem_compatible: vec![],
            ecosystem_requires: vec![],
            version: "1.0.0".to_string(),
            last_modified: "2023-01-01T00:00:00Z".to_string(),
        };
        
        registry.upsert(skill)?;
        assert_eq!(registry.count(), 1);
        
        registry.remove("removable-skill")?;
        assert_eq!(registry.count(), 0);
        assert!(registry.get("removable-skill").is_none());
        
        Ok(())
    }
}