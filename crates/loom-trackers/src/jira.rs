//! Jira issue tracker backend (REST API v3).
//!
//! Uses Basic Auth with an email + API token (or a Personal Access Token).
//! Configure via `JIRA_HOST`, `JIRA_EMAIL`, and `JIRA_TOKEN` environment
//! variables, or pass `--host` / `--token` on the command line.

use crate::models::{Issue, IssueQuery, TrackerError};
use crate::tracker::IssueTracker;
use reqwest::Client;

/// The default Jira Cloud REST API path.
const JIRA_API_PATH: &str = "/rest/api/3";

/// Jira issue tracker backend.
#[derive(Debug)]
pub struct JiraTracker {
    client: Client,
    base_url: String,
    auth_header: String,
}

impl JiraTracker {
    /// Create a new Jira tracker.
    ///
    /// Reads `JIRA_HOST`, `JIRA_EMAIL`, and `JIRA_TOKEN` from the environment
    /// when not explicitly provided.
    ///
    /// `host`: Jira instance base URL, e.g. `https://mycompany.atlassian.net`
    /// `auth_token`: Jira API token (for email+token) or Personal Access Token.
    /// `email`: Jira account email (optional when using PAT).
    pub fn new(
        host_override: Option<String>,
        email_override: Option<String>,
        token_override: Option<String>,
    ) -> Result<Self, TrackerError> {
        let host = host_override
            .or_else(|| std::env::var("JIRA_HOST").ok())
            .ok_or_else(|| {
                TrackerError::Config(
                    "JIRA_HOST must be set, or pass --host. e.g. https://mycompany.atlassian.net"
                        .into(),
                )
            })?;

        let token = token_override
            .or_else(|| std::env::var("JIRA_TOKEN").ok())
            .ok_or_else(|| {
                TrackerError::Config(
                    "JIRA_TOKEN must be set, or pass --token (API token or PAT)".into(),
                )
            })?;

        let base_url = host.trim_end_matches('/').to_string();

        let auth_header =
            if let Some(email) = email_override.or_else(|| std::env::var("JIRA_EMAIL").ok()) {
                // Basic auth with email + API token
                let credentials = format!("{}:{}", email.trim(), token);
                let encoded = base64::Engine::encode(
                    &base64::engine::general_purpose::STANDARD,
                    credentials.as_bytes(),
                );
                format!("Basic {encoded}")
            } else {
                // Bearer token (PAT)
                format!("Bearer {}", token.trim())
            };

        let client = Client::builder()
            .user_agent("loom-trackers/0.1")
            .build()
            .map_err(|e| TrackerError::Other(e.into()))?;

        Ok(Self {
            client,
            base_url,
            auth_header,
        })
    }

    /// Build a full API URL for a given path.
    fn api_url(&self, path: &str) -> String {
        format!("{}{}{}", self.base_url, JIRA_API_PATH, path)
    }

    /// Execute a GET request and deserialize the response.
    async fn get<T: serde::de::DeserializeOwned>(&self, url: &str) -> Result<T, TrackerError> {
        let resp = self
            .client
            .get(url)
            .header("Authorization", &self.auth_header)
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
            .header("Authorization", &self.auth_header)
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

    /// Parse a full Jira issue URL or key into the issue key.
    fn parse_key(input: &str) -> String {
        // Accept either "PROJECT-123" or "https://domain.atlassian.net/browse/PROJECT-123"
        if let Some(pos) = input.find("/browse/") {
            input[pos + "/browse/".len()..].to_string()
        } else {
            input.trim().to_string()
        }
    }
}

#[async_trait::async_trait]
impl IssueTracker for JiraTracker {
    fn name(&self) -> &str {
        "jira"
    }

    async fn get_issue(&self, id: &str) -> Result<Issue, TrackerError> {
        let key = Self::parse_key(id);
        let url = self.api_url(&format!("/issue/{key}"));
        let ji: JiraIssue = self.get(&url).await?;
        Ok(ji.into_issue(&self.base_url))
    }

    async fn list_issues(&self, query: &IssueQuery) -> Result<Vec<Issue>, TrackerError> {
        // Build JQL from query params
        let mut jql = String::new();

        if !query.repo.is_empty() {
            // repo field can be a project key
            jql.push_str(&format!("project = \"{}\"", query.repo));
        }

        if let Some(ref state) = query.state {
            if !jql.is_empty() {
                jql.push_str(" AND ");
            }
            jql.push_str(&format!("status = \"{}\"", state));
        }

        if let Some(ref labels) = query.labels {
            if !jql.is_empty() {
                jql.push_str(" AND ");
            }
            // labels can be comma-separated: "bug,frontend"
            let parts: Vec<&str> = labels.split(',').map(|l| l.trim()).collect();
            let label_jql: Vec<String> =
                parts.iter().map(|l| format!("labels = \"{l}\"")).collect();
            jql.push_str(&label_jql.join(" AND "));
        }

        if jql.is_empty() {
            jql.push_str("ORDER BY created DESC");
        }

        let limit = query.limit.unwrap_or(30).min(100);
        let url = self.api_url(&format!(
            "/search?jql={}&maxResults={}",
            urlencoding(&jql),
            limit
        ));

        let resp: JiraSearchResponse = self.get(&url).await?;
        let issues: Vec<Issue> = resp
            .issues
            .into_iter()
            .map(|ji| ji.into_issue(&self.base_url))
            .collect();
        Ok(issues)
    }

