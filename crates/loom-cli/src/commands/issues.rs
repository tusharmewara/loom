//! `loom issues` subcommands — track issues on GitHub, Linear, and more.

use crate::cli::IssuesAction;
use loom_trackers::github::GitHubTracker;
use loom_trackers::gitlab::GitLabTracker;
use loom_trackers::jira::JiraTracker;
use loom_trackers::linear::LinearTracker;
use loom_trackers::models::IssueQuery;
use loom_trackers::IssueTracker;
use std::path::Path;

/// Dispatch the issues subcommand.
pub async fn run(_loom_dir: &Path, action: &IssuesAction) -> anyhow::Result<()> {
    match action {
        IssuesAction::List {
            repo,
            state,
            labels,
            json,
            token,
            provider,
            host,
        } => {
            cmd_list(provider, repo, state.as_deref(), labels.as_deref(), *json, token.as_deref(), host.as_deref()).await
        }
        IssuesAction::Show {
            id,
            repo,
            json,
            token,
            provider,
            host,
        } => cmd_show(provider, id, repo, *json, token.as_deref(), host.as_deref()).await,
        IssuesAction::CreateBranch {
            id,
            repo,
            token,
            provider,
            host,
        } => cmd_create_branch(provider, id, repo, token.as_deref(), host.as_deref()).await,
        IssuesAction::LinkPr {
            id,
            pr_url,
            repo,
            token,
            provider,
            host,
        } => cmd_link_pr(provider, id, pr_url, repo, token.as_deref(), host.as_deref()).await,
    }
}

/// Create a tracker instance from the provider name.
fn create_tracker(provider: &str, token: Option<&str>, host: Option<&str>) -> anyhow::Result<Box<dyn IssueTracker>> {
    match provider {
        "github" => Ok(Box::new(GitHubTracker::new(token.map(|s| s.to_string()))?)),
        "gitlab" => Ok(Box::new(GitLabTracker::new(host.map(|s| s.to_string()), token.map(|s| s.to_string()))?)),
        "linear" => Ok(Box::new(LinearTracker::new(token.map(|s| s.to_string()))?)),
        "jira" => Ok(Box::new(JiraTracker::new(
            host.map(|s| s.to_string()),
            None,
            token.map(|s| s.to_string()),
        )?)),
        other => anyhow::bail!("Unknown provider '{other}'. Use 'github', 'gitlab', 'linear', or 'jira'."),
    }
}

async fn cmd_list(
    provider: &str,
    repo: &str,
    state: Option<&str>,
    labels: Option<&str>,
    json: bool,
    token: Option<&str>,
    host: Option<&str>,
) -> anyhow::Result<()> {
    let tracker = create_tracker(provider, token, host)?;

    let query = IssueQuery {
        repo: repo.to_string(),
        state: state.map(|s| s.to_string()),
        labels: labels.map(|l| l.to_string()),
        limit: Some(30),
    };

    let issues = tracker.list_issues(&query).await?;

    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "schema_version": 1,
                "command": "issues.list",
                "provider": provider,
                "repo": repo,
                "count": issues.len(),
                "issues": issues,
            }))?
        );
    } else {
        if issues.is_empty() {
            println!("No issues found for {repo} on {provider} (state={})", state.unwrap_or("open"));
            return Ok(());
        }
        println!("Issues for {} on {} (state={}):", repo, provider, state.unwrap_or("open"));
        for issue in &issues {
            let labels_str = if issue.labels.is_empty() {
                String::new()
            } else {
                format!(" [{}]", issue.labels.join(", "))
            };
            println!(
                "  {}  {}{}  {}",
                issue.id,
                if issue.state == "open" { "🔴" } else { "🟢" },
                labels_str,
                issue.title,
            );
        }
    }
    Ok(())
}

async fn cmd_show(
    provider: &str,
    id: &str,
    repo: &str,
    json: bool,
    token: Option<&str>,
    host: Option<&str>,
) -> anyhow::Result<()> {
    let tracker = create_tracker(provider, token, host)?;

    // For GitHub, if id is just a number, prepend the repo
    let issue_id = if provider == "github" && id.chars().all(|c| c.is_ascii_digit()) && !repo.is_empty() {
        format!("{}#{}", repo, id)
    } else {
        id.to_string()
    };

    let issue = tracker.get_issue(&issue_id).await?;

    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "schema_version": 1,
                "command": "issues.show",
                "provider": provider,
                "issue": issue,
            }))?
        );
    } else {
        println!("Issue: {} ({})", issue.id, provider);
        println!("Title: {}", issue.title);
        println!("State: {}", issue.state);
        if let Some(body) = &issue.body {
            for line in body.lines().take(20) {
                println!("  {}", line);
            }
            if body.lines().count() > 20 {
                println!("  ... ({} more lines)", body.lines().count() - 20);
            }
        }
        if !issue.labels.is_empty() {
            println!("Labels: {}", issue.labels.join(", "));
        }
        if let Some(assignee) = &issue.assignee {
            println!("Assignee: {}", assignee);
        }
        if let Some(url) = &issue.html_url {
            println!("URL: {}", url);
        }
    }
    Ok(())
}

async fn cmd_create_branch(
    provider: &str,
    id: &str,
    repo: &str,
    token: Option<&str>,
    host: Option<&str>,
) -> anyhow::Result<()> {
    let tracker = create_tracker(provider, token, host)?;

    let issue_id = if provider == "github" && id.chars().all(|c| c.is_ascii_digit()) && !repo.is_empty() {
        format!("{}#{}", repo, id)
    } else {
        id.to_string()
    };

    let issue = tracker.get_issue(&issue_id).await?;
    let branch_name = tracker
        .suggest_branch_name(&issue)
        .ok_or_else(|| anyhow::anyhow!("Could not generate branch name for issue"))?;

    let output = std::process::Command::new("git")
        .args(["checkout", "-b", &branch_name])
        .output()
        .map_err(|e| anyhow::anyhow!("Failed to run git: {e}"))?;

    if output.status.success() {
        println!("✓ Created branch: {}", branch_name);
        println!("  Issue: {} — {} ({})", issue.id, issue.title, provider);
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("Failed to create branch: {}", stderr.trim());
    }

    Ok(())
}

async fn cmd_link_pr(
    provider: &str,
    id: &str,
    pr_url: &str,
    repo: &str,
    token: Option<&str>,
    host: Option<&str>,
) -> anyhow::Result<()> {
    let tracker = create_tracker(provider, token, host)?;

    let issue_id = if provider == "github" && id.chars().all(|c| c.is_ascii_digit()) && !repo.is_empty() {
        format!("{}#{}", repo, id)
    } else {
        id.to_string()
    };

    tracker.link_pr(&issue_id, pr_url).await?;
    println!("✓ Linked PR to {provider} issue {issue_id}: {pr_url}");
    Ok(())
}
