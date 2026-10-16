//! GitLab issue tracker backend (REST API v4).
//!
//! Uses a Personal Access Token. Configure via `GITLAB_TOKEN` and
//! `GITLAB_HOST` (defaults to `https://gitlab.com`) environment
//! variables, or pass `--token` / `--host` on the command line.

use crate::models::{Issue, IssueQuery, TrackerError};
use crate::tracker::IssueTracker;
use reqwest::Client;

/// Default GitLab instance.
const DEFAULT_GITLAB_HOST: &str = "https://gitlab.com";

/// GitLab issue tracker backend.
#[derive(Debug)]
pub struct GitLabTracker {
    client: Client,
    base_url: String,
    token: String,
}

impl GitLabTracker {
    /// Create a new GitLab tracker.
    ///
    /// Reads `GITLAB_TOKEN` and `GITLAB_HOST` from the environment when not
    /// explicitly provided.
    pub fn new(
        host_override: Option<String>,
        token_override: Option<String>,
    ) -> Result<Self, TrackerError> {
        let host = host_override
            .or_else(|| std::env::var("GITLAB_HOST").ok())
            .unwrap_or_else(|| DEFAULT_GITLAB_HOST.to_string());

        let token = token_override
            .or_else(|| std::env::var("GITLAB_TOKEN").ok())
            .ok_or_else(|| {
                TrackerError::Config("GITLAB_TOKEN must be set, or pass --token".into())
            })?;

        let base_url = host.trim_end_matches('/').to_string();

        let client = Client::builder()
            .user_agent("loom-trackers/0.1")
            .build()
            .map_err(|e| TrackerError::Other(e.into()))?;

        Ok(Self {
            client,
            base_url,
            token,
        })
    }

    /// Build a full API URL for a given path.
    fn api_url(&self, path: &str) -> String {
        format!("{}/api/v4{}", self.base_url, path)
    }

    /// Execute a GET request and deserialize the response.
    async fn get<T: serde::de::DeserializeOwned>(&self, url: &str) -> Result<T, TrackerError> {
        let resp = self
            .client
            .get(url)
            .header("PRIVATE-TOKEN", &self.token)
            .header("Accept", "application/json")
            .send()
            .await
            .map_err(|e| TrackerError::GitHubApi(e.to_string()))?;

        Self::handle_response(resp).await
    }

    /// Execute a POST request with a JSON body.
    async fn post(
        &self,
        url: &str,
        body: &serde_json::Value,
    ) -> Result<serde_json::Value, TrackerError> {
        let resp = self
            .client
            .post(url)
            .header("PRIVATE-TOKEN", &self.token)
            .header("Content-Type", "application/json")
            .json(body)
            .send()
            .await
            .map_err(|e| TrackerError::GitHubApi(e.to_string()))?;

        Self::handle_response(resp).await
    }

    /// Common response handling.
    async fn handle_response<T: serde::de::DeserializeOwned>(
        resp: reqwest::Response,
    ) -> Result<T, TrackerError> {
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

    /// URL-encode a project path for use in API URLs.
    fn encode_project_path(path: &str) -> String {
        path.replace('/', "%2F")
    }
}

#[async_trait::async_trait]
impl IssueTracker for GitLabTracker {
    fn name(&self) -> &str {
        "gitlab"
    }

    async fn get_issue(&self, id: &str) -> Result<Issue, TrackerError> {
        // id format: "owner/repo#123"
        let (project, iid) = if let Some(pos) = id.rfind('#') {
            (id[..pos].to_string(), id[pos + 1..].to_string())
        } else {
            return Err(TrackerError::NotFound(
                "Issue ID must be in format 'owner/repo#number'".into(),
            ));
        };

        let encoded = Self::encode_project_path(&project);
        let url = self.api_url(&format!("/projects/{encoded}/issues/{iid}"));
        let gl: GitLabIssue = self.get(&url).await?;
        Ok(gl.into_issue(&project))
    }

    async fn list_issues(&self, query: &IssueQuery) -> Result<Vec<Issue>, TrackerError> {
        let mut url = self.api_url(&format!(
            "/projects/{}/issues",
            Self::encode_project_path(&query.repo)
        ));

        let mut params: Vec<String> = Vec::new();

        if let Some(ref state) = query.state {
            params.push(format!("state={}", state));
        }

        if let Some(ref labels) = query.labels {
            params.push(format!("labels={}", labels));
        }

        if let Some(limit) = query.limit {
            params.push(format!("per_page={}", limit.min(100)));
        }

        if !params.is_empty() {
            url.push('?');
            url.push_str(&params.join("&"));
        }

        let issues: Vec<GitLabIssue> = self.get(&url).await?;
        Ok(issues
            .into_iter()
            .map(|i| i.into_issue(&query.repo))
            .collect())
    }

    async fn add_comment(&self, id: &str, body: &str) -> Result<(), TrackerError> {
        let (project, iid) = if let Some(pos) = id.rfind('#') {
            (id[..pos].to_string(), id[pos + 1..].to_string())
        } else {
            return Err(TrackerError::NotFound(
                "Issue ID must be in format 'owner/repo#number'".into(),
            ));
        };

        let encoded = Self::encode_project_path(&project);
        let url = self.api_url(&format!("/projects/{encoded}/issues/{iid}/notes"));
        let payload = serde_json::json!({ "body": body });

        self.post(&url, &payload).await?;
        Ok(())
    }
}

// ─── GitLab API types ──────────────────────────────────────────────────

#[derive(serde::Deserialize)]
struct GitLabIssue {
    #[serde(default)]
    iid: u64,
    #[serde(default)]
    title: String,
    #[serde(default)]
    state: String,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    labels: Vec<String>,
    #[serde(default)]
    assignee: Option<GitLabUser>,
    #[serde(default)]
    web_url: Option<String>,
    #[serde(default)]
    created_at: Option<String>,
}

#[derive(serde::Deserialize)]
struct GitLabUser {
    #[serde(default)]
    name: String,
}

impl GitLabIssue {
    fn into_issue(self, project: &str) -> Issue {
        Issue {
            id: format!("{}#{}", project, self.iid),
            title: self.title,
            state: self.state,
            body: self.description,
            labels: self.labels,
            assignee: self.assignee.map(|u| u.name),
            html_url: self.web_url,
            metadata: std::collections::HashMap::new(),
            created_at: self.created_at,
        }
    }
}
