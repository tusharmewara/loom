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
        IssuesAction::Diff { pr, repo, token } => {
            cmd_diff(*pr, repo, token.as_deref()).await
        }
        IssuesAction::CiStatus { pr, repo, token, json } => {
            cmd_ci_status(*pr, repo, token.as_deref(), *json).await
        }
        IssuesAction::PullRequests { repo, state, json, token } => {
            cmd_pull_requests(repo, state, *json, token.as_deref()).await
        }
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

async fn cmd_diff(pr: u64, repo: &str, token: Option<&str>) -> anyhow::Result<()> {
    let tracker = GitHubTracker::new(token.map(|s| s.to_string()))?;
    let diff = tracker.fetch_pr_diff(repo, pr).await?;

    // Show diff with line numbers
    for line in diff.lines() {
        let prefix = if line.starts_with('+') && !line.starts_with("+++") {
            "\x1b[32m+\x1b[0m"
        } else if line.starts_with('-') && !line.starts_with("---") {
            "\x1b[31m-\x1b[0m"
        } else if line.starts_with("@@") {
            "\x1b[36m@@\x1b[0m"
        } else {
            " "
        };
        println!("{} {}", prefix, line);
    }
    Ok(())
}

async fn cmd_ci_status(pr: u64, repo: &str, token: Option<&str>, json: bool) -> anyhow::Result<()> {
    let tracker = GitHubTracker::new(token.map(|s| s.to_string()))?;
    let statuses = tracker.fetch_pr_ci_status(repo, pr).await?;

    if json {
        println!("{}", serde_json::to_string_pretty(&serde_json::json!({
            "schema_version": 1,
            "command": "issues.ci-status",
            "repo": repo,
            "pr": pr,
            "count": statuses.len(),
            "checks": statuses,
        }))?);
    } else {
        if statuses.is_empty() {
            println!("No CI checks found for PR #{pr} in {repo}");
            return Ok(());
        }
        println!("CI Status for {repo}#{pr}:");
        for c in &statuses {
            let icon = match c.status.as_str() {
                "success" => "✅",
                "failure" => "❌",
                "cancelled" => "⏹️",
                "pending" | "queued" | "in_progress" => "⏳",
                "neutral" => "➖",
                _ => "❓",
            };
            if let Some(ref url) = c.url {
                println!("  {} {}  ({})", icon, c.name, url);
            } else {
                println!("  {} {}", icon, c.name);
            }
        }
    }
    Ok(())
}

async fn cmd_pull_requests(repo: &str, state: &str, json: bool, token: Option<&str>) -> anyhow::Result<()> {
    let tracker = GitHubTracker::new(token.map(|s| s.to_string()))?;
    let prs = tracker.list_pull_requests(repo, state).await?;

    if json {
        println!("{}", serde_json::to_string_pretty(&serde_json::json!({
            "schema_version": 1,
            "command": "issues.pull-requests",
            "repo": repo,
            "state": state,
            "count": prs.len(),
            "issues": prs,
        }))?);
    } else {
        if prs.is_empty() {
            println!("No pull requests found for {repo} (state={state})");
            return Ok(());
        }
        println!("Pull requests for {repo} (state={state}):");
        for pr in &prs {
            println!("  {}  {}  {}", pr.id, pr.state, pr.title);
        }
    }
    Ok(())
}
