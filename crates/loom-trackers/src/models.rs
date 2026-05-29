//! Shared models for issue tracker integration.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// A single issue from any tracker.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Issue {
    /// Tracker-internal ID (e.g. issue number for GitHub).
    pub id: String,
    /// Human-readable title.
    pub title: String,
    /// Current state (open, closed, in_progress, etc.).
    pub state: String,
    /// Body / description (may be markdown).
    pub body: Option<String>,
    /// Labels or tags.
    #[serde(default)]
    pub labels: Vec<String>,
    /// Assignee login (if any).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub assignee: Option<String>,
    /// URL to view the issue in a browser.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub html_url: Option<String>,
    /// Extra metadata per tracker (tracker-specific fields).
    #[serde(default)]
    pub metadata: HashMap<String, String>,
    /// Timestamp (ISO 8601).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created_at: Option<String>,
}

/// Query parameters when listing issues.
#[derive(Debug, Clone)]
pub struct IssueQuery {
    /// Repository identifier (e.g. "owner/repo").
    pub repo: String,
    /// Filter by state. Typical values: "open", "closed", "all".
    pub state: Option<String>,
    /// Filter by label (comma-separated).
    pub labels: Option<String>,
    /// Maximum results.
    pub limit: Option<u32>,
}

impl Default for IssueQuery {
    fn default() -> Self {
        Self {
            repo: String::new(),
            state: Some("open".into()),
            labels: None,
            limit: Some(30),
        }
    }
}

/// Errors from tracker operations.
#[derive(Debug, thiserror::Error)]
pub enum TrackerError {
    #[error("GitHub API error: {0}")]
    GitHubApi(String),

    #[error("Authentication failed: {0}")]
    Auth(String),

    #[error("Not found: {0}")]
    NotFound(String),

    #[error("Rate limited")]
    RateLimited,

    #[error("HTTP error: {0} ({1})")]
    Http(reqwest::StatusCode, String),

    #[error("Configuration error: {0}")]
    Config(String),

    #[error("Branch creation failed: {0}")]
    BranchCreation(String),

    #[error(transparent)]
    Other(#[from] anyhow::Error),
}
