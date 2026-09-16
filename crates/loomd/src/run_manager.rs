//! Daemon-side run lifecycle tracking for ACP dispatches.
//!
//! A "run" records that `acp.dispatch` launched an agent for a task.
//! The daemon spawns agent processes and returns immediately, so a run
//! starts in `Dispatched` state; whoever observes process completion
//! records the outcome later via [`RunManager::complete_run`].
//!
//! `RunManager::new()` is pure in-memory. `RunManager::with_dir()`
//! additionally persists each run as `{dir}/{run_id}.run.json` so runs
//! survive daemon restarts; persistence is best-effort and never fails
//! the tracking calls.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

/// Terminal runs older than this are pruned on load (retention).
const RETENTION_SECS: u64 = 30 * 24 * 3600;

/// Lifecycle state of a tracked ACP dispatch run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RunStatus {
    /// The agent process was spawned; no outcome recorded yet.
    Dispatched,
    /// The agent process exited with status code 0.
    Succeeded,
    /// The agent process exited with a non-zero status code.
    Failed,
}

/// A single tracked ACP dispatch run.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunState {
    pub run_id: String,
    pub task_id: String,
    pub agent_id: String,
    pub status: RunStatus,
    pub exit_code: Option<i32>,
    pub started_at_secs: u64,
    pub ended_at_secs: Option<u64>,
    /// Loom session captured from this run, once session capture runs.
    pub session_id: Option<String>,
    /// Human-readable annotation (e.g. why a stale run was failed).
    #[serde(default)]
    pub note: Option<String>,
}

/// In-memory registry of ACP dispatch runs.
pub struct RunManager {
    runs: HashMap<String, RunState>,
    /// Directory for `{run_id}.run.json` persistence. `None` disables
    /// persistence (pure in-memory mode, used by unit tests).
    dir: Option<PathBuf>,
}

impl RunManager {
    pub fn new() -> Self {
        Self {
            runs: HashMap::new(),
            dir: None,
        }
    }

    /// Registry backed by `dir`: existing runs load on construction
    /// (stale `Dispatched` runs are marked failed, over-retention
    /// terminal runs are pruned) and every mutation persists.
    pub fn with_dir(dir: PathBuf) -> Self {
        let mut manager = Self {
            runs: HashMap::new(),
            dir: Some(dir),
        };
        manager.load_from_disk();
        manager
    }

    /// Record a new dispatch with a server-generated run id.
    /// Returns the run id.
    pub fn start_run(&mut self, task_id: &str, agent_id: &str) -> String {
        let run_id = uuid::Uuid::new_v4().to_string();
        self.insert_run(&run_id, task_id, agent_id);
        self.persist(&run_id);
        run_id
    }

    /// Record a dispatch under a caller-supplied run id (upsert).
    /// Returns the run id.
    pub fn record_external_run(
        &mut self,
        run_id: &str,
        task_id: &str,
        agent_id: &str,
    ) -> String {
        self.insert_run(run_id, task_id, agent_id);
        self.persist(run_id);
        run_id.to_string()
    }

    /// Look up a run by id.
    pub fn get_run(&self, run_id: &str) -> Option<RunState> {
        self.runs.get(run_id).cloned()
    }

    /// Record the outcome of a run. Unknown ids are ignored.
    pub fn complete_run(
        &mut self,
        run_id: &str,
        exit_code: i32,
        session_id: Option<String>,
    ) {
        if let Some(run) = self.runs.get_mut(run_id) {
            run.status = if exit_code == 0 {
                RunStatus::Succeeded
            } else {
                RunStatus::Failed
            };
            run.exit_code = Some(exit_code);
            run.session_id = session_id;
            run.ended_at_secs = Some(now_secs());
        }
        self.persist(run_id);
    }

    fn insert_run(&mut self, run_id: &str, task_id: &str, agent_id: &str) {
        self.runs.insert(
            run_id.to_string(),
            RunState {
                run_id: run_id.to_string(),
                task_id: task_id.to_string(),
                agent_id: agent_id.to_string(),
                status: RunStatus::Dispatched,
                exit_code: None,
                started_at_secs: now_secs(),
                ended_at_secs: None,
                session_id: None,
                note: None,
            },
        );
    }

