//! Worktree-task manifest: persisted mapping of task -> worktree/branch.
//!
//! Every fan-out run records one `WorktreeEntry` per task so an interrupted
//! run can be resumed or its stale worktrees cleaned up.

use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorktreeEntry {
    pub task_id: String,
    pub branch: String,
    pub worktree_path: String,
    pub base_ref: String,
    pub created_at: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct WorktreeManifest {
    pub entries: Vec<WorktreeEntry>,
}

impl WorktreeManifest {
    #[allow(unused_variables)]
    pub fn add(&mut self, entry: WorktreeEntry) {
        unimplemented!("WorktreeManifest::add")
    }

    #[allow(unused_variables)]
    pub fn entry_for(&self, task_id: &str) -> Option<&WorktreeEntry> {
        unimplemented!("WorktreeManifest::entry_for")
    }

    #[allow(unused_variables)]
    pub fn save(&self, path: &Path) -> Result<()> {
        unimplemented!("WorktreeManifest::save")
    }

    #[allow(unused_variables)]
    pub fn load(path: &Path) -> Result<WorktreeManifest> {
        unimplemented!("WorktreeManifest::load")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(task_id: &str) -> WorktreeEntry {
        WorktreeEntry {
            task_id: task_id.to_string(),
            branch: format!("loom/{task_id}-dark-mode"),
            worktree_path: format!("/tmp/wt/{task_id}"),
            base_ref: "main".to_string(),
            created_at: "2026-09-13T00:00:00Z".to_string(),
        }
    }

    #[test]
    fn round_trips_through_disk() {
        let tmp = tempfile::tempdir().unwrap();
        let mut m = WorktreeManifest::default();
        m.add(entry("t1"));
        let path = tmp.path().join("run.json");
        m.save(&path).unwrap();

        let loaded = WorktreeManifest::load(&path).unwrap();
        assert_eq!(loaded.entries.len(), 1);
        assert_eq!(loaded.entry_for("t1").unwrap().branch, "loom/t1-dark-mode");
        assert_eq!(loaded.entry_for("t1").unwrap().worktree_path, "/tmp/wt/t1");
        assert!(loaded.entry_for("ghost").is_none());
    }

    #[test]
    fn missing_file_loads_empty_manifest() {
        let tmp = tempfile::tempdir().unwrap();
        let m = WorktreeManifest::load(&tmp.path().join("nope.json")).unwrap();
        assert!(m.entries.is_empty());
    }
}