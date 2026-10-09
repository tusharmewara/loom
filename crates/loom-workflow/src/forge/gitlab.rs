//! GitLab forge integration.

use anyhow::{Context, Result};
use async_trait::async_trait;
use hmac::{Hmac, Mac};
use reqwest::Client;
use serde::Deserialize;
use sha2::Sha256;
use std::env;

use super::{Forge, PrInfo};

/// GitLab webhook payload for merge request events.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
#[allow(dead_code)]
struct GitLabMrPayload {
    object_kind: String,
    event_type: Option<String>,
    object_attributes: GitLabMrAttributes,
    project: GitLabProject,
    user: GitLabUser,
    labels: Option<Vec<GitLabLabel>>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
#[allow(dead_code)]
struct GitLabMrAttributes {
    iid: u64,
    title: String,
    description: Option<String>,
    source_branch: String,
    target_branch: String,
    sha: String,
    work_in_progress: bool,
    state: String,
    url: String,
    action: String,
    merge_status: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
#[allow(dead_code)]
struct GitLabProject {
    id: u64,
    name: String,
    path_with_namespace: String,
    namespace: GitLabNamespace,
    web_url: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
#[allow(dead_code)]
struct GitLabNamespace {
    path: String,
    kind: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
struct GitLabUser {
    username: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
struct GitLabLabel {
    title: String,
}

/// GitLab forge implementation.
pub struct GitLabForge {
    client: Client,
    webhook_secret: Option<String>,
    token: Option<String>,
    base_url: String,
}

impl GitLabForge {
    /// Create from environment variables.
    pub fn from_env(loom_dir: &str) -> Result<Self> {
        let token = env::var("GITLAB_TOKEN").ok()
            .or_else(|| {
                std::fs::read_to_string(format!("{}/config.toml", loom_dir))
                    .ok()
                    .and_then(|c| toml::from_str::<toml::Value>(&c).ok())
                    .and_then(|v| v.get("gitlab")?.get("token")?.as_str().map(|s| s.to_string()))
            });

        let webhook_secret = env::var("GITLAB_WEBHOOK_SECRET").ok()
            .or_else(|| {
                std::fs::read_to_string(format!("{}/config.toml", loom_dir))
                    .ok()
                    .and_then(|c| toml::from_str::<toml::Value>(&c).ok())
                    .and_then(|v| v.get("gitlab")?.get("webhook_secret")?.as_str().map(|s| s.to_string()))
            });

        let base_url = env::var("GITLAB_URL").unwrap_or_else(|_| "https://gitlab.com".to_string());

        Ok(Self {
            client: Client::new(),
            webhook_secret,
            token,
            base_url,
        })
    }
}

#[async_trait]
impl Forge for GitLabForge {
    fn verify_signature(&self, payload: &[u8], signature: &str) -> bool {
        let Some(secret) = &self.webhook_secret else {
            return true;
        };

        // GitLab uses X-Gitlab-Token header for webhook verification
        let expected = signature.strip_prefix("sha256=").unwrap_or(signature);
        let mut mac = Hmac::<Sha256>::new_from_slice(secret.as_bytes()).unwrap();
        mac.update(payload);
        let result = mac.finalize().into_bytes();
        let expected_bytes = hex::decode(expected).unwrap_or_default();
        result.as_slice() == expected_bytes.as_slice()
    }

    fn parse_pr_payload(&self, payload: &[u8]) -> Result<Option<PrInfo>> {
        let gl_payload: GitLabMrPayload = serde_json::from_slice(payload)
            .context("Failed to parse GitLab webhook payload")?;

        // Only handle merge request events
        if gl_payload.object_kind != "merge_request" {
            return Ok(None);
        }

        // Only handle relevant actions
        let action = gl_payload.object_attributes.action.as_str();
        if !matches!(action, "open" | "reopen" | "update" | "approved" | "unapproved") {
            return Ok(None);
        }

        let mr = &gl_payload.object_attributes;
        let project = &gl_payload.project;
        let owner = project.namespace.path.clone();

        Ok(Some(PrInfo {
            forge: "gitlab".to_string(),
            owner,
            repo: project.name.clone(),
            number: mr.iid,
            title: mr.title.clone(),
            body: mr.description.clone().unwrap_or_default(),
            head_sha: mr.sha.clone(),
            base_sha: mr.sha.clone(), // GitLab doesn't expose base SHA directly in webhook
            head_ref: mr.source_branch.clone(),
            base_ref: mr.target_branch.clone(),
            author: gl_payload.user.username.clone(),
            url: format!("{}/api/v4/projects/{}/merge_requests/{}", self.base_url, project.id, mr.iid),
            html_url: mr.url.clone(),
            is_draft: mr.work_in_progress,
            labels: gl_payload.labels.unwrap_or_default().into_iter().map(|l| l.title).collect(),
        }))
    }

    async fn post_review(&self, pr: &PrInfo, body: &str) -> Result<()> {
        let token = self.token.as_ref()
            .ok_or_else(|| anyhow::anyhow!("GitLab token not configured"))?;

        // Extract project ID from URL or owner/repo
        let project_path = format!("{}/{}", pr.owner, pr.repo);
        let encoded = urlencoding::encode(&project_path);
        let url = format!("{}/api/v4/projects/{}/merge_requests/{}/notes", self.base_url, encoded, pr.number);

        let note = serde_json::json!({
            "body": body
        });

        let resp = self.client
            .post(&url)
            .header("Authorization", format!("Bearer {}", token))
            .header("User-Agent", "Loom")
            .json(&note)
            .send()
            .await
            .context("Failed to send GitLab review request")?;

        if !resp.status().is_success() {
            let err = resp.text().await.unwrap_or_default();
            anyhow::bail!("GitLab review failed: {}", err);
        }

        Ok(())
    }

    fn default_branch(&self) -> &str {
        "main"
    }

    fn name(&self) -> &str {
        "gitlab"
    }
}