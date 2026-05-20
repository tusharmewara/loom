use anyhow::{Context, Result};
use loom_core::AclRule;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, RwLock};
use tracing::{debug, info, warn};

/// Per-tool ACL cache entry
#[derive(Debug, Clone, Serialize, Deserialize)]
struct AclEntry {
    /// Provider/tool identifier
    pub tool: String,
    /// Explicitly allowed requesting tools (empty = allow all)
    #[serde(default)]
    pub allow: Vec<String>,
    /// Explicitly denied requesting tools
    #[serde(default)]
    pub deny: Vec<String>,
    /// When this rule was last modified (ISO 8601)
    #[serde(default)]
    pub modified_at: String,
}

impl From<AclRule> for AclEntry {
    fn from(rule: AclRule) -> Self {
        Self {
            tool: rule.tool,
            allow: rule.allow,
            deny: rule.deny,
            modified_at: String::new(),
        }
    }
}

impl From<AclEntry> for AclRule {
    fn from(entry: AclEntry) -> Self {
        Self {
            tool: entry.tool,
            allow: entry.allow,
            deny: entry.deny,
        }
    }
}

/// ACL file format on disk
#[derive(Debug, Serialize, Deserialize)]
struct AclFile {
    version: u32,
    rules: Vec<AclEntry>,
}

impl Default for AclFile {
    fn default() -> Self {
        Self {
            version: 1,
            rules: Vec::new(),
        }
    }
}

/// ACL Engine with in-memory cache and disk persistence.
///
/// Supports SIGHUP-triggered reload via the `request_reload` / `reload_if_requested` pattern.
/// Thread-safe via `Arc<RwLock<..>>`.
#[derive(Clone)]
pub struct AclEngine {
    inner: Arc<RwLock<AclInner>>,
    /// Signal flag for SIGHUP reload
    reload_requested: Arc<AtomicBool>,
    /// Path to ACL rules file on disk
    rules_path: Option<String>,
}

struct AclInner {
    /// Per-tool ACL rules
    rules: HashMap<String, AclEntry>,
    /// Whether ACL has been modified since last save
    dirty: bool,
}

impl AclEngine {
    /// Create a new empty ACL engine.
    pub fn new() -> Self {
        Self {
            inner: Arc::new(RwLock::new(AclInner {
                rules: HashMap::new(),
                dirty: false,
            })),
            reload_requested: Arc::new(AtomicBool::new(false)),
            rules_path: None,
        }
    }

    /// Create an ACL engine and load rules from a file path.
    pub fn load(path: &str) -> Result<Self> {
        let engine = Self {
            inner: Arc::new(RwLock::new(AclInner {
                rules: HashMap::new(),
                dirty: false,
            })),
            reload_requested: Arc::new(AtomicBool::new(false)),
            rules_path: Some(path.to_string()),
        };
        engine.reload_from_path(path)?;
        Ok(engine)
    }

    /// Add or update an ACL rule.
    pub fn add_rule(&self, rule: AclRule) {
        let entry = AclEntry::from(rule);
        let mut inner = self.inner.write().expect("acl lock poisoned");
        inner.rules.insert(entry.tool.clone(), entry);
        inner.dirty = true;
        debug!("ACL rule added/updated");
    }

    /// Remove the ACL rule for a tool.
    pub fn remove_rule(&self, tool: &str) -> bool {
        let mut inner = self.inner.write().expect("acl lock poisoned");
        let removed = inner.rules.remove(tool).is_some();
        if removed {
            inner.dirty = true;
            debug!(tool, "ACL rule removed");
        }
        removed
    }

