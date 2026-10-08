use crate::phases::{default_model_tier, next_phase};
use crate::routing::{Classifier, DecisionEngine};
use anyhow::{Context, Result};
use loom_core::{
    config::ConfigReader,
    ComplexityLevel,
    ModelTier,
    PhaseHistoryEntry,
    RoutingDecision,
    Task,
    TaskPhase,
    TaskStatus,
    types::WorkflowConfig,
};
use std::path::{Path, PathBuf};
use uuid::Uuid;

pub struct TaskManager {
    tasks_dir: PathBuf,
    classifier: Classifier,
    router: DecisionEngine,
    loom_dir: PathBuf,
}

impl TaskManager {
    pub fn new(loom_dir: &str) -> Self {
        let loom_dir = PathBuf::from(loom_dir);
        let tasks_dir = loom_dir.join("workflow").join("tasks");
        let _ = std::fs::create_dir_all(&tasks_dir);

        // Load routing config from ~/.loom/config.toml (or overlays)
        let config_reader = ConfigReader::new(loom_dir.to_str().unwrap());
        let routing_config = config_reader.load().ok().and_then(|cfg| cfg.routing);

        let router = DecisionEngine::with_loom_dir(loom_dir.to_str().unwrap(), routing_config)
            .unwrap_or_else(|_| DecisionEngine::new());

        Self {
            tasks_dir,
            classifier: Classifier::new(),
            router,
            loom_dir,
        }
    }

    fn task_path(&self, task_id: &str) -> PathBuf {
        self.tasks_dir.join(format!("{task_id}.json"))
    }

    fn workflow_config(&self) -> WorkflowConfig {
        let config_reader = ConfigReader::new(self.loom_dir.to_str().unwrap());
        config_reader
            .load()
            .ok()
            .and_then(|cfg| cfg.workflow)
            .unwrap_or_default()
    }

    fn worktree_base(&self) -> PathBuf {
        let config = self.workflow_config();
        if let Some(base) = config.worktree_base {
            // Expand tilde
            let expanded = shellexpand::tilde(&base).to_string();
            PathBuf::from(expanded)
        } else {
            // Default: ~/.loom/worktrees
            dirs::home_dir()
                .unwrap_or_else(|| PathBuf::from("."))
                .join(".loom")
                .join("worktrees")
        }
    }

    fn create_worktree(&self, task_id: &str, title: &str) -> Result<PathBuf> {
        // Ensure git is available
        let output = std::process::Command::new("git")
            .arg("--version")
            .output()
            .map_err(|e| anyhow::anyhow!("Git not found: {e}"))?;
        if !output.status.success() {
            anyhow::bail!("Git is not available");
        }

        // Find repo root from current working directory
        let cwd = std::env::current_dir()?;
        let repo_root = Self::find_repo_root(&cwd)
            .ok_or_else(|| anyhow::anyhow!("Not in a git repository"))?;

        // Create base directory
        let base = self.worktree_base();
        std::fs::create_dir_all(&base)?;

        // Generate worktree name: UUID + sanitized title slug
        let slug = Self::sanitise_title(title);
        let worktree_name = format!("{}-{}", task_id, slug);
        let worktree_path = base.join(&worktree_name);

        // Create git worktree (detached HEAD at current branch)
        let output = std::process::Command::new("git")
            .args([
                "worktree",
                "add",
                "--detach",
                worktree_path.to_str().unwrap(),
            ])
            .current_dir(&repo_root)
            .output()
            .context("Failed to create git worktree")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            anyhow::bail!("git worktree add failed: {}", stderr.trim());
        }

        tracing::info!("Created worktree '{}' at {}", worktree_name, worktree_path.display());

