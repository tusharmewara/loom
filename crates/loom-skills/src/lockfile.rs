//! Lock file for skill reproducibility.
//!
//! The lock file (`~/.loom/skills/skills.lock`) captures the exact state of
//! installed skills so that teams and CI environments get deterministic skill sets.
//!
//! ## Usage
//!
//! ```bash
//! loom skills install ./my-skill.md    # updates lock file
//! loom lock generate                    # regenerate from current state
//! loom skills list --locked            # diff-check against lock file
//! ```

use loom_core::SkillEntry;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::path::Path;

/// The lock file schema version.
const LOCKFILE_VERSION: u32 = 1;

/// Top-level lock file structure.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillsLock {
    /// Schema version for forward compatibility.
    pub version: u32,
    /// Per-skill lock entries, keyed by skill name.
    pub skills: HashMap<String, LockedSkill>,
}

/// A single locked skill entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LockedSkill {
    /// Installed version at lock time.
    pub version: String,
    /// SHA-256 content hash (hex-encoded) for integrity verification.
    pub content_hash: String,
    /// Optional tags snapshot for informational purposes.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<String>,
}

impl SkillsLock {
    /// Create an empty lock file.
    pub fn new() -> Self {
        Self {
            version: LOCKFILE_VERSION,
            skills: HashMap::new(),
        }
    }

    /// Load the lock file from a path. Returns `None` if the file doesn't exist.
    pub fn load(path: &Path) -> anyhow::Result<Option<Self>> {
        if !path.exists() {
            return Ok(None);
        }
        let content = std::fs::read_to_string(path)?;
        let lock: SkillsLock = toml::from_str(&content)?;
        Ok(Some(lock))
    }

    /// Save the lock file to a path.
    pub fn save(&self, path: &Path) -> anyhow::Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let content = toml::to_string_pretty(self)?;
        std::fs::write(path, content)?;
        Ok(())
    }

    /// Lock an entry for a skill (add or update).
    pub fn lock_skill(&mut self, skill: &SkillEntry) {
        self.skills.insert(
            skill.name.clone(),
            LockedSkill {
                version: skill.version.clone(),
                content_hash: if skill.content_hash.is_empty() {
                    compute_file_hash(&skill.path)
                } else {
                    skill.content_hash.clone()
                },
                tags: skill.tags.clone(),
            },
        );
    }

    /// Remove a skill from the lock.
    pub fn unlock_skill(&mut self, name: &str) {
        self.skills.remove(name);
    }

    /// Check whether every locked skill still matches the current registry.
    /// Returns a list of discrepancies (name → description of what changed).
    pub fn verify_against(&self, registry_skills: &[SkillEntry]) -> Vec<LockViolation> {
        let mut violations = Vec::new();

        let current: HashMap<&str, &SkillEntry> = registry_skills
            .iter()
            .map(|s| (s.name.as_str(), s))
            .collect();

        // Check that every locked skill is still present and unchanged
        for (name, locked) in &self.skills {
            match current.get(name.as_str()) {
                None => {
                    violations.push(LockViolation {
                        name: name.clone(),
                        kind: LockViolationKind::Missing,
                        expected: locked.version.clone(),
                        actual: String::new(),
                    });
                }
                Some(entry) => {
                    if entry.version != locked.version {
                        violations.push(LockViolation {
                            name: name.clone(),
                            kind: LockViolationKind::VersionMismatch,
                            expected: locked.version.clone(),
                            actual: entry.version.clone(),
                        });
                    }
                    let actual_hash = if entry.content_hash.is_empty() {
                        compute_file_hash(&entry.path)
                    } else {
                        entry.content_hash.clone()
                    };
                    if !locked.content_hash.is_empty() && actual_hash != locked.content_hash {
                        violations.push(LockViolation {
                            name: name.clone(),
                            kind: LockViolationKind::ContentChanged,
                            expected: locked.content_hash.clone(),
                            actual: actual_hash,
                        });
                    }
                }
            }
        }

        // Check for unlocked skills that are present in the registry
        let locked_names: std::collections::HashSet<&str> =
            self.skills.keys().map(|s| s.as_str()).collect();
        for entry in registry_skills {
            if !locked_names.contains(entry.name.as_str()) {
                violations.push(LockViolation {
                    name: entry.name.clone(),
                    kind: LockViolationKind::Unlocked,
                    expected: String::new(),
                    actual: entry.version.clone(),
                });
            }
        }

        violations
    }

    /// Check whether the lock file is "clean" (no violations).
    pub fn is_clean(&self, registry_skills: &[SkillEntry]) -> bool {
        self.verify_against(registry_skills).is_empty()
    }
}

impl Default for SkillsLock {
    fn default() -> Self {
        Self::new()
    }
}

/// Describes why a lock check failed for a particular skill.
#[derive(Debug, Clone)]
pub struct LockViolation {
    pub name: String,
    pub kind: LockViolationKind,
    pub expected: String,
    pub actual: String,
}

