//! Git worktree isolation for parallel agent execution.
//!
//! Each agent gets its own isolated git worktree so they can modify files
//! independently without conflicts.

use anyhow::{Context, Result};
use std::path::{Path, PathBuf};

/// Manages creation and cleanup of git worktrees for parallel agent isolation.
pub struct WorktreeManager {
    repo_path: PathBuf,
    worktrees: Vec<WorktreeEntry>,
}

struct WorktreeEntry {
    name: String,
    path: PathBuf,
}

impl WorktreeManager {
    /// Create a new manager for the given git repository.
    ///
    /// `repo_path` should point to the root of a git repository.
    pub fn new(repo_path: &Path) -> Self {
        Self {
            repo_path: repo_path.to_path_buf(),
            worktrees: Vec::new(),
        }
    }

    /// Check whether the current directory is inside a git repository.
    pub fn is_git_repo(path: &Path) -> bool {
        let mut dir = path.to_path_buf();
        loop {
            if dir.join(".git").exists() {
                return true;
            }
            if !dir.pop() {
                return false;
            }
        }
    }

    /// Find the root of the current git repository.
    pub fn find_repo_root(path: &Path) -> Option<PathBuf> {
        let output = std::process::Command::new("git")
            .args(["rev-parse", "--show-toplevel"])
            .current_dir(path)
            .output()
            .ok()?;
        if output.status.success() {
            let root = String::from_utf8_lossy(&output.stdout);
            Some(PathBuf::from(root.trim()))
        } else {
            None
        }
    }

    /// Create a new worktree for an agent.
    ///
    /// The worktree is created as a detached HEAD at the current branch tip.
    /// Returns the path to the new worktree.
    pub fn create_worktree(&mut self, agent_name: &str) -> Result<PathBuf> {
        ensure_git_available()?;

        let sanitized = sanitise_name(agent_name);
        let branch_name = format!("loom-parallel/{sanitized}-{}", chrono::Utc::now().format("%Y%m%d%H%M%S"));
        let worktree_path = self.repo_path.parent()
            .map(|p| p.join(&branch_name))
            .unwrap_or_else(|| {
                let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
                cwd.join(&branch_name)
            });

        let output = std::process::Command::new("git")
            .args([
                "worktree",
                "add",
                "--detach",
                worktree_path.to_str().unwrap(),
            ])
            .current_dir(&self.repo_path)
            .output()
            .context("Failed to create git worktree")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            anyhow::bail!("git worktree add failed: {}", stderr.trim());
        }

        tracing::info!(
            "Created worktree '{}' for agent '{}' at {}",
            branch_name,
            agent_name,
            worktree_path.display()
        );

        self.worktrees.push(WorktreeEntry {
            name: branch_name.clone(),
            path: worktree_path.clone(),
        });

        Ok(worktree_path)
    }

    /// Remove all created worktrees.
    pub fn cleanup(&self) -> Result<()> {
        ensure_git_available()?;
        let mut errors = Vec::new();

        for entry in &self.worktrees {
            // Remove the worktree
            let output = std::process::Command::new("git")
                .args(["worktree", "remove", "--force"])
                .arg(entry.path.to_str().unwrap())
                .current_dir(&self.repo_path)
                .output();

            match output {
                Ok(out) if !out.status.success() => {
                    errors.push(format!(
                        "Failed to remove worktree '{}': {}",
                        entry.name,
                        String::from_utf8_lossy(&out.stderr).trim()
                    ));
                }
                Err(e) => {
                    errors.push(format!("Failed to run git worktree remove: {e}"));
                }
                _ => {}
            }

            // Also remove the directory if it still exists
            if entry.path.exists() {
                let _ = std::fs::remove_dir_all(&entry.path);
            }
        }

        if errors.is_empty() {
            if !self.worktrees.is_empty() {
                tracing::info!("Cleaned up {} worktree(s)", self.worktrees.len());
            }
            Ok(())
        } else {
            anyhow::bail!("Worktree cleanup errors:\n  {}", errors.join("\n  "));
        }
    }

    /// Number of active worktrees.
    pub fn active_count(&self) -> usize {
        self.worktrees.len()
    }
}

impl Drop for WorktreeManager {
    fn drop(&mut self) {
        if !self.worktrees.is_empty() {
            if let Err(e) = self.cleanup() {
                tracing::warn!("Worktree cleanup on drop: {e}");
            }
        }
    }
}

fn ensure_git_available() -> Result<()> {
    let output = std::process::Command::new("git")
        .arg("--version")
        .output()
        .map_err(|e| anyhow::anyhow!("Git not found: {e}"))?;

    if !output.status.success() {
        anyhow::bail!("Git is not available");
    }
    Ok(())
}

fn sanitise_name(name: &str) -> String {
    name.to_lowercase()
        .chars()
        .map(|c| if c.is_alphanumeric() || c == '-' || c == '_' { c } else { '-' })
        .collect::<String>()
        .trim_matches('-')
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sanitise_name() {
        assert_eq!(sanitise_name("OpenCode"), "opencode");
        assert_eq!(sanitise_name("hello world"), "hello-world");
        assert_eq!(sanitise_name("---name---"), "name");
    }

    #[test]
    fn test_is_git_repo() {
        // This directory might not be a git repo, just check it doesn't crash
        let _ = WorktreeManager::is_git_repo(Path::new("/tmp"));
    }

    #[test]
    fn test_find_repo_root() {
        // Just check it doesn't crash
        let _ = WorktreeManager::find_repo_root(Path::new("/tmp"));
    }

    #[test]
    fn test_new_manager() {
        let mgr = WorktreeManager::new(Path::new("/tmp/repo"));
        assert_eq!(mgr.active_count(), 0);
    }
}
