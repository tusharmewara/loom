use crate::scan::{scan_skill_tree, SkillScan};
use loom_core::SkillEntry;
use rusqlite::{params, Connection, Result};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, RwLock};
use tracing::debug;

/// Skill registry using SQLite with secondary indexes
pub struct SkillRegistry {
    /// Connection pool - we'll use a simple Arc<Mutex<Connection>> for now
    /// In production, this could be replaced with a proper connection pool
    db: Arc<Mutex<Connection>>,
    /// Cache for faster lookups
    cache: RwLock<HashMap<String, SkillEntry>>,
    /// The skills directory this registry mirrors.
    skills_dir: PathBuf,
    /// The result of the last scan of the skills directory.
    last_scan: RwLock<SkillScan>,
}

impl SkillRegistry {
    /// Create a new skill registry pointing to the given agents directory
    pub fn new(loom_dir: &str) -> anyhow::Result<Self> {
        let skills_dir = Path::new(loom_dir).join("skills");
        std::fs::create_dir_all(&skills_dir)?;

        let db_path = skills_dir.join("skills.db");
        let conn = loom_core::db::open(&db_path)?;
        conn.pragma_update(None, "foreign_keys", true.to_string())?;

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
            skills_dir,
            last_scan: RwLock::new(SkillScan::default()),
        };

        // Load existing skills into cache
        registry.refresh_cache()?;

        // The filesystem is the source of truth: one walk populates the rows.
        registry.sync_from_disk()?;

        Ok(registry)
    }

    /// Walk the skills directory and make the registry match what is on disk.
    ///
    /// The scan result IS the registry content. Validation annotates a skill,
    /// it never removes it: a skill with no `description:` still gets a row
    /// (with an empty description) and its defect is reported separately.
    /// Rows whose skill is gone from disk are removed, so the registry never
    /// reports a skill that is not installed. When a flat file and a directory
    /// share a name, the first one wins and the collision is reported as a
    /// defect — never resolved silently.
    ///
    /// Only changed rows are written, so a repeated sync does not churn
    /// `updated_at` on every read.
    pub fn sync_from_disk(&self) -> anyhow::Result<SkillScan> {
        let scan = scan_skill_tree(&self.skills_dir);

        let mut kept: Vec<SkillEntry> = Vec::new();
        for skill in &scan.skills {
            // First file with this name wins; the other is the duplicate the
            // scan already reported. The name column is UNIQUE, so one row is
            // the only thing the schema can hold.
            if kept.iter().any(|k| k.name == skill.name) {
                continue;
            }
            if self.needs_write(skill) {
                self.upsert(skill.clone())?;
            }
            kept.push(skill.clone());
        }

        // Drop rows whose skill is no longer on disk.
        for stale in self.stale_names(&kept) {
            self.remove(&stale)?;
        }

        *self.last_scan.write().unwrap() = scan.clone();
        debug!(
            skills = kept.len(),
            defects = scan.defects.len(),
            "Synced skill registry from disk"
        );
        Ok(scan)
    }

    /// Whether the stored row differs from the scanned skill.
    fn needs_write(&self, skill: &SkillEntry) -> bool {
        let cache = self.cache.read().unwrap();
        match cache.get(&skill.name) {
            None => true,
            Some(old) => {
                old.content_hash != skill.content_hash
                    || old.description != skill.description
                    || old.path != skill.path
                    || old.version != skill.version
                    || old.tags != skill.tags
            }
        }
    }

    /// The result of the last scan of the skills directory.
    pub fn last_scan(&self) -> SkillScan {
        self.last_scan.read().unwrap().clone()
    }

    /// Registered names that no longer exist on disk.
    fn stale_names(&self, kept: &[SkillEntry]) -> Vec<String> {
        let cache = self.cache.read().unwrap();
        cache
            .values()
            .filter(|s| !kept.iter().any(|k| k.name == s.name))
            .map(|s| s.name.clone())
            .collect()
    }

    /// Refresh the in-memory cache from database
    pub fn refresh_cache(&self) -> anyhow::Result<()> {
        let mut cache = self.cache.write().unwrap();
        cache.clear();

        let lock = loom_core::lock_ok!(self.db);
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
                ecosystem_compatible: serde_json::from_str(&row.get::<_, String>(5)?)
                    .unwrap_or_default(),
                ecosystem_requires: serde_json::from_str(&row.get::<_, String>(6)?)
                    .unwrap_or_default(),
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

        skill.last_modified = loom_core::rfc3339_now();

        let mut lock = loom_core::lock_ok!(self.db);
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
        let mut lock = loom_core::lock_ok!(self.db);
        let tx = lock.transaction()?;

        tx.execute("DELETE FROM skills WHERE name = ?", params![name])?;

        tx.commit()?;

        // Update cache
        let mut cache = self.cache.write().unwrap();
        cache.remove(name);

        Ok(())
    }

    /// Get skills modified since a given timestamp
    pub fn get_modified_since(&self, since: &str) -> Vec<SkillEntry> {
        let lock = loom_core::lock_ok!(self.db);
        let mut stmt = lock
            .prepare(
                r#"
            SELECT s.name, s.description, s.path, s.content_hash, 
                   s.tags, s.ecosystem_compatible, s.ecosystem_requires,
                   s.version, s.last_modified
            FROM skills s
            WHERE s.updated_at > ?
            ORDER BY s.updated_at DESC
        "#,
            )
            .unwrap();

        let skill_iter = stmt
            .query_map(params![since], |row| {
                Ok(SkillEntry {
                    name: row.get(0)?,
                    description: row.get(1)?,
                    path: row.get(2)?,
                    content_hash: row.get(3)?,
                    tags: serde_json::from_str(&row.get::<_, String>(4)?).unwrap_or_default(),
                    ecosystem_compatible: serde_json::from_str(&row.get::<_, String>(5)?)
                        .unwrap_or_default(),
                    ecosystem_requires: serde_json::from_str(&row.get::<_, String>(6)?)
                        .unwrap_or_default(),
                    version: row.get(7)?,
                    last_modified: row.get(8)?,
                })
            })
            .unwrap();

        skill_iter.filter_map(Result::ok).collect()
    }

    /// Get total count of skills
    pub fn count(&self) -> usize {
        let lock = loom_core::lock_ok!(self.db);
        lock.query_row("SELECT COUNT(*) FROM skills", [], |row| {
            row.get::<_, i64>(0)
        })
        .unwrap_or(0) as usize
    }
}

