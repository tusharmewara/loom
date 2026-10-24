//! Core review types — evidence states, findings, run lifecycle.

use serde::{Deserialize, Serialize};

/// Evidence state of a finding — copied verbatim from Parallax.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum EvidenceState {
    #[default]
    Unverified,
    Proven,
    SuiteGreen,
    CouldNotRun,
    Refuted,
}

impl EvidenceState {
    pub fn label(self) -> &'static str {
        match self {
            Self::Proven => "proven",
            Self::SuiteGreen => "suite_green",
            Self::Unverified => "unverified",
            Self::CouldNotRun => "could_not_run",
            Self::Refuted => "refuted",
        }
    }

    pub fn weight(self) -> f64 {
        match self {
            Self::Proven => 2.0,
            Self::SuiteGreen => 1.0,
            Self::Unverified => 0.8,
            Self::CouldNotRun => 0.8,
            Self::Refuted => 0.2,
        }
    }

    /// A proven finding stays proven even if the suite later fails.
    pub fn terminal(self) -> bool {
        matches!(self, Self::Proven | Self::Refuted)
    }
}

impl std::fmt::Display for EvidenceState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.label())
    }
}

/// A single finding — one mechanism, not one copy. See dedup rules.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Finding {
    /// Stable id for this mechanism.
    pub id: String,
    /// Short title.
    pub title: String,
    /// Full reason / explanation.
    pub reason: String,
    /// Patch (unified diff) addressing this finding.
    pub patch: Option<String>,
    /// File(s) touched by the patch.
    pub files: Vec<String>,
    /// What the sandbox established.
    pub state: EvidenceState,
    /// Suite note: green / not run / regressed (regressed → excluded).
    pub suite: Option<String>,
    /// Confidence in [0, 1].
    pub confidence: f64,
    /// The lens/seat that produced this (strongest copy carried forward).
    pub lens: String,
}

impl Finding {
    pub fn new(
        id: String,
        title: String,
        reason: String,
        patch: Option<String>,
        files: Vec<String>,
        state: EvidenceState,
        lens: String,
    ) -> Self {
        Self {
            id,
            title,
            reason,
            patch,
            files,
            state,
            suite: None,
            confidence: state.weight(),
            lens,
        }
    }
}

/// Lifecycle stage of a review run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum RunStatus {
    #[default]
    Pending,
    Planning,
    Seating,
    Reviewing,
    Verifying,
    Reporting,
    Done,
    Failed,
}

/// The full review run record.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ReviewRun {
    pub id: String,
    pub session_id: String,
    pub status: RunStatus,
    pub created_at: String,
    pub updated_at: String,
    pub seats: Vec<String>,
    pub lenses: Vec<String>,
    pub findings: Vec<Finding>,
    pub report_path: Option<String>,
}

impl ReviewRun {
    pub fn new(id: String, session_id: String) -> Self {
        let now = chrono::Utc::now().to_rfc3339();
        Self {
            id,
            session_id,
            status: RunStatus::Pending,
            created_at: now.clone(),
            updated_at: now,
            seats: Vec::new(),
            lenses: Vec::new(),
            findings: Vec::new(),
            report_path: None,
        }
    }
}
