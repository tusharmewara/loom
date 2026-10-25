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

    /// Load findings from a run's `findings.jsonl` (empty if absent).
    pub fn load_findings(&self, run_id: &str) -> Result<Vec<Finding>> {
        let path = self.run_dir(run_id).join("findings.jsonl");
        if !path.exists() {
            return Ok(Vec::new());
        }
        let content = std::fs::read_to_string(&path)
            .with_context(|| format!("read findings {}", path.display()))?;
        content
            .lines()
            .filter(|l| !l.trim().is_empty())
            .map(|l| serde_json::from_str(l).with_context(|| "parse finding line"))
            .collect()
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

    /// Overwrite a run's `lessons.json`.
    pub fn save_lessons(&self, run_id: &str, lessons: &[crate::lessons::Lesson]) -> Result<()> {
        let dir = self.run_dir(run_id);
        std::fs::create_dir_all(&dir)?;
        let path = dir.join("lessons.json");
        let content = serde_json::to_string_pretty(lessons)?;
        std::fs::write(&path, content)
            .with_context(|| format!("write lessons {}", path.display()))?;
        Ok(())
    }

    /// Load a run's `lessons.json` (empty if absent).
    pub fn load_lessons(&self, run_id: &str) -> Result<Vec<crate::lessons::Lesson>> {
        let path = self.run_dir(run_id).join("lessons.json");
        if !path.exists() {
            return Ok(Vec::new());
        }
        let content = std::fs::read_to_string(&path)
            .with_context(|| format!("read lessons {}", path.display()))?;
        Ok(serde_json::from_str(&content)?)
    }

    /// Every lesson across all runs (runs listed newest first).
    pub fn load_all_lessons(&self) -> Result<Vec<crate::lessons::Lesson>> {
        let mut all = Vec::new();
        for run_id in self.list_runs()? {
            all.extend(self.load_lessons(&run_id)?);
        }
        Ok(all)
    }

    /// Set a lesson's outcome (and reason) wherever it lives, matched by
    /// its cross-run-unique id. Errors when no run holds it.
    pub fn set_lesson_outcome(
        &self,
        lesson_id: &str,
        outcome: crate::lessons::LessonOutcome,
        reason: Option<String>,
    ) -> Result<()> {
        for run_id in self.list_runs()? {
            let mut lessons = self.load_lessons(&run_id)?;
            let Some(pos) = lessons.iter().position(|l| l.id == lesson_id) else {
                continue;
            };
            lessons[pos].outcome = outcome;
            lessons[pos].reason = reason;
            self.save_lessons(&run_id, &lessons)?;
            return Ok(());
        }
        anyhow::bail!("lesson {lesson_id} not found")
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
    fn findings_roundtrip_through_jsonl() {
        let dir = tempdir().expect("temp dir");
        let store = ReviewStore::new(dir.path()).expect("store opens");
        let f = Finding::new(
            "a1".into(), "t".into(), "r".into(), Some("diff".into()),
            vec!["a.rs".into()], EvidenceState::Proven, "bugs_security".into(),
        );
        store.append_findings("r1", &[f]).expect("append");

        let loaded = store.load_findings("r1").expect("load findings");
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].id, "a1");
        assert_eq!(loaded[0].state, EvidenceState::Proven);

        // Missing run dir → empty, not an error.
        assert!(store.load_findings("nope").expect("load missing").is_empty());
    }

    #[test]
    fn evidence_state_weights_and_labels() {
        assert_eq!(EvidenceState::Proven.label(), "proven");
        assert_eq!(EvidenceState::Proven.weight(), 2.0);
        assert!(EvidenceState::Proven.terminal());
        assert!(!EvidenceState::Unverified.terminal());
    }

    #[test]
    fn lessons_persist_and_set_outcome() {
        let dir = tempdir().expect("temp dir");
        let store = ReviewStore::new(dir.path()).expect("store opens");
        let f = Finding::new(
            "f1".into(), "t".into(), "r".into(), Some("diff".into()),
            vec![], EvidenceState::Proven, "seat".into(),
        );
        let lessons = crate::lessons::Lesson::derive("r1", &[f]);
        store.save_lessons("r1", &lessons).expect("save lessons");

        let loaded = store.load_lessons("r1").expect("load lessons");
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].outcome, crate::lessons::LessonOutcome::Pending);

        store
            .set_lesson_outcome(
                "r1:f1",
                crate::lessons::LessonOutcome::Dismissed,
                Some("wrong file".into()),
            )
            .expect("set outcome");
        let all = store.load_all_lessons().expect("load all");
        assert_eq!(all[0].outcome, crate::lessons::LessonOutcome::Dismissed);
        assert_eq!(all[0].reason.as_deref(), Some("wrong file"));

        assert!(store.set_lesson_outcome("nope:x", crate::lessons::LessonOutcome::Confirmed, None).is_err());
    }
}
