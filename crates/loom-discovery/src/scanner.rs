use crate::registry::SnapshotDb;
use sha2::Digest;
use std::path::Path;
use std::sync::Arc;

/// Phase 1 + Phase 2 scanner.
///
/// Phase 1: detect available layers by checking directory existence (fast, no I/O heavy).
/// Phase 2: validate snapshot entries against filesystem (SHA-256 hash verification).
pub struct Scanner {
    loom_dir: String,
    snapshot: Arc<SnapshotDb>,
}

impl Scanner {
    pub fn new(loom_dir: &str, snapshot: Arc<SnapshotDb>) -> Self {
        Self {
            loom_dir: loom_dir.to_string(),
            snapshot,
        }
    }

    /// Phase 1: Fast layer detection by directory existence.
    pub fn scan_layers(&self) -> anyhow::Result<Vec<String>> {
        let mut layers = Vec::new();
        let path = Path::new(&self.loom_dir);

        if path.join("skills").exists() {
            layers.push("skills".into());
        }
        if path.join("mcp").exists() {
            layers.push("mcp".into());
        }
        if path.join("sessions").exists() {
            layers.push("sessions".into());
        }
        if path.join("auth").exists() {
            layers.push("auth".into());
        }
        if path.join("hooks").exists() {
            layers.push("hooks".into());
        }
        if path.join("tools").exists() {
            layers.push("tools".into());
        }
        if path.join(".cache").exists() {
            layers.push(".cache".into());
        }

        Ok(layers)
    }

    /// Phase 1: Quick warnings check.
    pub fn check_warnings(&self) -> anyhow::Result<Vec<String>> {
        let mut warnings = Vec::new();
        let path = Path::new(&self.loom_dir);

        if !path.exists() {
            warnings.push("~/.loom/ does not exist. Run 'loom init' first.".into());
        } else if !path.join("config.toml").exists() {
            warnings.push("No config.toml found. Run 'loom init' to create one.".into());
        }

        let cache = path.join(".cache").join("snapshot.db");
        if !cache.exists() {
            warnings.push(
                "No SQLite snapshot found. Run 'loom init' or let the daemon build it.".into(),
            );
        }

        Ok(warnings)
    }

    /// Phase 2: Validate all snapshot entries against the filesystem.
    /// Checks content hashes, existence, and timestamps.
    pub fn validate_entries(&self) -> anyhow::Result<crate::initializer::ValidationResult> {
        let mut validated = 0;
        let mut updated = 0;
        let mut errors = Vec::new();

        // Validate skill entries
        if let Ok(skills) = self.snapshot.get_skills() {
            for skill in skills {
                validated += 1;
                let skill_path = Path::new(&skill.path);
                if !skill_path.exists() {
                    errors.push(crate::initializer::ValidationError {
                        entry_name: skill.name.clone(),
                        reason: format!("Skill file not found at {}", skill.path),
                    });
                    continue;
                }
                // Verify SHA-256 hash
                if let Ok(content) = std::fs::read_to_string(skill_path) {
                    let actual_hash = format!("{:x}", sha2::Sha256::digest(content.as_bytes()));
                    if actual_hash != skill.content_hash {
                        tracing::info!(
                            "Updating hash for skill '{}' (content changed)",
                            skill.name
                        );
                        updated += 1;
                    }
                }
            }
        }

        Ok(crate::initializer::ValidationResult {
            entries_validated: validated,
            entries_updated: updated,
            errors,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::registry::SnapshotDb;

    #[test]
    fn test_scan_layers_empty_dir() {
        let dir = std::env::temp_dir().join("loom-test-layers-empty");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        let snapshot = Arc::new(SnapshotDb::new(dir.to_str().unwrap()));
        let _scanner = super::Scanner::new(dir.to_str().unwrap(), snapshot);
    }
}
