//! IssueTracker trait — backend-agnostic interface for issue tracking systems.

use crate::models::{Issue, IssueQuery, TrackerError};

/// Trait for issue tracker backends (GitHub, GitLab, Linear, Jira, etc.).
#[async_trait::async_trait]
pub trait IssueTracker: Send + Sync {
    /// Human-readable name of this tracker.
    fn name(&self) -> &str;

    /// Fetch a single issue by its tracker ID.
    async fn get_issue(&self, id: &str) -> Result<Issue, TrackerError>;

    /// List issues matching the given query.
    async fn list_issues(&self, query: &IssueQuery) -> Result<Vec<Issue>, TrackerError>;

    /// Create a branch name from an issue (convention: `{type}/{issue-id}-{slug}`).
    fn suggest_branch_name(&self, issue: &Issue) -> Option<String> {
        let slug = Self::slugify(&issue.title);
        Some(format!("{}/{}", issue.id, slug))
    }

    /// Add a comment to an issue.
    async fn add_comment(&self, id: &str, body: &str) -> Result<(), TrackerError>;

    /// Helper: turn a title into a branch-safe slug.
    fn slugify(title: &str) -> String {
        title
            .to_lowercase()
            .chars()
            .map(|c| if c.is_alphanumeric() || c == '-' { c } else { '-' })
            .filter(|&c| c != '\'' && c != '"')
            .collect::<String>()
            .trim_matches('-')
            .to_string()
    }
}
