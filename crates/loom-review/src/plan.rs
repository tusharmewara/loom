//! Review plan extraction from a session.
//!
//! Extracts user intent + diff at session head — never the agent's
//! reasoning (blind review). Produces `ReviewPlan`.

use anyhow::Result;
use serde::{Deserialize, Serialize};

/// The extracted review plan: what the session changed and why.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ReviewPlan {
    pub session_id: String,
    /// Short description of user intent.
    pub intent: String,
    /// Files changed (paths).
    pub changed_files: Vec<String>,
    /// Unified diff at session head (truncated to first N lines).
    pub diff: Option<String>,
    /// Complexity score from loom-routing (stored at creation time).
    pub complexity_score: Option<f64>,
}

/// Extract a plan from a session. MVP: read intent + changed files from
/// the session manifest; read diff from the session's working tree if
/// available. For now, this is a skeleton that returns an intent + file
/// list from the session's topic/messages.
#[cfg_attr(not(test), expect(dead_code))]
pub fn plan_from_session(_session_id: &str) -> Result<ReviewPlan> {
    Ok(ReviewPlan {
        session_id: _session_id.to_string(),
        intent: String::new(),
        changed_files: Vec::new(),
        diff: None,
        complexity_score: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plan_from_session_returns_empty_plan() {
        let plan = plan_from_session("s1").expect("plan builds");
        assert_eq!(plan.session_id, "s1");
        assert!(plan.changed_files.is_empty());
        assert!(plan.diff.is_none());
    }
}
