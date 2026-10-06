use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::io::AsyncBufReadExt;
use tokio::process::{Child, ChildStdin, Command};
use tokio::sync::mpsc;

#[derive(Clone)]
pub struct LoomCliAdapter {
    bin: String,
    pub loom_dir: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CommandResult {
    pub action: String,
    pub success: bool,
    pub message: String,
}

impl LoomCliAdapter {
    pub fn new(bin: impl Into<String>, loom_dir: impl Into<String>) -> Self {
        Self {
            bin: bin.into(),
            loom_dir: loom_dir.into(),
        }
    }

    pub async fn fetch_status(&self) -> Result<Value> {
        self.run_json(&["status"]).await
    }

    pub async fn fetch_scan(&self) -> Result<Value> {
        self.run_json(&["scan"]).await
    }

    pub async fn fetch_doctor(&self) -> Result<Value> {
        self.run_json(&["doctor"]).await
    }

    pub async fn fetch_daemon_status(&self) -> Result<Value> {
        self.run_json(&["daemon", "status"]).await
    }

    pub async fn fetch_mcp_list(&self) -> Result<Value> {
        self.run_json(&["mcp", "list"]).await
    }

    pub async fn fetch_skills_list(&self) -> Result<Value> {
        self.run_json(&["skills", "list"]).await
    }

    pub async fn fetch_sessions_list(&self) -> Result<Value> {
        self.run_json(&["sessions", "list"]).await
    }

    pub async fn fetch_workflow_list(&self) -> Result<Value> {
        self.run_json(&["workflow", "list"]).await
    }

    /// Fetch one full workflow task (`workflow show` payload).
    pub async fn fetch_workflow_show(&self, task_id: &str) -> Result<Value> {
        self.run_json(&["workflow", "show", task_id]).await
    }

    /// Draft a task plan from title/details (`draft-plan --json`).
    pub async fn draft_plan(&self, title: &str, description: &str) -> Result<Value> {
        self.run_json(&[
            "workflow",
            "draft-plan",
            "--title",
            title,
            "--description",
            description,
        ])
        .await
    }

    /// Draft a plan and import it as tasks; returns created task ids.
    pub async fn draft_plan_create(
        &self,
        title: &str,
        description: &str,
    ) -> Result<Vec<String>> {
        let value = self
            .run_json(&[
                "workflow",
                "draft-plan",
                "--title",
                title,
                "--description",
                description,
                "--create",
            ])
            .await?;
        Ok(value
            .get("created")
            .and_then(Value::as_array)
            .map(|arr| {
                arr.iter()
                    .filter_map(Value::as_str)
                    .map(str::to_string)
                    .collect()
            })
            .unwrap_or_default())
    }

    /// Remove a workflow task. Output is human text; the exit code is
    /// the contract.
    pub async fn remove_task(&self, task_id: &str) -> Result<()> {
        self.run_text(vec![
            "workflow".to_string(),
            "remove".to_string(),
            task_id.to_string(),
        ])
        .await
        .map(|_| ())
    }

    /// Persist the agent assignment for a task. `workflow assign` has
    /// no JSON mode, so success is the process exit code (via run_text).
    pub async fn assign_task_agent(&self, task_id: &str, agent: &str) -> Result<()> {
        self.run_text(vec![
            "workflow".to_string(),
            "assign".to_string(),
            task_id.to_string(),
            "--agent".to_string(),
            agent.to_string(),
        ])
        .await
        .map(|_| ())
    }

    /// Fetch launchable ACP agents: daemon-registered handlers plus
    /// `config/agents.toml` entries (what `loom add-acp-agent` writes).
    pub async fn fetch_acp_agents(&self) -> Result<Value> {
        self.run_json(&["acp", "agents"]).await
    }

    /// Fetch a tracked ACP run's status from the daemon.
    pub async fn fetch_run_status(&self, run_id: &str) -> Result<Value> {
        self.run_json(&["acp", "run-status", "--run-id", run_id]).await
    }

    /// Create a pull request from the current branch (`gh pr create
    /// --fill`). Returns the PR URL.
    pub async fn create_pull_request(&self, repo: &str) -> Result<String> {
        let value = self.run_json(&["issues", "create-pr", "--repo", repo]).await?;
        value
            .get("url")
            .and_then(Value::as_str)
            .map(str::to_string)
            .ok_or_else(|| {
                anyhow::anyhow!("`loom issues create-pr` returned no PR URL")
            })
    }

