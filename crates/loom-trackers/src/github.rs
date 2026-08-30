//! GitHub Issues tracker implementation.

use crate::models::{Issue, IssueQuery, TrackerError};
use crate::tracker::IssueTracker;
use reqwest::Client;

/// Check run status from GitHub API.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct CiStatus {
    /// The CI check name (e.g. "CI / build", "Lint").
    pub name: String,
    /// Status: "success", "failure", "pending", "queued", "in_progress", "cancelled", "neutral"
    pub status: String,
    /// URL to the check run details.
    pub url: Option<String>,
}

impl GitHubTracker {
    /// List pull requests for a repository.
    pub async fn list_pull_requests(&self, repo: &str, state: &str) -> Result<Vec<Issue>, TrackerError> {
        let url = self.api_url(&format!("/repos/{}/pulls?state={}", repo, urlencoding(state)));
        let prs: Vec<GitHubIssue> = self.get(&url).await?;
        Ok(prs.into_iter().map(|pr| pr.into_issue(repo)).collect())
    }

    /// Fetch CI status for a pull request's head commit.
    pub async fn fetch_pr_ci_status(&self, repo: &str, pr_number: u64) -> Result<Vec<CiStatus>, TrackerError> {
        // First, get the PR to find the head SHA
        let pr_url = self.api_url(&format!("/repos/{}/pulls/{}", repo, pr_number));
        let pr: serde_json::Value = self.get(&pr_url).await?;
        let head_sha = pr["head"]["sha"].as_str()
            .ok_or_else(|| TrackerError::Other(anyhow::anyhow!("No head SHA on PR")))?;

        // Get check runs for the head commit
        let checks_url = self.api_url(&format!("/repos/{}/commits/{}/check-runs", repo, head_sha));
        let checks: CheckRunsResponse = match self.get(&checks_url).await {
            Ok(c) => c,
            Err(TrackerError::NotFound(_)) => {
                // The repo has no check runs (e.g. no GitHub Actions config).
                // Fall back to the combined status API only in this case.
                let status_url = self.api_url(&format!("/repos/{}/commits/{}/status", repo, head_sha));
                let combined: CombinedStatus = self.get(&status_url).await?;
                return Ok(combined.statuses.into_iter().map(|s| CiStatus {
                    name: s.context,
                    status: match s.state.as_str() {
                        "success" => "success".into(),
                        "failure" | "error" => "failure".into(),
                        _ => "pending".into(),
                    },
                    url: s.target_url,
                }).collect());
            }
            Err(e) => return Err(e),
        };

        Ok(checks.check_runs.into_iter().map(|c| CiStatus {
            name: c.name,
            status: c.conclusion.clone().unwrap_or(c.status),
            url: c.html_url,
        }).collect())
    }

    /// Fetch the diff for a pull request.
    pub async fn fetch_pr_diff(&self, repo: &str, pr_number: u64) -> Result<String, TrackerError> {
        let url = self.api_url(&format!("/repos/{}/pulls/{}", repo, pr_number));
        let resp = self.client
            .get(&url)
            .header(reqwest::header::ACCEPT, "application/vnd.github.v3.diff")
            .send()
            .await
            .map_err(|e| TrackerError::GitHubApi(e.to_string()))?;

        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(TrackerError::Http(status, body));
        }

        resp.text().await.map_err(|e| TrackerError::GitHubApi(format!("Failed to read diff: {e}")))
    }
}

/// The base URL for the GitHub REST API v3.
const GITHUB_API_BASE: &str = "https://api.github.com";

/// GitHub Issues backend.
pub struct GitHubTracker {
    client: Client,
}

impl GitHubTracker {
    /// Create a new GitHub tracker.
    ///
    /// Reads `GITHUB_TOKEN` from the environment if `token_override` is `None`.
    pub fn new(token_override: Option<String>) -> Result<Self, TrackerError> {
        let token = match token_override {
            Some(t) => t,
            None => std::env::var("GITHUB_TOKEN")
                .or_else(|_| std::env::var("GH_TOKEN"))
                .map_err(|_| {
                    TrackerError::Config(
                        "GITHUB_TOKEN or GH_TOKEN must be set, or pass --token".into(),
                    )
                })?,
        };

        let client = Client::builder()
            .user_agent("loom-trackers/0.1")
            .default_headers({
                let mut headers = reqwest::header::HeaderMap::new();
                let mut auth_value = reqwest::header::HeaderValue::from_str(&format!("Bearer {}", token))
                    .map_err(|e| TrackerError::Config(format!("Invalid token format: {e}")))?;
                auth_value.set_sensitive(true);
                headers.insert(reqwest::header::AUTHORIZATION, auth_value);
                headers.insert(
                    reqwest::header::ACCEPT,
                    reqwest::header::HeaderValue::from_static("application/vnd.github.v3+json"),
                );
                headers
            })
            .build()
            .map_err(|e| TrackerError::Other(e.into()))?;

        Ok(Self { client })
    }