/// The type of lock violation.
#[derive(Debug, Clone)]
pub enum LockViolationKind {
    /// Skill was in the lock file but is no longer in the registry.
    Missing,
    /// Skill version changed since locking.
    VersionMismatch,
    /// Skill content hash changed since locking (contents were modified).
    ContentChanged,
    /// Skill is present in the registry but not in the lock file.
    Unlocked,
}

impl std::fmt::Display for LockViolationKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LockViolationKind::Missing => write!(f, "missing"),
            LockViolationKind::VersionMismatch => write!(f, "version mismatch"),
            LockViolationKind::ContentChanged => write!(f, "content changed"),
            LockViolationKind::Unlocked => write!(f, "unlocked"),
        }
    }
}

/// Compute the SHA-256 hex hash of a file's contents.
fn compute_file_hash(path: &str) -> String {
    match std::fs::read_to_string(path) {
        Ok(content) => {
            let mut hasher = Sha256::new();
            hasher.update(content.as_bytes());
            format!("{:x}", hasher.finalize())
        }
        Err(_) => String::new(),
    }
}

/// Default lock file path within a loom directory.
pub fn default_lock_path(loom_dir: &str) -> std::path::PathBuf {
    Path::new(loom_dir).join("skills").join("skills.lock")
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn make_skill(name: &str, version: &str, hash: &str) -> SkillEntry {
        SkillEntry {
            name: name.to_string(),
            description: format!("Skill {}", name),
            path: format!("/tmp/{}.md", name),
            content_hash: hash.to_string(),
            tags: vec![],
            ecosystem_compatible: vec![],
            ecosystem_requires: vec![],
            version: version.to_string(),
            last_modified: "2025-01-01T00:00:00Z".to_string(),
        }
    }

    #[test]
    fn test_empty_lock_is_clean() {
        let lock = SkillsLock::new();
        assert!(lock.is_clean(&[]));
    }

    #[test]
    fn test_lock_and_verify_match() {
        let mut lock = SkillsLock::new();
        let skill = make_skill("test-skill", "1.0.0", "abc123");
        lock.lock_skill(&skill);
        assert!(lock.is_clean(&[skill]));
    }

    #[test]
    fn test_version_mismatch_detected() {
        let mut lock = SkillsLock::new();
        let skill_v1 = make_skill("test-skill", "1.0.0", "abc123");
        lock.lock_skill(&skill_v1);

        let skill_v2 = make_skill("test-skill", "2.0.0", "abc123");
        let violations = lock.verify_against(&[skill_v2]);
        assert_eq!(violations.len(), 1);
        assert!(matches!(violations[0].kind, LockViolationKind::VersionMismatch));
    }

    #[test]
    fn test_content_change_detected() {
        let mut lock = SkillsLock::new();
        let skill_orig = make_skill("test-skill", "1.0.0", "abc123");
        lock.lock_skill(&skill_orig);

        let skill_modified = make_skill("test-skill", "1.0.0", "def456");
        let violations = lock.verify_against(&[skill_modified]);
        assert_eq!(violations.len(), 1);
        assert!(matches!(violations[0].kind, LockViolationKind::ContentChanged));
    }

    #[test]
    fn test_missing_skill_detected() {
        let mut lock = SkillsLock::new();
        let skill = make_skill("test-skill", "1.0.0", "abc123");
        lock.lock_skill(&skill);

        let violations = lock.verify_against(&[]);
        assert_eq!(violations.len(), 1);
        assert!(matches!(violations[0].kind, LockViolationKind::Missing));
    }

    #[test]
    fn test_unlocked_skill_detected() {
        let lock = SkillsLock::new();
        let skill = make_skill("unlocked-skill", "1.0.0", "abc123");
        let violations = lock.verify_against(&[skill]);
        assert_eq!(violations.len(), 1);
        assert!(matches!(violations[0].kind, LockViolationKind::Unlocked));
    }

    #[test]
    fn test_roundtrip_serialization() -> anyhow::Result<()> {
        let dir = tempdir()?;
        let path = dir.path().join("skills.lock");

        let mut lock = SkillsLock::new();
        let skill = make_skill("test", "1.0.0", "hash123");
        lock.lock_skill(&skill);
        lock.save(&path)?;

        let loaded = SkillsLock::load(&path)?.unwrap();
        assert_eq!(loaded.version, LOCKFILE_VERSION);
        assert_eq!(loaded.skills.len(), 1);
        assert_eq!(loaded.skills["test"].version, "1.0.0");
        assert_eq!(loaded.skills["test"].content_hash, "hash123");

        Ok(())
    }

    #[test]
    fn test_load_nonexistent_returns_none() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("nonexistent.lock");
        let loaded = SkillsLock::load(&path).unwrap();
        assert!(loaded.is_none());
    }

    #[test]
    fn test_display_lock_violation_kind() {
        assert_eq!(LockViolationKind::Missing.to_string(), "missing");
        assert_eq!(LockViolationKind::VersionMismatch.to_string(), "version mismatch");
        assert_eq!(LockViolationKind::ContentChanged.to_string(), "content changed");
        assert_eq!(LockViolationKind::Unlocked.to_string(), "unlocked");
    }
}