    async fn add_comment(&self, id: &str, body: &str) -> Result<(), TrackerError> {
        let key = Self::parse_key(id);
        let url = self.api_url(&format!("/issue/{key}/comment"));
        let payload = serde_json::json!({ "body": {
            "type": "doc",
            "version": 1,
            "content": [{
                "type": "paragraph",
                "content": [{
                    "text": body,
                    "type": "text"
                }]
            }]
        }});

        self.post(&url, &payload).await?;
        Ok(())
    }
}

// ─── Jira API types ────────────────────────────────────────────────────

#[derive(serde::Deserialize)]
struct JiraSearchResponse {
    issues: Vec<JiraIssue>,
    #[allow(dead_code)]
    total: u32,
}

#[derive(serde::Deserialize)]
#[allow(dead_code)]
struct JiraIssue {
    #[serde(default)]
    key: String,
    #[serde(default)]
    fields: JiraFields,
}

#[derive(serde::Deserialize, Default)]
struct JiraFields {
    #[serde(default)]
    summary: String,
    #[serde(default)]
    description: Option<serde_json::Value>,
    #[serde(default)]
    status: Option<JiraStatus>,
    #[serde(default)]
    labels: Vec<String>,
    #[serde(default)]
    assignee: Option<JiraUser>,
    #[serde(default)]
    created: Option<String>,
    #[serde(default)]
    #[allow(dead_code)]
    project: Option<JiraProject>,
}

#[derive(serde::Deserialize)]
struct JiraStatus {
    #[serde(default)]
    name: String,
}

#[derive(serde::Deserialize)]
struct JiraUser {
    #[serde(default)]
    display_name: String,
}

#[derive(serde::Deserialize)]
#[allow(dead_code)]
struct JiraProject {
    #[serde(default)]
    key: String,
    #[serde(default)]
    name: String,
}

impl JiraIssue {
    fn into_issue(self, base_url: &str) -> Issue {
        let description_text = self.fields.description.map(extract_text_from_adf);
        let html_url = Some(format!(
            "{}/browse/{}",
            base_url.trim_end_matches('/'),
            self.key
        ));
        let state = self
            .fields
            .status
            .map(|s| s.name)
            .unwrap_or_else(|| "unknown".to_string());
        let assignee = self.fields.assignee.map(|u| u.display_name);

        Issue {
            id: self.key,
            title: self.fields.summary,
            state,
            body: description_text,
            labels: self.fields.labels,
            assignee,
            html_url,
            metadata: std::collections::HashMap::new(),
            created_at: self.fields.created,
        }
    }
}

/// Extract plain text from Atlassian Document Format (ADF).
fn extract_text_from_adf(value: serde_json::Value) -> String {
    let mut text = String::new();
    extract_adf_text(&value, &mut text);
    text
}

fn extract_adf_text(value: &serde_json::Value, output: &mut String) {
    match value.get("type").and_then(|t| t.as_str()) {
        Some("text") => {
            if let Some(t) = value.get("text").and_then(|t| t.as_str()) {
                output.push_str(t);
            }
        }
        Some("paragraph") | Some("heading") | Some("bulletList") | Some("orderedList")
        | Some("listItem") | Some("codeBlock") | Some("blockquote") => {
            if let Some(content) = value.get("content").and_then(|c| c.as_array()) {
                for child in content {
                    extract_adf_text(child, output);
                }
            }
            if matches!(
                value.get("type").and_then(|t| t.as_str()),
                Some("paragraph") | Some("heading")
            ) {
                output.push('\n');
            }
        }
        _ => {
            // Recurse into content if present, handle unknown wrapper types
            if let Some(content) = value.get("content").and_then(|c| c.as_array()) {
                for child in content {
                    extract_adf_text(child, output);
                }
            }
        }
    }
}

/// URL-encode a string for use in a URL query parameter.
fn urlencoding(s: &str) -> String {
    s.chars()
        .map(|c| match c {
            'A'..='Z' | 'a'..='z' | '0'..='9' | '_' | '-' | '.' | '~' => c.to_string(),
            ' ' => "+".to_string(),
            other => format!("%{:02X}", other as u8),
        })
        .collect()
}
