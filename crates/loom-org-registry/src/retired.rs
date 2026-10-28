//! Sidecar store for `retired-by-org` items.
//!
//! When the org registry no longer contains an item that exists locally, the
//! item is marked `retired-by-org` here instead of being deleted from the
//! local registry. Retired items are hidden from `OrgSync` filtered views but
//! remain on disk, recoverable if the org re-adds them.
//!
//! Stored at `{loom_dir}/org/{org_id}/retired.json` (mode 0600).

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

/// A retired-by-org item.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RetiredItem {
    pub kind: String,      // "skill" | "mcp" | "template"
    pub id: String,        // unique within kind
    pub retired_at: String, // RFC3339
}

/// Retired-item sidecar store.
pub struct RetiredStore {
    path: PathBuf,
    items: HashMap<String, RetiredItem>, // key = "kind:id"
}

impl RetiredStore {
    /// Create (or load) the store at `retired_path`.
    pub fn new(retired_path: &Path) -> Self {
        let mut items = HashMap::new();
        if retired_path.exists() {
            if let Ok(raw) = fs::read_to_string(retired_path) {
                if let Ok(file) = serde_json::from_str::<RetiredFile>(&raw) {
                    for item in file.items {
                        items.insert(format!("{}:{}", item.kind, item.id), item);
                    }
                }
            }
        }
        Self {
            path: retired_path.to_path_buf(),
            items,
        }
    }

    /// `true` when `(kind, id)` is marked retired.
    pub fn is_retired(&self, kind: &str, id: &str) -> bool {
        self.items.contains_key(&format!("{kind}:{id}"))
    }

    /// Mark `(kind, id)` as retired.
    pub fn retire(&mut self, kind: &str, id: &str) {
        self.items.insert(
            format!("{kind}:{id}"),
            RetiredItem {
                kind: kind.to_string(),
                id: id.to_string(),
                retired_at: loom_core::rfc3339_now(),
            },
        );
    }

    /// Clear retirement for `(kind, id)`.
    pub fn restore(&mut self, kind: &str, id: &str) {
        self.items.remove(&format!("{kind}:{id}"));
    }

    /// List all retired items.
    pub fn list_retired(&self) -> Vec<RetiredItem> {
        self.items.values().cloned().collect()
    }

    /// Persist to disk (0600).
    pub fn save(&self) -> anyhow::Result<()> {
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent)?;
        }
        let file = RetiredFile {
            items: self.items.values().cloned().collect(),
        };
        let json = serde_json::to_string_pretty(&file)?;
        vault_write_private(&self.path, json.as_bytes())?;
        Ok(())
    }

    /// Reload from disk.
    pub fn reload(&mut self) -> anyhow::Result<()> {
        let mut items = HashMap::new();
        if self.path.exists() {
            let raw = fs::read_to_string(&self.path)?;
            if let Ok(file) = serde_json::from_str::<RetiredFile>(&raw) {
                for item in file.items {
                    items.insert(format!("{}:{}", item.kind, item.id), item);
                }
            }
        }
        self.items = items;
        Ok(())
    }
}

/// On-disk format for retired items.
#[derive(Debug, Serialize, Deserialize)]
struct RetiredFile {
    items: Vec<RetiredItem>,
}

/// Write with owner-only permissions (0600 on unix).
pub(crate) fn vault_write_private(path: &Path, contents: &[u8]) -> anyhow::Result<()> {
    use std::fs::OpenOptions;
    use std::io::Write;

    let mut opts = OpenOptions::new();
    opts.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opts.mode(0o600);
    }
    let mut file = opts
        .open(path)
        .map_err(|e| anyhow::anyhow!("failed to open {path:?}: {e}"))?;
    file.write_all(contents)
        .map_err(|e| anyhow::anyhow!("failed to write {path:?}: {e}"))?;
    drop(file);

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))
            .map_err(|e| anyhow::anyhow!("failed to set permissions on {path:?}: {e}"))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn retire_and_restore_roundtrip() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("retired.json");
        let mut store = RetiredStore::new(&path);

        assert!(!store.is_retired("skill", "code-review"));

        store.retire("skill", "code-review");
        assert!(store.is_retired("skill", "code-review"));
        assert!(!store.is_retired("mcp", "filesystem"));

        store.restore("skill", "code-review");
        assert!(!store.is_retired("skill", "code-review"));
    }

    #[test]
    fn persist_and_reload() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("retired.json");

        {
            let mut store = RetiredStore::new(&path);
            store.retire("skill", "code-review");
            store.retire("mcp", "filesystem");
            store.save().unwrap();
        }

        let store = RetiredStore::new(&path);
        assert!(store.is_retired("skill", "code-review"));
        assert!(store.is_retired("mcp", "filesystem"));
        assert_eq!(store.list_retired().len(), 2);
    }

    #[test]
    fn file_permissions_are_0600() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("retired.json");
        let mut store = RetiredStore::new(&path);
        store.retire("skill", "code-review");
        store.save().unwrap();

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&path)
                .unwrap()
                .permissions()
                .mode()
                & 0o777;
            assert_eq!(mode, 0o600, "retired.json must be 0600");
        }
    }
}
