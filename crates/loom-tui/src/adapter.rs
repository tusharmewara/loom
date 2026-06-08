use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::io::AsyncBufReadExt;
use tokio::process::{Child, ChildStdin, Command};
use tokio::sync::mpsc;

#[derive(Clone)]
pub struct LoomCliAdapter {
    bin: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CommandResult {
    pub action: String,
    pub success: bool,
    pub message: String,
}

impl LoomCliAdapter {
    pub fn new(bin: impl Into<String>) -> Self {
        Self { bin: bin.into() }
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

    pub async fn fetch_hooks_list(&self) -> Result<Value> {
        self.run_json(&["hooks", "list"]).await
    }

    pub async fn fetch_knowledge_stats(&self) -> Result<Value> {
        self.run_json(&["knowledge", "stats"]).await
    }

    pub async fn fetch_pulls_list(&self) -> Result<Value> {
        self.run_json(&["issues", "list", "--json", "--state", "all", "--repo", ""]).await
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