    /// Create a workflow task from Orchestrator title/details input.
    ///
    /// `loom workflow create` prints a human-readable confirmation before
    /// the task payload, so the created task ID is extracted with
    /// [`parse_created_task_id`] instead of parsing stdout as pure JSON.
    pub async fn create_workflow_task(&self, title: &str, description: &str) -> Result<String> {
        let stdout = self
            .run_text(vec![
                "workflow".to_string(),
                "create".to_string(),
                "--title".to_string(),
                title.to_string(),
                "--description".to_string(),
                description.to_string(),
            ])
            .await?;
        parse_created_task_id(&stdout).ok_or_else(|| {
            anyhow::anyhow!("could not parse created task ID from `loom workflow create` output")
        })
    }

    pub async fn fetch_hooks_list(&self) -> Result<Value> {
        self.run_json(&["hooks", "list"]).await
    }

    pub async fn fetch_config(&self) -> Result<Value> {
        self.run_json(&["config", "show", "--json"]).await
    }

    pub async fn fetch_knowledge_stats(&self) -> Result<Value> {
        self.run_json(&["knowledge", "stats"]).await
    }

    pub async fn fetch_pulls_list(&self) -> Result<Value> {
        self.run_json(&["issues", "list", "--json", "--state", "all", "--repo", ""]).await
    }

    pub async fn fetch_pull_requests(&self, repo: &str, state: &str) -> Result<Value> {
        self.run_json(&["issues", "pull-requests", "--json", "--repo", repo, "--state", state]).await
    }

    pub async fn fetch_pr_ci_status(&self, repo: &str, pr: u64) -> Result<Value> {
        self.run_json(&["issues", "ci-status", "--json", "--repo", repo, "--pr", &pr.to_string()]).await
    }

    pub async fn fetch_pr_diff(&self, repo: &str, pr: u64) -> Result<String> {
        let output = Command::new(&self.bin)
            .args(["issues", "diff", "--repo", repo, "--pr", &pr.to_string()])
            .output()
            .await
            .with_context(|| format!("failed to execute `{}`", self.bin))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
            anyhow::bail!("diff failed: {}", stderr);
        }

        Ok(String::from_utf8_lossy(&output.stdout).to_string())
    }

    /// Detect the current git repository (as `owner/repo`) from the working
    /// directory's `origin` remote, so the PR/CI/diff view can target a real
    /// repository. Returns `None` when not inside a git checkout or when no
    /// `origin` remote is configured.
    pub async fn fetch_current_repo(&self) -> Result<Option<String>> {
        let output = Command::new("git")
            .args(["config", "--get", "remote.origin.url"])
            .output()
            .await
            .with_context(|| "failed to run `git`")?;

        if !output.status.success() {
            return Ok(None);
        }

        let url = String::from_utf8_lossy(&output.stdout).trim().to_string();
        Ok(normalize_repo(&url))
    }

    pub async fn daemon_start(&self) -> CommandResult {
        self.run_daemon_action("start", &["daemon", "start"]).await
    }

    pub async fn daemon_stop(&self) -> CommandResult {
        self.run_daemon_action("stop", &["daemon", "stop"]).await
    }

    pub async fn daemon_restart(&self) -> CommandResult {
        self.run_daemon_action("restart", &["daemon", "restart"]).await
    }

    pub async fn daemon_install_service(&self) -> CommandResult {
        self.run_daemon_action("install-service", &["daemon", "install-service"]).await
    }

    pub async fn daemon_uninstall_service(&self) -> CommandResult {
        self.run_daemon_action("uninstall-service", &["daemon", "uninstall-service"]).await
    }

    pub async fn run_scan(&self) -> CommandResult {
        match self.fetch_scan().await {
            Ok(_) => CommandResult {
                action: "scan".to_string(),
                success: true,
                message: "scan completed".to_string(),
            },
            Err(e) => CommandResult {
                action: "scan".to_string(),
                success: false,
                message: e.to_string(),
            },
        }
    }

    pub async fn run_doctor(&self) -> CommandResult {
        match self.fetch_doctor().await {
            Ok(_) => CommandResult {
                action: "doctor".to_string(),
                success: true,
                message: "doctor checks passed".to_string(),
            },
            Err(e) => CommandResult {
                action: "doctor".to_string(),
                success: false,
                message: e.to_string(),
            },
        }
    }

