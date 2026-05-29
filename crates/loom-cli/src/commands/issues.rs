//! `loom issues` subcommands — track issues on GitHub and other providers.

use crate::cli::IssuesAction;
use loom_trackers::github::GitHubTracker;
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
        } => cmd_list(repo, state.as_deref(), labels.as_deref(), *json, token.as_deref()).await,
        IssuesAction::Show {
            id,
            repo,
            json,
            token,
        } => cmd_show(id, repo, *json, token.as_deref()).await,
        IssuesAction::CreateBranch {
            id,
            repo,
            token,
        } => cmd_create_branch(id, repo, token.as_deref()).await,
    }
}

async fn cmd_list(
    repo: &str,
    state: Option<&str>,
    labels: Option<&str>,
    json: bool,
    token: Option<&str>,
) -> anyhow::Result<()> {
    let tracker = GitHubTracker::new(token.map(|s| s.to_string()))?;

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
                "repo": repo,
                "count": issues.len(),
                "issues": issues,
            }))?
        );
    } else {
        if issues.is_empty() {
            println!("No issues found for {repo} (state={})", state.unwrap_or("open"));
            return Ok(());
        }
        println!("Issues for {} (state={}):", repo, state.unwrap_or("open"));
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

async fn cmd_show(id: &str, repo: &str, json: bool, token: Option<&str>) -> anyhow::Result<()> {
    let tracker = GitHubTracker::new(token.map(|s| s.to_string()))?;

    // If id is just a number, prepend the repo
    let issue_id = if id.chars().all(|c| c.is_ascii_digit()) && !repo.is_empty() {
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
                "issue": issue,
            }))?
        );
    } else {
        println!("Issue: {}", issue.id);
        println!("Title: {}", issue.title);
        println!("State: {}", issue.state);
        if let Some(body) = &issue.body {
            // Print first 20 lines of body
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

async fn cmd_create_branch(id: &str, repo: &str, token: Option<&str>) -> anyhow::Result<()> {
    let tracker = GitHubTracker::new(token.map(|s| s.to_string()))?;

    let issue_id = if id.chars().all(|c| c.is_ascii_digit()) && !repo.is_empty() {
        format!("{}#{}", repo, id)
    } else {
        id.to_string()
    };

    let issue = tracker.get_issue(&issue_id).await?;
    let branch_name = tracker
        .suggest_branch_name(&issue)
        .ok_or_else(|| anyhow::anyhow!("Could not generate branch name for issue"))?;

    // Create the branch locally (do NOT push, just create from HEAD)
    let output = std::process::Command::new("git")
        .args(["checkout", "-b", &branch_name])
        .output()
        .map_err(|e| anyhow::anyhow!("Failed to run git: {e}"))?;

    if output.status.success() {
        println!("✓ Created branch: {}", branch_name);
        println!("  Issue: {} — {}", issue.id, issue.title);
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("Failed to create branch: {}", stderr.trim());
    }

    Ok(())
}