        Ok(worktree_path)
    }

    fn find_repo_root(path: &Path) -> Option<PathBuf> {
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

    fn sanitise_title(title: &str) -> String {
        title
            .to_lowercase()
            .chars()
            .map(|c| if c.is_alphanumeric() || c == '-' || c == '_' { c } else { '-' })
            .collect::<String>()
            .trim_matches('-')
            .to_string()
            .chars()
            .take(40)
            .collect()
    }

    pub fn list(&self) -> Result<Vec<Task>> {
        let mut tasks = Vec::new();
        if !self.tasks_dir.exists() {
            return Ok(tasks);
        }
        for entry in std::fs::read_dir(&self.tasks_dir)? {
            let path = entry?.path();
            if path.extension().and_then(|e| e.to_str()) == Some("json") {
                if let Ok(task) = self.read_task_file(&path) {
                    tasks.push(task);
                }
            }
        }
        tasks.sort_by(|a, b| a.created_at.cmp(&b.created_at));
        Ok(tasks)
    }

    pub fn get(&self, task_id: &str) -> Result<Option<Task>> {
        let path = self.task_path(task_id);
        if !path.exists() {
            return Ok(None);
        }
        Ok(Some(self.read_task_file(&path)?))
    }

    fn read_task_file(&self, path: &Path) -> Result<Task> {
        let raw = std::fs::read_to_string(path)?;
        Ok(serde_json::from_str(&raw)?)
    }

    fn write_task(&self, task: &Task) -> Result<()> {
        let path = self.task_path(&task.task_id);
        std::fs::write(path, serde_json::to_string_pretty(task)?)?;
        Ok(())
    }

    pub fn create_task(&self, title: &str, description: &str, worktree: bool, runtime: Option<String>) -> Result<Task> {
        let now = rfc3339_now();
        let task_id = Uuid::new_v4().to_string();

        // Initial scoring with agent role for Plan phase
        let mut complexity = self
            .classifier
            .classify_with_role(description, Some("trellis-brainstorm"));

        // Apply config thresholds if provided
        if let Some(thresholds) = self.router.thresholds() {
            let new_level = if complexity.numeric < thresholds.low_max {
                ComplexityLevel::Low
            } else if complexity.numeric < thresholds.high_min {
                ComplexityLevel::Medium
            } else {
                ComplexityLevel::High
            };
            complexity.level = new_level;
            complexity.model_tier = match new_level {
                ComplexityLevel::Low => ModelTier::Slm,
                ComplexityLevel::Medium => ModelTier::Mid,
                ComplexityLevel::High => ModelTier::Flagship,
            };
        }

        // Apply phase default for Plan (Flagship), taking higher tier
        let phase_default_tier = default_model_tier(&TaskPhase::Plan);
        if phase_default_tier as u8 > complexity.model_tier as u8 {
            complexity.model_tier = phase_default_tier;
        }

        let routing_decision = self.router.decide(complexity.clone());

        // Determine if worktree should be created
        let config = self.workflow_config();
        let create_worktree = worktree && config.worktree_auto;

        let worktree_path = if create_worktree {
            Some(self.create_worktree(&task_id, title)?)
        } else {
            None
        };

        let task = Task {
            task_id: task_id.clone(),
            title: title.to_string(),
            description: description.to_string(),
            phase: TaskPhase::Plan,
            phase_history: vec![PhaseHistoryEntry {
                phase: "Plan".into(),
                started_at: now.clone(),
                completed_at: None,
                complexity_score: Some(complexity.clone()),
                routing_decision: Some(routing_decision.clone()),
            }],
            prd_path: None,
            context_files: vec![],
            routed_provider: None,
            files_modified: vec![],
            complexity_score: Some(complexity),
            routing_decision: Some(routing_decision),
            last_agent: None,
            linked_issue: None,
            sub_agents: None,
            phase_results: Vec::new(),
            tags: Vec::new(),
            worktree_path: worktree_path.map(|p| p.to_string_lossy().to_string()),
            runtime,
            created_at: now.clone(),
            updated_at: now,
            status: TaskStatus::Pending,
        };

        self.write_task(&task)?;
        Ok(task)
    }

    pub fn advance_phase(&self, task_id: &str) -> Result<Task> {
        let mut task = self
            .get(task_id)?
            .with_context(|| format!("task '{task_id}' not found"))?;

        let next = next_phase(&task.phase)
            .with_context(|| format!("task '{task_id}' is already in Finish phase"))?;

        let now = rfc3339_now();
        if let Some(last) = task.phase_history.last_mut() {
            last.completed_at = Some(now.clone());
        }

        task.phase = next.clone();
        task.phase_history.push(PhaseHistoryEntry {
            phase: format!("{next:?}"),
            started_at: now.clone(),
            completed_at: None,
            complexity_score: None,
            routing_decision: None,
        });
        task.updated_at = now;
        task.status = TaskStatus::InProgress;

        // Collect dynamic signals for refined scoring
        let prd_length = if let Some(prd_path) = &task.prd_path {
            if let Ok(content) = std::fs::read_to_string(prd_path) {
                Some(content.len())
            } else {
                None
            }
        } else {
            None
        };

        let files_modified_count = task.files_modified.len();

        let context_lines_count = if !task.context_files.is_empty() {
            // Rough estimate: sum of lines in context files
            let mut total_lines = 0u32;
            for ctx_file in &task.context_files {
                if let Ok(content) = std::fs::read_to_string(ctx_file) {
                    total_lines += content.lines().count() as u32;
                }
            }
            Some(total_lines)
        } else {
            None
        };

        // Count iterations of current phase in history (excluding the one we just added)
        let phase_iteration_count = task
            .phase_history
            .iter()
            .filter(|entry| entry.phase == format!("{next:?}"))
            .count() as u8;

        // Error rate not yet tracked; could be derived from retry counts later
        let error_rate = None;

        let dynamic_signals = crate::routing::DynamicSignals {
            files_modified_count,
            prd_length,
            context_lines_count,
            phase_iteration_count,
            error_rate,
            ..Default::default()
        };

        // Determine agent role based on phase (could be overridden by explicit agent assignment)
        let agent_role = match next {
            TaskPhase::Plan => Some("trellis-brainstorm"), // default, may be overridden
            TaskPhase::Implement => Some("trellis-implement"),
            TaskPhase::Verify => Some("trellis-check"),
            TaskPhase::Finish => Some("trellis-update-spec"),
        };

        // Re-score with dynamic signals
        let mut complexity =
            self.classifier
                .classify_dynamic(&task.description, agent_role, &dynamic_signals);

        // Apply config thresholds if provided (overrides level/tier from numeric)
        if let Some(thresholds) = self.router.thresholds() {
            let new_level = if complexity.numeric < thresholds.low_max {
                ComplexityLevel::Low
            } else if complexity.numeric < thresholds.high_min {
                ComplexityLevel::Medium
            } else {
                ComplexityLevel::High
            };
            complexity.level = new_level;
            complexity.model_tier = match new_level {
                ComplexityLevel::Low => ModelTier::Slm,
                ComplexityLevel::Medium => ModelTier::Mid,
                ComplexityLevel::High => ModelTier::Flagship,
            };
        }

        // Apply phase default, taking the higher tier (Flagship > Mid > Slm)
        let phase_default_tier = default_model_tier(&task.phase);
        if phase_default_tier as u8 > complexity.model_tier as u8 {
            complexity.model_tier = phase_default_tier;
        }

        task.complexity_score = Some(complexity.clone());
        task.routing_decision = Some(self.router.decide(complexity.clone()));

        // Also store scoring in the phase history entry we just added
        if let Some(last) = task.phase_history.last_mut() {
            last.complexity_score = Some(complexity.clone());
            last.routing_decision = Some(self.router.decide(complexity));
        }

        if matches!(task.phase, TaskPhase::Finish) {
            task.status = TaskStatus::Completed;
        }

        self.write_task(&task)?;
        Ok(task)
    }

    pub fn assign_routing(&self, task_id: &str) -> Result<Task> {
        self.assign_routing_with_overrides(task_id, None, None)
    }

    pub fn assign_routing_with_overrides(
        &self,
        task_id: &str,
        provider: Option<&str>,
        model: Option<&str>,
    ) -> Result<Task> {
        let mut task = self
            .get(task_id)?
            .with_context(|| format!("task '{task_id}' not found"))?;

        match (provider, model) {
            (Some(prov), Some(modl)) => {
                // User overrode both provider and model
                let decision = RoutingDecision {
                    recommended_tier: task
                        .routing_decision
                        .as_ref()
                        .map(|d| d.recommended_tier)
                        .unwrap_or(ModelTier::Mid),
                    recommended_provider: prov.to_string(),
                    recommended_model: modl.to_string(),
                    reason: format!("User override: provider={prov}, model={modl}"),
                    overrides_role_default: true,
                    decided_at: rfc3339_now(),
                    cost_estimate: task
                        .routing_decision
                        .as_ref()
                        .map(|d| d.cost_estimate.clone())
                        .unwrap_or_default(),
                };
                task.routing_decision = Some(decision.clone());
                task.routed_provider = Some(decision.recommended_provider.clone());
            }
            (Some(prov), None) => {
                // User overrode provider — decide model based on provider
                let catalog = self.router.catalog();
                let model_entry = catalog
                    .models
                    .iter()
                    .find(|m| m.provider_id == prov && m.available)
                    .or_else(|| catalog.models.iter().find(|m| m.provider_id == prov));
                let model_id = model_entry
                    .map(|m| m.model_id.clone())
                    .unwrap_or_else(|| format!("{prov}/default"));
                let decision = RoutingDecision {
                    recommended_tier: model_entry
                        .as_ref()
                        .map(|m| m.tier)
                        .or_else(|| {
                            task.routing_decision
                                .as_ref()
                                .map(|d| d.recommended_tier)
                        })
                        .unwrap_or(ModelTier::Mid),
                    recommended_provider: prov.to_string(),
                    recommended_model: model_id,
                    reason: format!("User override: provider={prov}"),
                    overrides_role_default: true,
                    decided_at: rfc3339_now(),
                    cost_estimate: task
                        .routing_decision
                        .as_ref()
                        .map(|d| d.cost_estimate.clone())
                        .unwrap_or_default(),
                };
                task.routing_decision = Some(decision.clone());
                task.routed_provider = Some(decision.recommended_provider.clone());
            }
            (None, _) => {
                // No provider override — run classifier as before
                if let Some(routing_decision) = &task.routing_decision {
                    task.routed_provider = Some(routing_decision.recommended_provider.clone());
                } else {
                    let complexity = self.classifier.classify(&task.description);
                    let routing_decision = self.router.decide(complexity);
                    task.routed_provider = Some(routing_decision.recommended_provider.clone());
                    task.routing_decision = Some(routing_decision);
                }
            }
        }

        task.updated_at = rfc3339_now();
        self.write_task(&task)?;
        Ok(task)
    }

    pub fn set_last_agent(&self, task_id: &str, agent: &str) -> Result<Task> {
        let mut task = self
            .get(task_id)?
            .with_context(|| format!("task '{task_id}' not found"))?;
        task.last_agent = Some(agent.to_string());
        task.updated_at = rfc3339_now();
        self.write_task(&task)?;
        Ok(task)
    }

    /// Link an issue to a task.
    pub fn set_linked_issue(&self, task_id: &str, issue_ref: &str) -> Result<Task> {
        let mut task = self
            .get(task_id)?
            .with_context(|| format!("task '{task_id}' not found"))?;
        task.linked_issue = Some(issue_ref.to_string());
        task.updated_at = rfc3339_now();
        self.write_task(&task)?;
        Ok(task)
    }

    /// Set per-phase sub-agent configuration.
    pub fn set_sub_agents(&self, task_id: &str, config: loom_core::SubAgentConfig) -> Result<Task> {
        let mut task = self
            .get(task_id)?
            .with_context(|| format!("task '{task_id}' not found"))?;
        task.sub_agents = Some(config);
        task.updated_at = rfc3339_now();
        self.write_task(&task)?;
        Ok(task)
    }

    /// Get the agent for the current phase based on sub-agent config.
    pub fn resolve_agent(&self, task_id: &str) -> Result<Option<String>> {
        let task = self
            .get(task_id)?
            .with_context(|| format!("task '{task_id}' not found"))?;
        let agent = match &task.sub_agents {
            Some(config) => config.agent_for(&task.phase, task.last_agent.as_deref()),
            None => task.last_agent.clone(),
        };
        Ok(agent)
    }

    /// Append (or replace) a phase result on the task. If a result for the
    /// same phase already exists it is replaced (re-runs overwrite).
    pub fn set_phase_result(&self, task_id: &str, result: loom_core::PhaseResult) -> Result<Task> {
        let mut task = self
            .get(task_id)?
            .with_context(|| format!("task '{task_id}' not found"))?;
        task.phase_results.retain(|r| r.phase != result.phase);
        task.phase_results.push(result);
        task.updated_at = rfc3339_now();
        self.write_task(&task)?;
        Ok(task)
    }

    /// Replace the tag set for a task.
    pub fn set_tags(&self, task_id: &str, tags: &[String]) -> Result<Task> {
        let mut task = self
            .get(task_id)?
            .with_context(|| format!("task '{task_id}' not found"))?;
        task.tags = tags.to_vec();
        task.updated_at = rfc3339_now();
        self.write_task(&task)?;
        Ok(task)
    }

    /// Delete a task manifest plus its per-phase sidecars
    /// (`{id}.{phase}.result.{json,md}`, `{id}.prior_sessions.json`).
    /// Captured sessions are left intact (they are history, not metadata).
    /// Errors when the task does not exist.
    pub fn remove_task(&self, task_id: &str) -> Result<()> {
        let path = self.task_path(task_id);
        if !path.exists() {
            anyhow::bail!("task '{task_id}' not found");
        }
        std::fs::remove_file(&path)?;
        if let Ok(entries) = std::fs::read_dir(&self.tasks_dir) {
            for entry in entries.flatten() {
                let name = entry.file_name().to_string_lossy().to_string();
                let is_result = name.starts_with(&format!("{task_id}."))
                    && (name.ends_with(".result.json") || name.ends_with(".result.md"));
                let is_priors = name == format!("{task_id}.prior_sessions.json");
                if is_result || is_priors {
                    let _ = std::fs::remove_file(entry.path());
                }
            }
        }
        Ok(())
    }

    /// Get the most recent phase result (across all phases), if any.
    pub fn latest_phase_result(&self, task_id: &str) -> Result<Option<loom_core::PhaseResult>> {
        let task = self
            .get(task_id)?
            .with_context(|| format!("task '{task_id}' not found"))?;
        Ok(task.phase_results.last().cloned())
    }
}