    /// Build API URL for a given path.
    fn api_url(&self, path: &str) -> String {
        format!("{}{}", GITHUB_API_BASE, path)
    }

    /// Execute a GET request and deserialize the response.
    pub(crate) async fn get<T: serde::de::DeserializeOwned>(&self, url: &str) -> Result<T, TrackerError> {
        let resp = self
            .client
            .get(url)
            .send()
            .await
            .map_err(|e| TrackerError::GitHubApi(e.to_string()))?;

        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            return match status.as_u16() {
                401 | 403 => Err(TrackerError::Auth(body)),
                404 => Err(TrackerError::NotFound(body)),
                429 => Err(TrackerError::RateLimited),
                _ => Err(TrackerError::Http(status, body)),
            };
        }

        resp.json::<T>()
            .await
            .map_err(|e| TrackerError::GitHubApi(format!("Failed to parse response: {e}")))
    }
}

#[async_trait::async_trait]
impl IssueTracker for GitHubTracker {
    fn name(&self) -> &str {
        "github"
    }

    async fn get_issue(&self, id: &str) -> Result<Issue, TrackerError> {
        // id may be just a number like "123" or "owner/repo#123"
        let (repo, number) = if id.contains('#') {
            let mut parts = id.splitn(2, '#');
            (
                parts.next().unwrap().to_string(),
                parts.next().unwrap().to_string(),
            )
        } else {
            return Err(TrackerError::NotFound(
                "Issue ID must be in format 'owner/repo#number' or set repo via --repo".into(),
            ));
        };

        let url = self.api_url(&format!("/repos/{}/issues/{}", repo, number));
        let gh: GitHubIssue = self.get(&url).await?;
        Ok(gh.into_issue(&repo))
    }

    async fn list_issues(&self, query: &IssueQuery) -> Result<Vec<Issue>, TrackerError> {
        let mut url = self.api_url(&format!("/repos/{}/issues", query.repo));
        let mut params = vec![];

        let state = query.state.as_deref().unwrap_or("open");
        params.push(format!("state={}", urlencoding(state)));

        if let Some(labels) = &query.labels {
            params.push(format!("labels={}", urlencoding(labels)));
        }

        if let Some(limit) = query.limit {
            params.push(format!("per_page={}", limit.min(100)));
        }

        if !params.is_empty() {
            url.push('?');
            url.push_str(&params.join("&"));
        }

        let issues: Vec<GitHubIssue> = self.get(&url).await?;
        Ok(issues.into_iter().map(|i| i.into_issue(&query.repo)).collect())
    }

    async fn add_comment(&self, id: &str, body: &str) -> Result<(), TrackerError> {
        // id may be "owner/repo#number"
        let (repo, number) = if id.contains('#') {
            let mut parts = id.splitn(2, '#');
            (
                parts.next().unwrap().to_string(),
                parts.next().unwrap().to_string(),
            )
        } else {
            return Err(TrackerError::NotFound(
                "Issue ID must be in format 'owner/repo#number'".into(),
            ));
        };

        let url = self.api_url(&format!("/repos/{}/issues/{}/comments", repo, number));
        let payload = serde_json::json!({ "body": body });

        let resp = self
            .client
            .post(&url)
            .json(&payload)
            .send()
            .await
            .map_err(|e| TrackerError::GitHubApi(e.to_string()))?;

        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            return match status.as_u16() {
                401 | 403 => Err(TrackerError::Auth(body)),
                404 => Err(TrackerError::NotFound(body)),
                429 => Err(TrackerError::RateLimited),
                _ => Err(TrackerError::Http(status, body)),
            };
        }

        Ok(())
    }
}

/// A GitHub check run (CI status).
#[derive(Debug, Clone, serde::Deserialize)]
pub struct CheckRun {
    pub name: String,
    pub status: String,       // "queued", "in_progress", "completed"
    pub conclusion: Option<String>, // "success", "failure", "neutral", "cancelled", etc.
    pub html_url: Option<String>,
    pub started_at: Option<String>,
    pub completed_at: Option<String>,
}

