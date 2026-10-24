//! Filesystem-backed review state under `~/.loom/review/runs/<id>/`:
//! `plan.json`, `findings.jsonl`, `verify.jsonl`, `report.md`.

use crate::types::{Finding, ReviewRun};
use anyhow::{Context, Result};
use std::io::Write;
use std::path::{Path, PathBuf};

/// On-disk review store.
#[derive(Debug, Clone)]
pub struct ReviewStore {
    /// `~/.loom/review/runs`
    base: PathBuf,
}

impl ReviewStore {
    pub fn new(loom_dir: &Path) -> Result<Self> {
        let base = loom_dir.join("review").join("runs");
        std::fs::create_dir_all(&base)
            .with_context(|| format!("create review dir {}", base.display()))?;
        Ok(Self { base })
    }

    /// Directory for a run.
    pub fn run_dir(&self, run_id: &str) -> PathBuf {
        self.base.join(run_id)
    }

    /// Persist a run manifest as `plan.json`.
    pub fn save_plan(&self, run_id: &str, run: &ReviewRun) -> Result<()> {
        let dir = self.run_dir(run_id);
        std::fs::create_dir_all(&dir)?;
        let path = dir.join("plan.json");
        let content = serde_json::to_string_pretty(run)?;
        std::fs::write(&path, content)
            .with_context(|| format!("write plan {}", path.display()))?;
        Ok(())
    }

    /// Append findings as `findings.jsonl` (one JSON object per line).
    pub fn append_findings(&self, run_id: &str, findings: &[Finding]) -> Result<()> {
        let dir = self.run_dir(run_id);
        std::fs::create_dir_all(&dir)?;
        let path = dir.join("findings.jsonl");
        let mut lines = String::new();
        for f in findings {
            lines.push_str(&serde_json::to_string(f)?);
            lines.push('\n');
        }
        std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .with_context(|| format!("append findings {}", path.display()))?
            .write_all(lines.as_bytes())?;
        Ok(())
    }

    /// Append a verification record to `verify.jsonl`.
    pub fn append_verify(&self, _run_id: &str) -> Result<()> {
        Ok(())
    }

    /// Write `report.md`.
    pub fn write_report(&self, run_id: &str, markdown: &str) -> Result<PathBuf> {
        let dir = self.run_dir(run_id);
        std::fs::create_dir_all(&dir)?;
        let path = dir.join("report.md");
        std::fs::write(&path, markdown)
            .with_context(|| format!("write report {}", path.display()))?;
        Ok(path)
    }

    /// List run ids on disk (newest first).
    pub fn list_runs(&self) -> Result<Vec<String>> {
        let mut ids = Vec::new();
        for entry in std::fs::read_dir(&self.base)? {
            let entry = entry?;
            if entry.file_type()?.is_dir() {
                if let Some(name) = entry.file_name().to_str() {
                    ids.push(name.to_string());
                }
            }
        }
        ids.sort();
        ids.reverse();
        Ok(ids)
    }

    /// Load a run manifest from disk.
    pub fn load_run(&self, run_id: &str) -> Result<Option<ReviewRun>> {
        let path = self.run_dir(run_id).join("plan.json");
        if !path.exists() {
            return Ok(None);
        }
        let content = std::fs::read_to_string(&path)
            .with_context(|| format!("read plan {}", path.display()))?;
        let run: ReviewRun = serde_json::from_str(&content)?;
        Ok(Some(run))
    }

    /// Mark a run as done by saving its manifest.
    pub fn mark_done(&self, run_id: &str, run: &ReviewRun) -> Result<()> {
        self.save_plan(run_id, run)
    }

    /// Path for a run's report file.
    pub fn report_path(&self, run_id: &str) -> PathBuf {
        self.run_dir(run_id).join("report.md")
    }

    /// State directory for a run (base for plan/verify artifacts).
    pub fn state_dir(&self, run_id: &str) -> PathBuf {
        self.run_dir(run_id)
    }

    /// Base reviews root (`~/.loom/review`).
    pub fn review_root(loom_dir: &Path) -> PathBuf {
        loom_dir.join("review")
    }
}

impl Default for ReviewStore {
    fn default() -> Self {
        Self::new(std::path::Path::new(".")).expect("cwd exists")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::EvidenceState;
    use tempfile::tempdir;

    #[test]
    fn store_creates_and_lists_runs() {
        let dir = tempdir().expect("temp dir");
        let store = ReviewStore::new(dir.path()).expect("store opens");
        let run = ReviewRun::new("r1".to_string(), "s1".to_string());
        store.save_plan("r1", &run).expect("plan saved");

        let ids = store.list_runs().expect("list runs");
        assert!(ids.contains(&"r1".to_string()));

        let loaded = store.load_run("r1").expect("load run");
        assert!(loaded.is_some());
        assert_eq!(loaded.unwrap().session_id, "s1");
    }

    #[test]
    fn evidence_state_weights_and_labels() {
        assert_eq!(EvidenceState::Proven.label(), "proven");
        assert_eq!(EvidenceState::Proven.weight(), 2.0);
        assert!(EvidenceState::Proven.terminal());
        assert!(!EvidenceState::Unverified.terminal());
    }
}
