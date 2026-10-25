//! Outcome-gated lessons: one record per terminal finding, confirmed or
//! dismissed by a human before it is promoted to durable knowledge.
//!
//! Only findings with a real outcome generate lessons (`proven`,
//! `suite_green` = the defect held; `refuted` = the reviewer was wrong).
//! `unverified` / `could_not_run` carry no outcome, so they produce nothing.

use crate::types::{EvidenceState, Finding};
use serde::{Deserialize, Serialize};

/// Human verdict on a lesson.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum LessonOutcome {
    #[default]
    Pending,
    Confirmed,
    Dismissed,
}

/// One lesson derived from a review finding's outcome.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Lesson {
    pub id: String,
    pub run_id: String,
    pub finding_id: String,
    pub title: String,
    /// Evidence state the finding ended with (`proven`/`suite_green`/`refuted`).
    pub state: String,
    pub outcome: LessonOutcome,
    /// Why a lesson was dismissed (required by `lesson dismiss`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    pub created_at: String,
}

impl Lesson {
    /// Derive lessons from a finished run's findings. Terminal findings
    /// only; ids are `{run_id}:{finding_id}` so they are unique across runs.
    pub fn derive(run_id: &str, findings: &[Finding]) -> Vec<Lesson> {
        findings
            .iter()
            .filter(|f| {
                matches!(
                    f.state,
                    EvidenceState::Proven | EvidenceState::SuiteGreen | EvidenceState::Refuted
                )
            })
            .map(|f| Lesson {
                id: format!("{run_id}:{}", f.id),
                run_id: run_id.to_string(),
                finding_id: f.id.clone(),
                title: f.title.clone(),
                state: f.state.label().to_string(),
                outcome: LessonOutcome::Pending,
                reason: None,
                created_at: chrono::Utc::now().to_rfc3339(),
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn finding(id: &str, state: EvidenceState) -> Finding {
        Finding::new(
            id.into(),
            "t".into(),
            "r".into(),
            Some("diff".into()),
            vec![],
            state,
            "seat".into(),
        )
    }

    #[test]
    fn only_terminal_findings_become_lessons() {
        let findings = vec![
            finding("f1", EvidenceState::Proven),
            finding("f2", EvidenceState::SuiteGreen),
            finding("f3", EvidenceState::Refuted),
            finding("f4", EvidenceState::Unverified),
            finding("f5", EvidenceState::CouldNotRun),
        ];
        let lessons = Lesson::derive("r1", &findings);
        assert_eq!(lessons.len(), 3);
        assert!(lessons.iter().all(|l| l.outcome == LessonOutcome::Pending));
        assert!(lessons.iter().any(|l| l.id == "r1:f3" && l.state == "refuted"));
    }
}