    /// Check whether `requesting_tool` is allowed to access `tool`.
    ///
    /// Resolution order:
    /// 1. If no rule exists for the tool → allow (default-open)
    /// 2. If `requesting_tool` is in the deny list → deny
    /// 3. If allow list is non-empty and `requesting_tool` is not in it → deny
    /// 4. Otherwise → allow
    pub fn check_access(&self, tool: &str, requesting_tool: &str) -> bool {
        let inner = self.inner.read().expect("acl lock poisoned");
        let Some(entry) = inner.rules.get(tool) else {
            // Default: allow if no rules defined
            return true;
        };

        // Check deny list first
        if entry.deny.iter().any(|d| d == requesting_tool) {
            debug!(tool, requesting_tool, "ACL: denied by deny list");
            return false;
        }

        // If allow list is non-empty, requesting_tool must be in it
        if !entry.allow.is_empty() && !entry.allow.iter().any(|a| a == requesting_tool) {
            debug!(
                tool,
                requesting_tool, "ACL: denied by allow list (not explicitly allowed)"
            );
            return false;
        }

        true
    }

    /// Get a rule for a specific tool.
    pub fn get_rule(&self, tool: &str) -> Option<AclRule> {
        let inner = self.inner.read().expect("acl lock poisoned");
        inner.rules.get(tool).map(|e| AclRule::from(e.clone()))
    }

    /// List all ACL rules.
    pub fn list_rules(&self) -> Vec<AclRule> {
        let inner = self.inner.read().expect("acl lock poisoned");
        inner.rules.values().cloned().map(AclRule::from).collect()
    }

    /// Number of rules in the engine.
    pub fn len(&self) -> usize {
        let inner = self.inner.read().expect("acl lock poisoned");
        inner.rules.len()
    }

    /// Check if ACL engine has no rules.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    // ─── Persistence ───────────────────────────────────────────────

    /// Save rules to the configured path.
    pub fn save(&self) -> Result<()> {
        let path = self
            .rules_path
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("no rules path configured"))?;
        self.save_to_path(path)
    }

    /// Save rules to a specific file path.
    pub fn save_to_path(&self, path: &str) -> Result<()> {
        let inner = self.inner.read().expect("acl lock poisoned");
        let acl_file = AclFile {
            version: 1,
            rules: inner.rules.values().cloned().collect(),
        };
        let parent = Path::new(path).parent();
        if let Some(parent) = parent {
            fs::create_dir_all(parent)
                .with_context(|| format!("failed to create ACL dir: {:?}", parent))?;
        }
        let json = serde_json::to_string_pretty(&acl_file)?;
        fs::write(path, &json).with_context(|| format!("failed to write ACL file: {path}"))?;
        // Drop the read lock before acquiring write lock
        drop(inner);
        let mut inner = self.inner.write().expect("acl lock poisoned");
        inner.dirty = false;
        debug!(path, "ACL rules saved");
        Ok(())
    }

    /// Reload rules from the configured path.
    pub fn reload(&self) -> Result<()> {
        let path = self
            .rules_path
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("no rules path configured"))?;
        self.reload_from_path(path)
    }

    fn reload_from_path(&self, path: &str) -> Result<()> {
        let path_obj = Path::new(path);
        if !path_obj.exists() {
            warn!(path, "ACL rules file not found, starting empty");
            return Ok(());
        }
        let json = fs::read_to_string(path_obj)
            .with_context(|| format!("failed to read ACL file: {path}"))?;
        let acl_file: AclFile = serde_json::from_str(&json)
            .with_context(|| format!("failed to parse ACL file: {path}"))?;

        let mut inner = self.inner.write().expect("acl lock poisoned");
        inner.rules.clear();
        for entry in acl_file.rules {
            inner.rules.insert(entry.tool.clone(), entry);
        }
        inner.dirty = false;
        info!(path, rules = inner.rules.len(), "ACL rules reloaded");
        Ok(())
    }

    // ─── SIGHUP / Hot Reload ───────────────────────────────────────

    /// Signal that a reload should happen (called from signal handler).
    pub fn request_reload(&self) {
        self.reload_requested.store(true, Ordering::SeqCst);
        debug!("ACL reload requested");
    }

    /// Check if a reload was requested and perform it if so.
    /// Returns `true` if a reload occurred.
    pub fn reload_if_requested(&self) -> Result<bool> {
        if self.reload_requested.swap(false, Ordering::SeqCst) {
            self.reload()?;
            Ok(true)
        } else {
            Ok(false)
        }
    }
}

