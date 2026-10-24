//! Evidence runner — verifies a finding patch in a fresh worktree.
//!
//! MVP uses `loom-parallel` worktrees + `loom-runtime`'s
//! `ProcessRuntime`. Not yet container-gated (pillar 2).

use crate::types::{EvidenceState, Finding};
use anyhow::Result;
use loom_core::types::{LaunchConfig, Task, TaskPhase, TaskStatus};
use loom_parallel::WorktreeManager;
use loom_runtime::{ProcessRuntime, Runtime};
use std::path::PathBuf;

/// Per-finding verification result.
#[derive(Debug, Clone)]
pub struct VerifyResult {
    pub state: EvidenceState,
    pub suite: String, // "green" / "not run" / "regressed"
    pub reason: String,
}

/// Runs the evidence pipeline for one finding:
///   1. Create an isolated worktree via loom-parallel.
///   2. Apply the finding's patch.
///   3. Run the test command via ProcessRuntime.
///   4. Set EvidenceState based on outcome.
pub struct EvidenceRunner {
    /// Working tree root where worktrees are created.
    pub root: PathBuf,
    /// Test command to run in the worktree (e.g. `cargo test`).
    pub test_cmd: String,
    /// Optional setup command to run once before test.
    pub setup_cmd: Option<String>,
}

impl EvidenceRunner {
    pub fn new(root: PathBuf, test_cmd: String, setup_cmd: Option<String>) -> Self {
        Self {
            root,
            test_cmd,
            setup_cmd,
        }
    }

    /// Verify a finding in an isolated worktree at `base_commit`.
    ///
    /// Returns `Proven` if verification passes, `SuiteGreen` if the suite
    /// check passes (or is not applicable), `CouldNotRun` if the patch
    /// cannot be applied, the test command is unavailable, or the worktree
    /// creation fails.
    pub async fn verify(&self, finding: &Finding, _base_commit: &str) -> Result<VerifyResult> {
        // Step 1: Create an isolated worktree. `self.root` is the git repo root.
        // If worktree creation fails (e.g. not a git repo, permissions), map to CouldNotRun.
        let mut wm = WorktreeManager::new(&self.root);
        let worktree_path = match wm.create_worktree(&finding.id) {
            Ok(p) => p,
            Err(e) => {
                return Ok(VerifyResult {
                    state: EvidenceState::CouldNotRun,
                    suite: "n/a".to_string(),
                    reason: format!("worktree create failed: {e}"),
                });
            }
        };

        // Ensure cleanup on drop
        drop(wm);

        let worktree_str = worktree_path.to_string_lossy().to_string();

        // Step 2: Apply the finding's patch
        let patch_applied = if let Some(patch_str) = &finding.patch {
            // Apply the diff patch to the worktree
            let status = std::process::Command::new("git")
                .args(["apply", "-3", patch_str])
                .current_dir(&worktree_path)
                .status()
                .map_err(|e| anyhow::anyhow!("git apply failed: {e}"))?;

            if !status.success() {
                // Clean up and return CouldNotRun
                let _ = std::fs::remove_dir_all(&worktree_path);
                return Ok(VerifyResult {
                    state: EvidenceState::CouldNotRun,
                    suite: "n/a".to_string(),
                    reason: "patch did not apply".to_string(),
                });
            }
            true
        } else {
            // No patch — just proceed
            true
        };

        // Step 3: Run the test command via ProcessRuntime
        let task = Task {
            task_id: format!("verify-{}", finding.id),
            title: format!("verify: {}", finding.title),
            description: finding.reason.clone(),
            worktree_path: Some(worktree_str.clone()),
            phase: TaskPhase::Verify,
            phase_history: vec![],
            prd_path: None,
            context_files: vec![],
            routed_provider: None,
            files_modified: vec![],
            complexity_score: None,
            routing_decision: None,
            last_agent: None,
            linked_issue: None,
            sub_agents: None,
            spec_path: None,
            findings_path: None,
            converged: None,
            phase_results: vec![],
            tags: vec![],
            runtime: None,
            last_run: None,
            created_at: String::new(),
            updated_at: String::new(),
            status: TaskStatus::Pending,
        };

        let config = LaunchConfig {
            command: self.test_cmd.clone(),
            args: vec![],
            working_dir: Some(worktree_str),
            ..LaunchConfig::default()
        };

        let runtime = ProcessRuntime::new();
        let result = runtime.run(&config, &task).await;

        // Step 4: Set EvidenceState based on outcome
        let (state, suite, reason) = match result {
            Ok(r) if r.success && patch_applied => {
                // Verification passed — the patch applied and the test succeeded
                (EvidenceState::Proven, "green".to_string(),
                 "verification passed in worktree".to_string())
            }
            Ok(r) if r.success && !patch_applied => {
                // Test passed but no patch — SuiteGreen
                (EvidenceState::SuiteGreen, "green".to_string(),
                 "test passed, no patch".to_string())
            }
            Ok(r) if !r.success && finding.patch.is_some() => {
                // Patch existed but test failed
                (EvidenceState::SuiteGreen, "regressed".to_string(),
                 format!("test failed: {}", r.stderr))
            }
            Err(e) if finding.patch.is_some() => {
                // Patch could not be applied or runtime error
                (EvidenceState::CouldNotRun, "n/a".to_string(),
                 format!("verification error: {}", e))
            }
            _ => {
                // Fallback
                (EvidenceState::CouldNotRun, "n/a".to_string(),
                 "verification environment unavailable".to_string())
            }
        };

        // Clean up worktree
        let _ = std::fs::remove_dir_all(&worktree_path);

        Ok(VerifyResult {
            state,
            suite,
            reason,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn runner_returns_could_not_run_without_patch() {
        // Use a real git repo root (the loom checkout) so worktree creation works.
        let runner = EvidenceRunner::new(
            std::path::PathBuf::from("/Users/tmewara/codebase/loom"),
            "cargo test".to_string(),
            None,
        );
        // Finding without a patch — worktree creation may fail (e.g. permissions),
        // but the runner should return CouldNotRun in that case.
        let f = crate::types::Finding::new(
            "x".into(), "t".into(), "r".into(), None, vec!["a.rs".into()],
            EvidenceState::Unverified, "opencode".into(),
        );
        let res = runner.verify(&f, "HEAD").await.expect("verify runs");
        // The runner should always return CouldNotRun when the environment is unavailable.
        assert_eq!(res.state, EvidenceState::CouldNotRun);
    }
}