impl Drop for SkillRegistry {
    fn drop(&mut self) {
        // The Arc<Mutex<Connection>> will be cleaned up automatically
        // when all clones are dropped
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scan::Severity;
    use tempfile::tempdir;

    fn write(path: &Path, content: &str) {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(path, content).unwrap();
    }

    fn skill_md(name: &str, description: &str) -> String {
        format!("---\nname: {name}\ndescription: {description}\n---\n\n# {name}\n\nBody.\n")
    }

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

    #[test]
    fn walk_populates_registry_rows() -> anyhow::Result<()> {
        let dir = tempdir()?;
        let skills = dir.path().join("skills");
        write(
            &skills.join("dir-skill/SKILL.md"),
            &skill_md("dir-skill", "\"A skill in a directory\""),
        );
        write(
            &skills.join("flat-skill.md"),
            &skill_md("flat-skill", "\"A skill in a flat file\""),
        );

        let registry = SkillRegistry::new(dir.path().to_str().unwrap())?;

        assert_eq!(registry.count(), 2, "both layouts must be registered");
        let names: Vec<String> = registry.list().into_iter().map(|s| s.name).collect();
        assert!(names.contains(&"dir-skill".to_string()));
        assert!(names.contains(&"flat-skill".to_string()));
        Ok(())
    }

    #[test]
    fn sync_registers_the_description() -> anyhow::Result<()> {
        let dir = tempdir()?;
        write(
            &dir.path().join("skills/one/SKILL.md"),
            &skill_md("one", "\"Use when the user asks for it\""),
        );

        let registry = SkillRegistry::new(dir.path().to_str().unwrap())?;
        let skill = registry.get("one").expect("skill must be registered");
        assert_eq!(skill.description, "Use when the user asks for it");
        Ok(())
    }

    #[test]
    fn fatal_skill_still_gets_a_row() -> anyhow::Result<()> {
        let dir = tempdir()?;
        // No frontmatter at all: the live `gdrive` shape.
        write(
            &dir.path().join("skills/gdrive/SKILL.md"),
            "# Google Drive\n\nUpload and download files.\n",
        );
        write(
            &dir.path().join("skills/healthy/SKILL.md"),
            &skill_md("healthy", "\"Fine\""),
        );

        let registry = SkillRegistry::new(dir.path().to_str().unwrap())?;

        // Validation annotates, it never removes: the skill is in the
        // inventory, with an empty description.
        let gdrive = registry
            .get("gdrive")
            .expect("a description-less skill is still registered");
        assert_eq!(gdrive.description, "");
        assert!(registry.get("healthy").is_some());
        assert_eq!(registry.count(), 2);

        // The defect is reported, not swallowed.
        let scan = registry.last_scan();
        let defect = scan
            .defects
            .iter()
            .find(|d| d.code == "missing_description")
            .expect("missing_description must be reported");
        assert_eq!(defect.severity, Severity::Fatal);
        Ok(())
    }

    #[test]
    fn sync_reports_a_duplicate_pair() -> anyhow::Result<()> {
        let dir = tempdir()?;
        let skills = dir.path().join("skills");
        write(&skills.join("dup/SKILL.md"), &skill_md("dup", "\"One copy\""));
        write(&skills.join("dup.md"), &skill_md("dup", "\"Another copy\""));

        let registry = SkillRegistry::new(dir.path().to_str().unwrap())?;

        // One row: the name column is UNIQUE.
        assert_eq!(registry.count(), 1);
        assert!(registry.get("dup").is_some());

        // The collision is reported, not silently resolved.
        let scan = registry.last_scan();
        let defect = scan
            .defects
            .iter()
            .find(|d| d.code == "duplicate_name")
            .expect("duplicate_name must be reported");
        assert_eq!(defect.severity, Severity::Warn);
        Ok(())
    }

    #[test]
    fn version_is_synthesized_when_frontmatter_has_none() -> anyhow::Result<()> {
        let dir = tempdir()?;
        write(
            &dir.path().join("skills/one/SKILL.md"),
            &skill_md("one", "\"Use when the user asks for it\""),
        );

        let registry = SkillRegistry::new(dir.path().to_str().unwrap())?;
        let skill = registry.get("one").expect("the skill must be registered");
        assert!(!skill.version.is_empty(), "the version is synthesized");
        Ok(())
    }

    #[test]
    fn registry_hash_matches_the_skill_file() -> anyhow::Result<()> {
        let dir = tempdir()?;
        let path = dir.path().join("skills/one/SKILL.md");
        let body = skill_md("one", "\"Use when the user asks for it\"");
        write(&path, &body);

        let registry = SkillRegistry::new(dir.path().to_str().unwrap())?;
        let skill = registry.get("one").expect("the skill must be registered");
        assert_eq!(skill.path, path.to_string_lossy(), "the path is the file");
        assert_eq!(skill.content_hash.len(), 64, "SHA-256 of the file bytes");
        Ok(())
    }

    #[test]
    fn refresh_removes_rows_for_vanished_skills() -> anyhow::Result<()> {
        let dir = tempdir()?;
        let path = dir.path().join("skills/gone/SKILL.md");
        write(&path, &skill_md("gone", "\"Here now\""));

        let registry = SkillRegistry::new(dir.path().to_str().unwrap())?;
        assert_eq!(registry.count(), 1);

        std::fs::remove_dir_all(dir.path().join("skills/gone"))?;
        registry.sync_from_disk()?;

        assert_eq!(registry.count(), 0, "a removed skill must not stay registered");
        assert!(registry.get("gone").is_none());
        Ok(())
    }
}