    async fn run_daemon_action(&self, label: &str, args: &[&str]) -> CommandResult {
        match self.run_json(args).await {
            Ok(_) => CommandResult {
                action: label.to_string(),
                success: true,
                message: format!("daemon {label} completed"),
            },
            Err(e) => CommandResult {
                action: label.to_string(),
                success: false,
                message: e.to_string(),
            },
        }
    }

    async fn run_json(&self, args: &[&str]) -> Result<Value> {
        let output = Command::new(&self.bin)
            .args(args)
            .arg("--json")
            .output()
            .await
            .with_context(|| format!("failed to execute `{}`", self.bin))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
            anyhow::bail!(
                "`{} {}` failed: {}",
                self.bin,
                args.join(" "),
                if stderr.is_empty() {
                    "unknown error".to_string()
                } else {
                    stderr
                }
            );
        }

        let stdout = String::from_utf8_lossy(&output.stdout).to_string();
        let value: Value = serde_json::from_str(&stdout)
            .with_context(|| format!("invalid JSON from `{}`", self.bin))?;
        Ok(value)
    }

    /// Run a CLI command and return its stdout as text. Unlike
    /// [`Self::run_json`], no `--json` flag is appended, for commands
    /// whose output mixes human-readable lines with a JSON payload.
    async fn run_text(&self, args: Vec<String>) -> Result<String> {
        let output = Command::new(&self.bin)
            .args(&args)
            .output()
            .await
            .with_context(|| format!("failed to execute `{}`", self.bin))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
            anyhow::bail!(
                "`{} {}` failed: {}",
                self.bin,
                args.join(" "),
                if stderr.is_empty() {
                    "unknown error".to_string()
                } else {
                    stderr
                }
            );
        }

        Ok(String::from_utf8_lossy(&output.stdout).to_string())
    }

    /// Run a CLI command and stream its stdout lines through a channel.
    /// Returns the child process handle and a receiver for stdout lines.
    /// Callers must await the child process to completion after draining
    /// the receiver.
    pub fn stream_command(
        &self,
        args: &[&str],
    ) -> Result<(Child, mpsc::Receiver<String>, Option<ChildStdin>)> {
        let mut child = Command::new(&self.bin)
            .args(args)
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .stdin(std::process::Stdio::piped())
            .spawn()
            .with_context(|| format!("failed to spawn `{}`", self.bin))?;

        let stdin = child
            .stdin
            .take();

        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| anyhow::anyhow!("failed to capture stdout"))?;

        let (tx, rx) = mpsc::channel::<String>(256);

        tokio::spawn(async move {
            let reader = tokio::io::BufReader::new(stdout);
            let mut lines = reader.lines();
            while let Ok(Some(line)) = lines.next_line().await {
                if tx.send(line).await.is_err() {
                    break;
                }
            }
        });

        Ok((child, rx, stdin))
    }

    /// Spawn an agent tool for a task and return a receiver for stdout lines.
    pub fn stream_background(&self, agent: &str, task_id: &str) -> (Option<Child>, mpsc::Receiver<String>, Option<ChildStdin>) {
        match self.stream_command(&["workflow", "dispatch", task_id, "--agent", agent]) {
            Ok((child, rx, stdin)) => (Some(child), rx, stdin),
            Err(_) => {
                let (tx, rx) = mpsc::channel(1);
                let _ = tx.try_send(format!("Failed to spawn {} for task {}", agent, task_id));
                (None, rx, None)
            }
        }
    }
}

/// Extract the created task ID from `loom workflow create` output.
///
/// The command prints a human-readable confirmation line before the
/// task payload, so three shapes are accepted, in order:
/// 1. a `Created task <id>` confirmation line (authoritative),
/// 2. an embedded JSON payload containing `task_id`,
/// 3. any bare UUID-shaped token (8-4-4-4-12 hex).
/// No `regex` dependency: the UUID scan is a small manual matcher.
fn parse_created_task_id(output: &str) -> Option<String> {
    for line in output.lines().map(str::trim) {
        if let Some(rest) = line.strip_prefix("Created task ") {
            if let Some(id) = rest.split_whitespace().next().filter(|s| !s.is_empty()) {
                return Some(id.to_string());
            }
        }
    }
    if let Some(start) = output.find('{') {
        if let Ok(value) = serde_json::from_str::<Value>(&output[start..]) {
            if let Some(id) = value
                .get("task_id")
                .and_then(Value::as_str)
                .filter(|s| !s.is_empty())
            {
                return Some(id.to_string());
            }
        }
    }
    output.split_whitespace().find_map(|token| {
        let token = token.trim_matches(|c: char| !c.is_ascii_hexdigit() && c != '-');
        is_uuid_shaped(token).then(|| token.to_string())
    })
}