    fn run_file(&self, run_id: &str) -> Option<PathBuf> {
        self.dir
            .as_ref()
            .map(|d| d.join(format!("{run_id}.run.json")))
    }

    /// Persist one run to disk. Best-effort: persistence failures are
    /// logged and never fail tracking. No-op when dirless.
    fn persist(&self, run_id: &str) {
        let (Some(path), Some(run)) = (self.run_file(run_id), self.runs.get(run_id))
        else {
            return;
        };
        if let Some(parent) = path.parent() {
            if std::fs::create_dir_all(parent).is_err() {
                tracing::warn!("run registry: cannot create dir for '{run_id}'");
                return;
            }
        }
        match serde_json::to_string_pretty(run) {
            Ok(text) => {
                if std::fs::write(&path, text).is_err() {
                    tracing::warn!("run registry: cannot persist run '{run_id}'");
                }
            }
            Err(e) => tracing::warn!("run registry: cannot serialize run '{run_id}': {e}"),
        }
    }

    /// Load persisted runs: skip unreadable/corrupt files, prune
    /// over-retention terminal runs, and mark still-`Dispatched` runs
    /// as failed (their waiter thread died with the old daemon).
    fn load_from_disk(&mut self) {
        let Some(dir) = self.dir.clone() else {
            return;
        };
        let Ok(entries) = std::fs::read_dir(&dir) else {
            return;
        };
        let now = now_secs();
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("json") {
                continue;
            }
            if !path
                .file_stem()
                .and_then(|s| s.to_str())
                .map(|s| s.ends_with(".run"))
                .unwrap_or(false)
            {
                continue;
            }
            let Ok(raw) = std::fs::read_to_string(&path) else {
                tracing::warn!("run registry: cannot read '{}', skipping", path.display());
                continue;
            };
            let mut run: RunState = match serde_json::from_str(&raw) {
                Ok(run) => run,
                Err(e) => {
                    tracing::warn!(
                        "run registry: cannot parse '{}' ({e}), skipping",
                        path.display()
                    );
                    continue;
                }
            };
            let age = now.saturating_sub(run.started_at_secs);
            let terminal =
                matches!(run.status, RunStatus::Succeeded | RunStatus::Failed);
            if terminal && age > RETENTION_SECS {
                let _ = std::fs::remove_file(&path);
                continue;
            }
            if matches!(run.status, RunStatus::Dispatched) {
                run.status = RunStatus::Failed;
                run.ended_at_secs = Some(now);
                run.note = Some(
                    "daemon restarted before completion was recorded".to_string(),
                );
            }
            let run_id = run.run_id.clone();
            self.runs.insert(run_id.clone(), run);
            self.persist(&run_id);
        }
    }
}

impl Default for RunManager {
    fn default() -> Self {
        Self::new()
    }
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::{RunManager, RunStatus};

    #[test]
    fn start_run_creates_a_retrievable_dispatched_run() {
        let mut manager = RunManager::new();
        let run_id = manager.start_run("task-1", "opencode");

        let run = manager
            .get_run(&run_id)
            .expect("run must exist after start_run");
        assert_eq!(run.run_id, run_id);
        assert_eq!(run.task_id, "task-1");
        assert_eq!(run.agent_id, "opencode");
        assert_eq!(run.status, RunStatus::Dispatched);
        assert_eq!(run.exit_code, None);
        assert!(run.started_at_secs > 0);
        assert_eq!(run.ended_at_secs, None);
        assert_eq!(run.session_id, None);
    }

    #[test]
    fn start_run_generates_unique_ids() {
        let mut manager = RunManager::new();
        let first = manager.start_run("task-1", "opencode");
        let second = manager.start_run("task-1", "opencode");
        assert_ne!(first, second);
    }

    #[test]
    fn get_run_returns_none_for_unknown_id() {
        let manager = RunManager::new();
        assert!(manager.get_run("does-not-exist").is_none());
    }

    #[test]
    fn complete_run_with_zero_exit_marks_succeeded() {
        let mut manager = RunManager::new();
        let run_id = manager.start_run("task-1", "opencode");

        manager.complete_run(&run_id, 0, Some("session-1".to_string()));

        let run = manager.get_run(&run_id).expect("run must exist");
        assert_eq!(run.status, RunStatus::Succeeded);
        assert_eq!(run.exit_code, Some(0));
        assert_eq!(run.session_id.as_deref(), Some("session-1"));
        assert!(run.ended_at_secs.is_some());
    }

