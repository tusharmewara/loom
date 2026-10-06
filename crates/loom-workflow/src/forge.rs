//! Forge integration for GitHub and GitLab webhooks.
//!
//! Handles webhook events from GitHub/GitLab to create workflow tasks
//! and post reviews back to the forge.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Mutex;

pub mod github;
pub mod gitlab;

use github::GitHubForge;
use gitlab::GitLabForge;

use crate::task_manager::TaskManager;

/// Forge trait for abstracting GitHub/GitLab operations.
#[async_trait::async_trait]
pub trait Forge: Send + Sync {
    /// Verify webhook signature.
    fn verify_signature(&self, payload: &[u8], signature: &str) -> bool;

    /// Parse webhook payload to extract PR info.
    fn parse_pr_payload(&self, payload: &[u8]) -> Result<Option<PrInfo>>;

    /// Post a review comment on a PR.
    async fn post_review(&self, pr: &PrInfo, body: &str) -> Result<()>;

    /// Get the default branch name.
    fn default_branch(&self) -> &str;

    /// Get the forge name (github/gitlab).
    fn name(&self) -> &str;
}

/// Pull request information extracted from webhook.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PrInfo {
    pub forge: String,           // "github" or "gitlab"
    pub owner: String,           // repo owner/org
    pub repo: String,            // repo name
    pub number: u64,             // PR number
    pub title: String,           // PR title
    pub body: String,            // PR description
    pub head_sha: String,        // commit SHA at head
    pub base_sha: String,        // commit SHA at base
    pub head_ref: String,        // source branch
    pub base_ref: String,        // target branch
    pub author: String,          // PR author
    pub url: String,             // PR URL
    pub html_url: String,        // PR web URL
    pub is_draft: bool,          // draft PR
    pub labels: Vec<String>,     // PR labels
}

/// Forge manager that handles multiple forges.
pub struct ForgeManager {
    forges: HashMap<String, Box<dyn Forge>>,
    task_manager: Arc<Mutex<TaskManager>>,
    loom_dir: String,
}

impl ForgeManager {
    /// Create a new forge manager.
    pub fn new(loom_dir: &str, task_manager: TaskManager) -> Result<Self> {
        let mut forges: HashMap<String, Box<dyn Forge>> = HashMap::new();

        // Initialize GitHub forge if configured
        if let Ok(github) = GitHubForge::from_env(loom_dir) {
            forges.insert("github".to_string(), Box::new(github));
        }

        // Initialize GitLab forge if configured
        if let Ok(gitlab) = GitLabForge::from_env(loom_dir) {
            forges.insert("gitlab".to_string(), Box::new(gitlab));
        }

        Ok(Self {
            forges,
            task_manager: Arc::new(Mutex::new(task_manager)),
            loom_dir: loom_dir.to_string(),
        })
    }

    /// Get a forge by name.
    pub fn get_forge(&self, name: &str) -> Option<&dyn Forge> {
        self.forges.get(name).map(|b| b.as_ref())
    }

    /// Handle incoming webhook from a forge.
    pub async fn handle_webhook(
        &self,
        forge_name: &str,
        payload: &[u8],
        signature: Option<&str>,
    ) -> Result<WebhookResult> {
        let forge = self.get_forge(forge_name)
            .ok_or_else(|| anyhow::anyhow!("Unknown forge: {}", forge_name))?;

        // Verify signature if provided
        if let Some(sig) = signature {
            if !forge.verify_signature(payload, sig) {
                return Ok(WebhookResult {
                    success: false,
                    message: "Invalid webhook signature".to_string(),
                    task_id: None,
                });
            }
        }

        // Parse PR info
        let pr_info = match forge.parse_pr_payload(payload)? {
            Some(info) => info,
            None => return Ok(WebhookResult {
                success: true,
                message: "No actionable PR event".to_string(),
                task_id: None,
            }),
        };

        // Only process opened/reopened/synchronize events for non-draft PRs
        if pr_info.is_draft {
            return Ok(WebhookResult {
                success: true,
                message: "Draft PR ignored".to_string(),
                task_id: None,
            });
        }

        // Create or update task
        let task_manager = self.task_manager.lock().await;
        let title = format!("PR #{}: {}", pr_info.number, pr_info.title);
        let description = format!(
            "Automated task from {} PR #{} ({})\n\n{}",
            forge.name(),
            pr_info.number,
            pr_info.url,
            pr_info.body
        );

        let task = task_manager
            .create_task(&title, &description, true, None)
            .context("Failed to create task")?;

        // Store PR linkage
        let linked_issue = format!("{}/{}/#{}/{}", pr_info.forge, pr_info.owner, pr_info.repo, pr_info.number);
        task_manager.set_linked_issue(&task.task_id, &linked_issue)?;

        // Index PR entities in knowledge graph
        if let Err(e) = self.index_pr_entities(&task.task_id, &pr_info).await {
            tracing::warn!("Failed to index PR entities: {}", e);
        }

        Ok(WebhookResult {
            success: true,
            message: format!("Created task {} for PR #{}/{}", task.task_id, forge.name(), pr_info.number),
            task_id: Some(task.task_id),
        })
    }

    /// Post a review to the PR.
    pub async fn post_review(&self, pr_info: &PrInfo, body: &str) -> Result<()> {
        let forge = self.get_forge(&pr_info.forge)
            .ok_or_else(|| anyhow::anyhow!("Unknown forge: {}", pr_info.forge))?;
        forge.post_review(pr_info, body).await
    }

    /// Index PR entities in knowledge graph.
    async fn index_pr_entities(&self, task_id: &str, pr_info: &PrInfo) -> Result<()> {
        // Knowledge graph indexing - optional, graceful if not available
        // This would be implemented when loom-knowledge is available
        let _ = task_id;
        let _ = pr_info;
        Ok(())
    }
}

/// Result of webhook processing.
#[derive(Debug, Serialize, Deserialize)]
pub struct WebhookResult {
    pub success: bool,
    pub message: String,
    pub task_id: Option<String>,
}

/// Initialize forge integrations from config.
pub async fn initialize_forges(loom_dir: &str, task_manager: TaskManager) -> Result<ForgeManager> {
    ForgeManager::new(loom_dir, task_manager)
}