fn is_uuid_shaped(token: &str) -> bool {
    let parts: Vec<&str> = token.split('-').collect();
    match parts.as_slice() {
        [a, b, c, d, e] => {
            a.len() == 8
                && b.len() == 4
                && c.len() == 4
                && d.len() == 4
                && e.len() == 12
                && token.chars().all(|c| c.is_ascii_hexdigit() || c == '-')
        }
        _ => false,
    }
}

/// Normalize a git remote URL into `owner/repo` form.
///
/// Handles `https://github.com/owner/repo(.git)`, `git@github.com:owner/repo.git`,
/// `ssh://git@github.com/owner/repo.git`, and `git://github.com/owner/repo.git`.
/// Anything that does not resolve to an `owner/repo` shape returns `None`.
fn normalize_repo(url: &str) -> Option<String> {
    let url = url.trim().trim_end_matches(".git");
    if url.is_empty() {
        return None;
    }

    // URL-style: scheme://host/owner/repo
    if let Some(rest) = url
        .strip_prefix("https://")
        .or_else(|| url.strip_prefix("http://"))
        .or_else(|| url.strip_prefix("ssh://"))
        .or_else(|| url.strip_prefix("git://"))
    {
        let path = rest.split_once('/')?.1;
        let mut parts = path.split('/');
        let owner = parts.next()?;
        let repo = parts.next()?;
        if owner.is_empty() || repo.is_empty() || parts.next().is_some() {
            return None;
        }
        return Some(format!("{owner}/{repo}"));
    }

    // SCP-style: git@github.com:owner/repo
    if let Some(path) = url.split_once(':') {
        let mut parts = path.1.split('/');
        let owner = parts.next()?;
        let repo = parts.next()?;
        if owner.is_empty() || repo.is_empty() || parts.next().is_some() {
            return None;
        }
        return Some(format!("{owner}/{repo}"));
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_normalize_repo_https() {
        assert_eq!(
            normalize_repo("https://github.com/owner/repo.git").as_deref(),
            Some("owner/repo")
        );
    }

    #[test]
    fn test_normalize_repo_scp() {
        assert_eq!(
            normalize_repo("git@github.com:owner/repo.git").as_deref(),
            Some("owner/repo")
        );
    }

    #[test]
    fn test_normalize_repo_ssh() {
        assert_eq!(
            normalize_repo("ssh://git@github.com/owner/repo.git").as_deref(),
            Some("owner/repo")
        );
    }

    #[test]
    fn test_normalize_repo_invalid() {
        assert_eq!(normalize_repo(""), None);
        assert_eq!(normalize_repo("https://github.com"), None);
        assert_eq!(normalize_repo("not a url"), None);
    }

    #[test]
    fn test_parse_created_task_id_prefix_line() {
        let output = "Created task 123e4567-e89b-12d3-a456-426614174000\n";
        assert_eq!(
            parse_created_task_id(output).as_deref(),
            Some("123e4567-e89b-12d3-a456-426614174000")
        );
    }

    #[test]
    fn test_parse_created_task_id_prefers_prefix_over_payload() {
        let output = "Created task 123e4567-e89b-12d3-a456-426614174000\n{\"task_id\": \"ignored\"}";
        assert_eq!(
            parse_created_task_id(output).as_deref(),
            Some("123e4567-e89b-12d3-a456-426614174000")
        );
    }

    #[test]
    fn test_parse_created_task_id_json_only() {
        let output = "{\"task_id\": \"123e4567-e89b-12d3-a456-426614174000\", \"title\": \"x\"}";
        assert_eq!(
            parse_created_task_id(output).as_deref(),
            Some("123e4567-e89b-12d3-a456-426614174000")
        );
    }

    #[test]
    fn test_parse_created_task_id_bare_uuid() {
        let output = "task ready: 123e4567-e89b-12d3-a456-426614174000 done";
        assert_eq!(
            parse_created_task_id(output).as_deref(),
            Some("123e4567-e89b-12d3-a456-426614174000")
        );
    }

    #[test]
    fn test_parse_created_task_id_none() {
        assert_eq!(parse_created_task_id("no task here"), None);
        assert_eq!(parse_created_task_id("Created task "), None);
        assert_eq!(parse_created_task_id(""), None);
    }
}