/// Check runs response from GitHub API.
#[derive(serde::Deserialize)]
struct CheckRunsResponse {
    check_runs: Vec<CheckRun>,
    total_count: u64,
}

/// Combined status for a commit.
#[derive(serde::Deserialize)]
struct CombinedStatus {
    state: String,   // "success", "failure", "pending"
    statuses: Vec<StatusEntry>,
    total_count: u64,
}

#[derive(serde::Deserialize)]
struct StatusEntry {
    context: String,
    state: String,
    description: Option<String>,
    target_url: Option<String>,
}

/// Minimal GitHub API issue representation.
#[derive(serde::Deserialize)]
struct GitHubIssue {
    number: u64,
    title: String,
    state: String,
    body: Option<String>,
    html_url: Option<String>,
    labels: Vec<GitHubLabel>,
    assignee: Option<GitHubUser>,
    created_at: Option<String>,
    pull_request: Option<serde_json::Value>,
    /// Head SHA for PRs (used to query CI status).
    head: Option<GitHubPRHead>,
}

#[derive(serde::Deserialize)]
struct GitHubPRHead {
    sha: String,
    repo: Option<GitHubPRRepo>,
}

#[derive(serde::Deserialize)]
struct GitHubPRRepo {
    full_name: Option<String>,
}

#[derive(serde::Deserialize)]
struct GitHubLabel {
    name: String,
}

#[derive(serde::Deserialize)]
struct GitHubUser {
    login: String,
}

impl GitHubIssue {
    fn into_issue(self, repo: &str) -> Issue {
        let mut labels: Vec<String> = self.labels.into_iter().map(|l| l.name).collect();
        labels.sort();

        let mut metadata = std::collections::HashMap::new();
        metadata.insert("repo".into(), repo.to_string());
        if self.pull_request.is_some() {
            metadata.insert("is_pr".into(), "true".into());
        }

        Issue {
            id: format!("{}#{}", repo, self.number),
            title: self.title,
            state: self.state,
            body: self.body,
            labels,
            assignee: self.assignee.map(|u| u.login),
            html_url: self.html_url,
            metadata,
            created_at: self.created_at,
        }
    }
}

/// Simple URL-encoding since we don't want to add a dependency.
fn urlencoding(input: &str) -> String {
    input
        .chars()
        .map(|c| match c {
            'a'..='z' | 'A'..='Z' | '0'..='9' | '-' | '_' | '.' | '~' => c.to_string(),
            ' ' => "+".to_string(),
            other => format!("%{:02X}", other as u8),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_urlencoding() {
        assert_eq!(urlencoding("hello world"), "hello+world");
        assert_eq!(urlencoding("a/b"), "a%2Fb");
        assert_eq!(urlencoding("open"), "open");
    }

    #[test]
    fn test_slugify() {
        let slug = crate::tracker::slugify("Fix the login bug!");
        assert_eq!(slug, "fix-the-login-bug");
    }

    #[test]
    fn test_suggest_branch_name() {
        let tracker = GitHubTracker::new(Some("test-token".into())).unwrap();
        let issue = Issue {
            id: "owner/repo#42".into(),
            title: "Fix the login bug".into(),
            state: "open".into(),
            body: None,
            labels: vec![],
            assignee: None,
            html_url: None,
            metadata: std::collections::HashMap::new(),
            created_at: None,
        };
        let branch = tracker.suggest_branch_name(&issue);
        assert_eq!(branch, Some("owner/repo#42/fix-the-login-bug".into()));
    }

    #[test]
    fn test_add_comment_parses_issue_id() {
        let tracker = GitHubTracker::new(Some("test-token".into())).unwrap();
        // This will fail with network error (no real GitHub API), but
        // the issue ID parsing should happen first and error with auth.
        let rt = tokio::runtime::Runtime::new().unwrap();
        let result = rt.block_on(tracker.add_comment("invalid", "test body"));
        assert!(result.is_err());
        let err = result.unwrap_err();
        // Should be NotFound because "invalid" doesn't contain '#'
        assert!(
            matches!(err, TrackerError::NotFound(_)),
            "expected NotFound for missing '#', got: {err:?}"
        );

        // With valid format, should fail with auth (network error with 401)
        let result2 = rt.block_on(tracker.add_comment("owner/repo#42", "test body"));
        assert!(result2.is_err());
    }
}