    #[test]
    fn complete_run_with_nonzero_exit_marks_failed() {
        let mut manager = RunManager::new();
        let run_id = manager.start_run("task-1", "opencode");

        manager.complete_run(&run_id, 3, None);

        let run = manager.get_run(&run_id).expect("run must exist");
        assert_eq!(run.status, RunStatus::Failed);
        assert_eq!(run.exit_code, Some(3));
        assert_eq!(run.session_id, None);
        assert!(run.ended_at_secs.is_some());
    }

    #[test]
    fn complete_run_on_unknown_id_is_a_no_op() {
        let mut manager = RunManager::new();
        manager.complete_run("does-not-exist", 0, None);
        assert!(manager.get_run("does-not-exist").is_none());
    }

    #[test]
    fn record_external_run_uses_the_caller_supplied_id() {
        let mut manager = RunManager::new();
        let run_id = manager.record_external_run("client-run-9", "task-7", "goose");
        assert_eq!(run_id, "client-run-9");

        let run = manager.get_run("client-run-9").expect("run must exist");
        assert_eq!(run.task_id, "task-7");
        assert_eq!(run.agent_id, "goose");
        assert_eq!(run.status, RunStatus::Dispatched);
    }

    #[test]
    fn run_state_serializes_with_expected_fields() {
        let mut manager = RunManager::new();
        let run_id = manager.start_run("task-1", "opencode");
        let run = manager.get_run(&run_id).expect("run must exist");

        let value = serde_json::to_value(&run).expect("RunState must serialize");
        assert_eq!(value["run_id"], serde_json::Value::String(run_id));
        assert_eq!(value["task_id"], serde_json::Value::String("task-1".to_string()));
        assert_eq!(
            value["agent_id"],
            serde_json::Value::String("opencode".to_string())
        );
        assert_eq!(
            value["status"],
            serde_json::Value::String("dispatched".to_string())
        );
    }

    #[test]
    fn round_trip_start_and_complete_through_disk() {
        let dir = tempfile::tempdir().unwrap();
        let mut manager = RunManager::with_dir(dir.path().to_path_buf());
        let run_id = manager.start_run("t1", "echo");
        manager.complete_run(&run_id, 0, Some("s1".to_string()));
        drop(manager);

        let reloaded = RunManager::with_dir(dir.path().to_path_buf());
        let run = reloaded.get_run(&run_id).expect("run must survive restart");
        assert_eq!(run.status, RunStatus::Succeeded);
        assert_eq!(run.exit_code, Some(0));
        assert_eq!(run.session_id.as_deref(), Some("s1"));
    }

    #[test]
    fn stale_dispatched_runs_are_marked_failed_on_load() {
        let dir = tempfile::tempdir().unwrap();
        let mut manager = RunManager::with_dir(dir.path().to_path_buf());
        let run_id = manager.start_run("t1", "echo");
        drop(manager);

        let reloaded = RunManager::with_dir(dir.path().to_path_buf());
        let run = reloaded.get_run(&run_id).expect("run must exist");
        assert_eq!(run.status, RunStatus::Failed);
        assert_eq!(run.exit_code, None);
        assert!(
            run.note.unwrap_or_default().contains("restart"),
            "stale run must explain itself"
        );
    }

    #[test]
    fn corrupt_files_are_skipped_and_old_terminal_runs_pruned() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("garbage.run.json"), "{not json").unwrap();
        let old_secs = super::now_secs().saturating_sub(31 * 24 * 3600);
        let old_run = serde_json::json!({
            "run_id": "old-run",
            "task_id": "t",
            "agent_id": "a",
            "status": "succeeded",
            "exit_code": 0,
            "started_at_secs": old_secs,
            "ended_at_secs": old_secs,
            "session_id": null,
        });
        std::fs::write(
            dir.path().join("old-run.run.json"),
            serde_json::to_string_pretty(&old_run).unwrap(),
        )
        .unwrap();

        let reloaded = RunManager::with_dir(dir.path().to_path_buf());
        assert!(reloaded.get_run("old-run").is_none());
        // Only our two junk files exist; nothing loaded, nothing errored.
    }
}
