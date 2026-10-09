//! GitHub forge integration.

use anyhow::{Context, Result};
use async_trait::async_trait;
use hmac::{Hmac, Mac};
use reqwest::Client;
use serde::Deserialize;
use sha2::Sha256;
use std::env;

use super::{Forge, PrInfo};

/// GitHub webhook payload for pull request events.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
#[allow(dead_code)]
struct GitHubPrPayload {
    action: String,
    number: u64,
    pull_request: GitHubPullRequest,
    repository: GitHubRepository,
    sender: GitHubUser,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct GitHubPullRequest {
    title: String,
    body: Option<String>,
    head: GitHubBranchRef,
    base: GitHubBranchRef,
    draft: bool,
    labels: Vec<GitHubLabel>,
    html_url: String,
    url: String,
    #[allow(dead_code)]
    head_sha: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
#[allow(dead_code)]
struct GitHubBranchRef {
    sha: String,
    #[serde(rename = "ref")]
    ref_name: String,
    repo: Option<GitHubRepository>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
#[allow(dead_code)]
struct GitHubRepository {
    full_name: String,
    owner: GitHubUser,
    name: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct GitHubUser {
    login: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct GitHubLabel {
    name: String,
}

/// GitHub forge implementation.
pub struct GitHubForge {
    client: Client,
    webhook_secret: Option<String>,
    token: Option<String>,
}

impl GitHubForge {
    /// Create from environment variables.
    pub fn from_env(loom_dir: &str) -> Result<Self> {
        // Try to read from ~/.loom/config.toml or env
        let token = env::var("GITHUB_TOKEN").ok()
            .or_else(|| {
                // Could read from loom config
                std::fs::read_to_string(format!("{}/config.toml", loom_dir))
                    .ok()
                    .and_then(|c| toml::from_str::<toml::Value>(&c).ok())
                    .and_then(|v| v.get("github")?.get("token")?.as_str().map(|s| s.to_string()))
            });

        let webhook_secret = env::var("GITHUB_WEBHOOK_SECRET").ok()
            .or_else(|| {
                std::fs::read_to_string(format!("{}/config.toml", loom_dir))
                    .ok()
                    .and_then(|c| toml::from_str::<toml::Value>(&c).ok())
                    .and_then(|v| v.get("github")?.get("webhook_secret")?.as_str().map(|s| s.to_string()))
            });

        Ok(Self {
            client: Client::new(),
            webhook_secret,
            token,
        })
    }

    /// Get the webhook secret.
    pub fn webhook_secret(&self) -> Option<&str> {
        self.webhook_secret.as_deref()
    }
}

#[async_trait]
impl Forge for GitHubForge {
    fn verify_signature(&self, payload: &[u8], signature: &str) -> bool {
        let Some(secret) = &self.webhook_secret else {
            return true; // No secret configured, skip verification
        };

        let expected = signature.strip_prefix("sha256=").unwrap_or(signature);
        let mut mac = Hmac::<Sha256>::new_from_slice(secret.as_bytes()).unwrap();
        mac.update(payload);
        let result = mac.finalize().into_bytes();
        let expected_bytes = hex::decode(expected).unwrap_or_default();
        result.as_slice() == expected_bytes.as_slice()
    }

    fn parse_pr_payload(&self, payload: &[u8]) -> Result<Option<PrInfo>> {
        let gh_payload: GitHubPrPayload = serde_json::from_slice(payload)
            .context("Failed to parse GitHub webhook payload")?;

        // Only handle relevant actions
        if !matches!(gh_payload.action.as_str(), "opened" | "reopened" | "synchronize" | "ready_for_review") {
            return Ok(None);
        }

        let pr = gh_payload.pull_request;
        let repo = gh_payload.repository;
        let owner = repo.owner.login.clone();

        Ok(Some(PrInfo {
            forge: "github".to_string(),
            owner,
            repo: repo.name,
            number: gh_payload.number,
            title: pr.title,
            body: pr.body.unwrap_or_default(),
            head_sha: pr.head.sha,
            base_sha: pr.base.sha,
            head_ref: pr.head.ref_name,
            base_ref: pr.base.ref_name,
            author: gh_payload.sender.login,
            url: pr.url,
            html_url: pr.html_url,
            is_draft: pr.draft,
            labels: pr.labels.into_iter().map(|l| l.name).collect(),
        }))
    }

    async fn post_review(&self, pr: &PrInfo, body: &str) -> Result<()> {
        let token = self.token.as_ref()
            .ok_or_else(|| anyhow::anyhow!("GitHub token not configured"))?;

        let url = format!(
            "https://api.github.com/repos/{}/{}/pulls/{}/reviews",
            pr.owner, pr.repo, pr.number
        );

        let review = serde_json::json!({
            "body": body,
            "event": "COMMENT"
        });

        let resp = self.client
            .post(&url)
            .header("Authorization", format!("Bearer {}", token))
            .header("User-Agent", "Loom")
            .json(&review)
            .send()
            .await
            .context("Failed to send GitHub review request")?;

        if !resp.status().is_success() {
            let err = resp.text().await.unwrap_or_default();
            anyhow::bail!("GitHub review failed: {}", err);
        }

        Ok(())
    }

    fn default_branch(&self) -> &str {
        "main"
    }

    fn name(&self) -> &str {
        "github"
    }
}