fn rfc3339_now() -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    let secs = now.as_secs();
    let days = secs / 86400;
    let time_secs = secs % 86400;
    let hours = time_secs / 3600;
    let minutes = (time_secs % 3600) / 60;
    let seconds = time_secs % 60;
    let (year, month, day) = days_to_date(days as i64 + 719468);
    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z",
        year, month, day, hours, minutes, seconds
    )
}

fn days_to_date(z: i64) -> (i64, u32, u32) {
    let z = z - 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    (y, m as u32, d as u32)
}

#[cfg(test)]
mod tests {
    use super::TaskManager;

    fn manager() -> (tempfile::TempDir, TaskManager) {
        let dir = tempfile::tempdir().unwrap();
        let loom = dir.path().join("loom");
        std::fs::create_dir_all(&loom).unwrap();
        // Hermetic: no git worktrees under the real ~/.loom in unit tests.
        std::fs::write(
            loom.join("config.toml"),
            "[workflow]\nworktree_auto = false\n",
        )
        .unwrap();
        let manager = TaskManager::new(loom.to_str().unwrap());
        (dir, manager)
    }

    #[test]
    fn set_tags_replaces_the_tag_set() {
        let (_tmp, manager) = manager();
        let task = manager.create_task("title", "description", true, None).unwrap();
        assert!(task.tags.is_empty());

        let updated = manager
            .set_tags(&task.task_id, &["plan".to_string(), "plan-task:t1".to_string()])
            .unwrap();
        assert_eq!(updated.tags, vec!["plan".to_string(), "plan-task:t1".to_string()]);

        let reloaded = manager.get(&task.task_id).unwrap().unwrap();
        assert_eq!(reloaded.tags, updated.tags);

        assert!(manager.set_tags("no-such-task", &[]).is_err());
    }

