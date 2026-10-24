//! Evidence runner — verifies a finding patch in a fresh worktree.
//!
//! MVP uses `loom-parallel` worktrees via `loom-runtime`'s
//! `ProcessRuntime`. Not yet container-gated (pillar 2).

use crate::types::{EvidenceState, Finding};
use anyhow::Result;
use std::path::PathBuf;

/// Per-finding verification result.
#[derive(Debug, Clone)]
pub struct VerifyResult {
    pub state: EvidenceState,
    pub suite: String,      // "green" / "not run" / "regressed"
    pub reason: String,
}

/// Runs the evidence pipeline for one finding:
///   1. apply patch — fail → CouldNotRun "patch did not apply"
///   2. reviewer's repro: fails without fix + passes with fix → Proven
///   3. suite → note green / not run / regressed
pub struct EvidenceRunner {
    /// Working tree root where worktrees are created.
    pub root: PathBuf,
    /// Test command (e.g. `cargo test`).
    pub test_cmd: String,
    /// Setup command (run once before test).
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
    pub fn verify(&self, _finding: &Finding, _base_commit: &str) -> Result<VerifyResult> {
        // MVP: return unverified — real worktree execution lands
        // alongside loom-parallel integration. Placeholder keeps the
        // contract honest without shipping half-baked sandboxing.
        Ok(VerifyResult {
            state: EvidenceState::Unverified,
            suite: "not run".to_string(),
            reason: "worktree execution not yet wired; install loom-parallel integration".to_string(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn runner_returns_unverified_by_default() {
        let runner = EvidenceRunner::new(std::path::PathBuf::from("/tmp"), "cargo test".to_string(), None);
        let f = crate::types::Finding::new(
            "x".into(), "t".into(), "r".into(), None, vec!["a.rs".into()],
            EvidenceState::Unverified, "opencode".into(),
        );
        let res = runner.verify(&f, "HEAD").expect("verify runs");
        assert_eq!(res.state, EvidenceState::Unverified);
        assert_eq!(res.suite, "not run");
    }
}
