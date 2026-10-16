//! Concurrent agent pool — runs multiple tool processes with bounded concurrency.

use std::collections::HashMap;
use std::path::PathBuf;
use std::process::{Command, ExitStatus, Stdio};
use std::time::{Duration, Instant};

/// A single agent task to be executed by the pool.
#[derive(Debug, Clone)]
pub struct AgentTask {
    /// Human-readable name (e.g. "opencode", "goose").
    pub name: String,
    /// Path to the binary.
    pub binary_path: PathBuf,
    /// Arguments to pass.
    pub args: Vec<String>,
    /// Environment variables overlay.
    pub env_vars: HashMap<String, String>,
    /// Optional working directory (e.g. a git worktree).
    pub work_dir: Option<PathBuf>,
}

impl AgentTask {
    /// Create a new task with the given name and binary path.
    pub fn new(name: impl Into<String>, binary_path: impl Into<PathBuf>) -> Self {
        Self {
            name: name.into(),
            binary_path: binary_path.into(),
            args: Vec::new(),
            env_vars: HashMap::new(),
            work_dir: None,
        }
    }

    /// Set the arguments.
    pub fn args(mut self, args: Vec<String>) -> Self {
        self.args = args;
        self
    }

    /// Add a single environment variable.
    pub fn env(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.env_vars.insert(key.into(), value.into());
        self
    }

    /// Add multiple environment variables.
    pub fn envs(mut self, vars: &HashMap<String, String>) -> Self {
        for (k, v) in vars {
            self.env_vars.insert(k.clone(), v.clone());
        }
        self
    }

    /// Set the working directory.
    pub fn work_dir(mut self, path: impl Into<PathBuf>) -> Self {
        self.work_dir = Some(path.into());
        self
    }
}

/// The result of executing a single agent task.
#[derive(Debug, Clone)]
pub struct AgentResult {
    pub name: String,
    pub exit_status: Option<ExitStatus>,
    pub success: bool,
    pub duration: Duration,
    pub error: Option<String>,
}

impl AgentResult {
    pub fn success(&self) -> bool {
        self.success
    }
}

/// A pool that runs agent tools concurrently, up to a configurable maximum.
pub struct AgentPool {
    max_concurrent: usize,
}

impl AgentPool {
    /// Create a new pool with the given concurrency limit.
    ///
    /// `max_concurrent` is clamped to at least 1.
    pub fn new(max_concurrent: usize) -> Self {
        Self {
            max_concurrent: max_concurrent.max(1),
        }
    }

    /// Return the concurrency limit.
    pub fn max_concurrent(&self) -> usize {
        self.max_concurrent
    }

    /// Execute all tasks concurrently, respecting `max_concurrent`.
    ///
    /// Tasks are started immediately up to the concurrency limit; subsequent
    /// tasks wait until a slot opens.
    pub async fn execute_all(&self, tasks: Vec<AgentTask>) -> Vec<AgentResult> {
        let semaphore = Arc::new(tokio::sync::Semaphore::new(self.max_concurrent));
        let mut handles = Vec::with_capacity(tasks.len());

        for task in tasks {
            let permit = semaphore
                .clone()
                .acquire_owned()
                .await
                .expect("semaphore closed");
            handles.push(tokio::spawn(async move {
                let _permit = permit; // held for the task duration
                run_task(task).await
            }));
        }

        let mut results = Vec::with_capacity(handles.len());
        for handle in handles {
            match handle.await {
                Ok(result) => results.push(result),
                Err(e) => results.push(AgentResult {
                    name: "(panic)".into(),
                    exit_status: None,
                    success: false,
                    duration: Duration::ZERO,
                    error: Some(format!("Task panicked: {e}")),
                }),
            }
        }
        results
    }
}

use std::sync::Arc;

async fn run_task(task: AgentTask) -> AgentResult {
    let start = Instant::now();
    let name = task.name.clone();

    let mut cmd = Command::new(&task.binary_path);
    cmd.args(&task.args);
    cmd.envs(&task.env_vars);
    cmd.stdout(Stdio::inherit());
    cmd.stderr(Stdio::inherit());

    if let Some(ref work_dir) = task.work_dir {
        cmd.current_dir(work_dir);
    }

    // Spawn and wait in a blocking task (std::process::Command is blocking)
    let result = tokio::task::spawn_blocking(move || {
        let mut child = match cmd.spawn() {
            Ok(c) => c,
            Err(e) => {
                return AgentResult {
                    name,
                    exit_status: None,
                    success: false,
                    duration: start.elapsed(),
                    error: Some(format!("Failed to spawn: {e}")),
                };
            }
        };

        let exit_status = child.wait().ok();
        let success = exit_status.as_ref().map(|s| s.success()).unwrap_or(false);
        let error = exit_status
            .as_ref()
            .filter(|s| !s.success())
            .map(|s| format!("Exited with: {s}"));

        AgentResult {
            name,
            exit_status,
            success,
            duration: start.elapsed(),
            error,
        }
    })
    .await
    .unwrap_or_else(|_| AgentResult {
        name: "(panic)".into(),
        exit_status: None,
        success: false,
        duration: start.elapsed(),
        error: Some("Blocking task panicked".into()),
    });

    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_execute_single_task() {
        let pool = AgentPool::new(1);
        let task = AgentTask::new("echo", "echo").args(vec!["hello".into()]);
        let results = pool.execute_all(vec![task]).await;
        assert_eq!(results.len(), 1);
        assert!(results[0].success);
    }

    #[tokio::test]
    async fn test_execute_multiple_tasks() {
        let pool = AgentPool::new(4);
        let tasks: Vec<_> = (0..3)
            .map(|i| AgentTask::new(format!("echo-{i}"), "echo").args(vec![i.to_string()]))
            .collect();
        let results = pool.execute_all(tasks).await;
        assert_eq!(results.len(), 3);
        for r in &results {
            assert!(r.success, "{} failed", r.name);
        }
    }

    #[tokio::test]
    async fn test_pool_respects_max_concurrent() {
        let pool = AgentPool::new(2);
        assert_eq!(pool.max_concurrent(), 2);

        let tasks: Vec<_> = (0..4)
            .map(|i| AgentTask::new(format!("sleep-{i}"), "sleep").args(vec!["0.01".into()]))
            .collect();
        let start = Instant::now();
        let results = pool.execute_all(tasks).await;
        // With max_concurrent=2 and 4 tasks each sleeping 0.01s,
        // total should be ~0.02s (two batches)
        let elapsed = start.elapsed();
        assert!(
            elapsed < Duration::from_secs(1),
            "took too long: {elapsed:?}"
        );
        assert_eq!(results.len(), 4);
        for r in &results {
            assert!(r.success, "{} failed", r.name);
        }
    }

    #[test]
    fn test_agent_task_builder() {
        let task = AgentTask::new("test", "/bin/echo")
            .args(vec!["hello".into()])
            .env("KEY", "VALUE");
        assert_eq!(task.name, "test");
        assert_eq!(task.binary_path.to_str(), Some("/bin/echo"));
        assert_eq!(task.env_vars.get("KEY").unwrap(), "VALUE");
    }
}