    #[test]
    fn remove_task_deletes_manifest_and_sidecars() {
        use std::io::Write;
        let (_tmp, manager) = manager();
        let task = manager.create_task("title", "description", true, None).unwrap();

        let tasks_dir = _tmp.path().join("loom").join("workflow").join("tasks");
        let sidecar = tasks_dir.join(format!("{}.Plan.result.json", task.task_id));
        let mut file = std::fs::File::create(&sidecar).unwrap();
        writeln!(file, "{{\"summary\": \"x\"}}").unwrap();
        let priors = tasks_dir.join(format!("{}.prior_sessions.json", task.task_id));
        std::fs::write(&priors, "{}").unwrap();

        manager.remove_task(&task.task_id).unwrap();
        assert!(manager.get(&task.task_id).unwrap().is_none());
        assert!(!sidecar.exists());
        assert!(!priors.exists());
        assert!(manager.remove_task(&task.task_id).is_err());
    }

    #[test]
    fn test_sanitise_title() {
        assert_eq!(TaskManager::sanitise_title("Hello World"), "hello-world");
        assert_eq!(TaskManager::sanitise_title("My_Task-Name"), "my_task-name");
        assert_eq!(TaskManager::sanitise_title("---test---"), "test");
        let long = "A".repeat(50);
        assert_eq!(TaskManager::sanitise_title(&long), "a".repeat(40));
    }

    #[test]
    fn test_find_repo_root() {
        // Should return None for non-git directory
        assert!(TaskManager::find_repo_root(std::path::Path::new("/tmp")).is_none());
    }

    #[test]
    fn test_workflow_config_defaults() {
        use loom_core::types::WorkflowConfig;
        let config = WorkflowConfig::default();
        assert!(config.worktree_auto);
        assert!(config.worktree_base.is_none());
    }

    #[test]
    fn test_create_task_worktree_field_when_disabled() {
        let (_tmp, manager) = manager();
        let task = manager.create_task("Test task", "Test description", false, None).unwrap();
        assert!(task.worktree_path.is_none());
    }
}