impl Default for AclEngine {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_allow() {
        let acl = AclEngine::new();
        assert!(acl.check_access("any-tool", "any-requester"));
    }

    #[test]
    fn test_deny_specific() {
        let acl = AclEngine::new();
        acl.add_rule(AclRule {
            tool: "protected-tool".to_string(),
            allow: vec![],
            deny: vec!["bad-actor".to_string()],
        });
        assert!(!acl.check_access("protected-tool", "bad-actor"));
        assert!(acl.check_access("protected-tool", "good-actor"));
    }

    #[test]
    fn test_allow_list() {
        let acl = AclEngine::new();
        acl.add_rule(AclRule {
            tool: "premium-api".to_string(),
            allow: vec!["premium-client".to_string(), "admin".to_string()],
            deny: vec![],
        });
        assert!(acl.check_access("premium-api", "premium-client"));
        assert!(acl.check_access("premium-api", "admin"));
        assert!(!acl.check_access("premium-api", "free-client"));
    }

    #[test]
    fn test_deny_overrides_allow() {
        let acl = AclEngine::new();
        acl.add_rule(AclRule {
            tool: "api".to_string(),
            allow: vec!["everyone".to_string()],
            deny: vec!["blocked".to_string()],
        });
        assert!(!acl.check_access("api", "blocked"));
        assert!(acl.check_access("api", "everyone"));
    }

    #[test]
    fn test_remove_rule() {
        let acl = AclEngine::new();
        acl.add_rule(AclRule {
            tool: "temp".to_string(),
            allow: vec![],
            deny: vec!["x".to_string()],
        });
        assert!(!acl.check_access("temp", "x"));
        assert!(acl.remove_rule("temp"));
        assert!(acl.check_access("temp", "x"));
    }

    #[test]
    fn test_persistence() {
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("acl.json");
        let path_str = path.to_str().unwrap().to_string();

        // Create engine, add rules, save
        let acl1 = AclEngine::new();
        acl1.add_rule(AclRule {
            tool: "tool-a".to_string(),
            allow: vec!["client-1".to_string()],
            deny: vec![],
        });
        acl1.add_rule(AclRule {
            tool: "tool-b".to_string(),
            allow: vec![],
            deny: vec!["spammer".to_string()],
        });
        acl1.save_to_path(&path_str).unwrap();

        // Load from file
        let acl2 = AclEngine::load(&path_str).unwrap();
        assert_eq!(acl2.len(), 2);
        assert!(acl2.check_access("tool-a", "client-1"));
        assert!(!acl2.check_access("tool-a", "client-2"));
        assert!(!acl2.check_access("tool-b", "spammer"));
    }

    #[test]
    fn test_reload() {
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("acl.json");
        let path_str = path.to_str().unwrap().to_string();

        // Create initial engine with one rule
        let acl1 = AclEngine::new();
        acl1.add_rule(AclRule {
            tool: "v1".to_string(),
            allow: vec!["admin".to_string()],
            deny: vec![],
        });
        acl1.save_to_path(&path_str).unwrap();

        // Load into new engine
        let acl2 = AclEngine::load(&path_str).unwrap();
        assert_eq!(acl2.len(), 1);

        // Modify file externally
        let acl3 = AclEngine::new();
        acl3.add_rule(AclRule {
            tool: "v2".to_string(),
            allow: vec![],
            deny: vec!["bad".to_string()],
        });
        acl3.save_to_path(&path_str).unwrap();

        // Reload acl2 from file
        acl2.reload().unwrap();
        assert_eq!(acl2.len(), 1);
        assert!(acl2.get_rule("v2").is_some());
    }
}
