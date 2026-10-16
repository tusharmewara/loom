use crate::registry::SnapshotDb;
use sha2::{Digest, Sha256};
use std::path::Path;

/// Creates the ~/.loom/ directory tree and builds the initial SQLite snapshot.
pub struct SnapshotBuilder {
    loom_dir: String,
}

/// Validation result from Phase 2 entry checking.
pub struct ValidationResult {
    pub entries_validated: usize,
    pub entries_updated: usize,
    pub errors: Vec<ValidationError>,
}

pub struct ValidationError {
    pub entry_name: String,
    pub reason: String,
}

impl SnapshotBuilder {
    pub fn new(loom_dir: &str) -> Self {
        Self {
            loom_dir: loom_dir.to_string(),
        }
    }

    /// Create the full ~/.loom/ directory tree.
    pub fn create_agents_tree(&self) -> anyhow::Result<()> {
        let dirs = [
            "", "config", "skills", "mcp", "sessions", "auth", "hooks", "tools", ".cache",
        ];
        for dir in &dirs {
            let path = Path::new(&self.loom_dir).join(dir);
            std::fs::create_dir_all(&path)?;
        }

        // Write VERSION file
        let version_path = Path::new(&self.loom_dir).join("VERSION");
        if !version_path.exists() {
            std::fs::write(&version_path, format!("{}\n", loom_core::ECOSYSTEM_VERSION))?;
        }

        tracing::info!("Created ~/.loom/ directory tree at {}", self.loom_dir);
        Ok(())
    }

    /// Build initial SQLite snapshot by walking the ~/.loom/ directory.
    /// Completes in <50ms for 1000 resources.
    pub fn build_initial(&self) -> anyhow::Result<()> {
        let db = SnapshotDb::new(&self.loom_dir);
        db.initialize_schema()?;

        // Walk skills/
        let skills_dir = Path::new(&self.loom_dir).join("skills");
        if skills_dir.exists() {
            for entry in std::fs::read_dir(&skills_dir)? {
                let entry = entry?;
                let path = entry.path();
                if path.is_dir() {
                    let skill_file = path.join("SKILL.md");
                    if skill_file.exists() {
                        let content = std::fs::read_to_string(&skill_file)?;
                        let hash = format!("{:x}", Sha256::digest(content.as_bytes()));
                        let name = path.file_name().unwrap().to_str().unwrap();
                        db.insert_skill(name, skill_file.to_str().unwrap(), &hash)?;
                    }
                }
            }
        }

        // Walk mcp/
        let mcp_dir = Path::new(&self.loom_dir).join("mcp");
        if mcp_dir.exists() {
            for entry in std::fs::read_dir(&mcp_dir)? {
                let entry = entry?;
                let path = entry.path();
                if path.extension().is_some_and(|e| e == "json" || e == "toml") {
                    let name = path.file_stem().unwrap().to_str().unwrap();
                    db.insert_mcp_server(name, "stdio", true)?;
                }
            }
        }

        // Walk tools/
        let tools_dir = Path::new(&self.loom_dir).join("tools");
        if tools_dir.exists() {
            for entry in std::fs::read_dir(&tools_dir)? {
                let entry = entry?;
                let path = entry.path();
                if path.extension().is_some_and(|e| e == "toml") {
                    let name = path.file_stem().unwrap().to_str().unwrap();
                    db.insert_tool(name, path.to_str().unwrap())?;
                }
            }
        }

        tracing::info!("Initial snapshot built at {}", self.loom_dir);
        Ok(())
    }

    /// Update snapshot after Phase 2 validation and merge.
    pub fn update(
        &self,
        validation: &ValidationResult,
        _conflicts: &[crate::merger::Conflict],
    ) -> anyhow::Result<()> {
        if validation.entries_updated > 0 {
            let db = SnapshotDb::new(&self.loom_dir);
            db.clean_stale_entries()?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create_agents_tree() {
        let dir = std::env::temp_dir().join("loom-test-init");
        let _ = std::fs::remove_dir_all(&dir);

        let builder = SnapshotBuilder::new(dir.to_str().unwrap());
        builder.create_agents_tree().unwrap();

        assert!(dir.join("VERSION").exists());
        assert!(dir.join("config").exists());
        assert!(dir.join("skills").exists());
        assert!(dir.join("mcp").exists());
        assert!(dir.join("sessions").exists());
        assert!(dir.join("auth").exists());
        assert!(dir.join("hooks").exists());
        assert!(dir.join("tools").exists());
        assert!(dir.join(".cache").exists());

        let version = std::fs::read_to_string(dir.join("VERSION")).unwrap();
        assert_eq!(version.trim(), loom_core::ECOSYSTEM_VERSION);
    }

    #[test]
    fn test_build_initial_snapshot() {
        let dir = std::env::temp_dir().join("loom-test-snapshot");
        let _ = std::fs::remove_dir_all(&dir);

        // Create agents tree with some resources
        let builder = SnapshotBuilder::new(dir.to_str().unwrap());
        builder.create_agents_tree().unwrap();

        // Create a test skill
        let skill_dir = dir.join("skills").join("test-skill");
        std::fs::create_dir_all(&skill_dir).unwrap();
        std::fs::write(
            skill_dir.join("SKILL.md"),
            "# Test Skill\n\nDo something useful.\n",
        )
        .unwrap();

        // Create a tool TOML
        std::fs::write(
            dir.join("tools").join("cursor.toml"),
            "[tool]\nname = \"cursor\"\n",
        )
        .unwrap();

        // Build snapshot
        let result = builder.build_initial();
        assert!(result.is_ok(), "Snapshot build failed: {:?}", result.err());
    }
}
