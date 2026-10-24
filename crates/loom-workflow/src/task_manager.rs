use crate::phases::{default_model_tier, next_phase};
use crate::routing::{Classifier, DecisionEngine, RoutingOutcomeStore};
use anyhow::{Context, Result};
use loom_core::{
    config::ConfigReader, derive_level, derive_tier, types::WorkflowConfig, ModelTier,
    PhaseHistoryEntry, RoutingDecision, Task, TaskPhase, TaskStatus,
};
use std::path::{Path, PathBuf};
use uuid::Uuid;

/// Determine the next phase from Spec, applying the ComplexityScore gate.
/// Returns the next phase (Simulate or Plan) and the agent role to use.
fn spec_gate_next_phase(task: &Task, classifier: &Classifier) -> (TaskPhase, Option<&'static str>) {
    // Read stored complexity_score (computed at create_task), else classify description
    let score = match &task.complexity_score {
        Some(score) => score.clone(),
        None => classifier.classify_with_role(&task.description, Some("trellis-brainstorm")),
    };
    // Check if tier is at least Mid
    let next_phase = if score.model_tier as u8 >= ModelTier::Mid as u8 {
        TaskPhase::Simulate
    } else {
        TaskPhase::Plan
    };
    (next_phase, Some("trellis-brainstorm"))
}

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

    /// Persist a routing decision with its signals (routing-debt.md Step 6).
    /// Best-effort: telemetry warns on failure, never breaks the workflow.
    fn record_outcome(
        &self,
        source: &str,
        task_id: &str,
        phase: Option<&str>,
        score: &loom_core::ComplexityScore,
        decision: Option<&loom_core::RoutingDecision>,
    ) {
        let Ok(store) = RoutingOutcomeStore::new(self.loom_dir.to_str().unwrap()) else {
            return;
        };
        if let Err(e) = store.record_decision(source, task_id, phase, score, decision) {
            eprintln!("routing telemetry: failed to record decision: {e}");
        }
    }

    /// Close the task's open decisions as `completed` (task reached Finish).
    fn close_outcomes(&self, task_id: &str) {
        let Ok(store) = RoutingOutcomeStore::new(self.loom_dir.to_str().unwrap()) else {
            return;
        };
        if let Err(e) = store.record_completion(task_id) {
            eprintln!("routing telemetry: failed to record completion: {e}");
        }
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
            PathBuf::from(loom_core::loom_dir()).join("worktrees")
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
        let repo_root =
            Self::find_repo_root(&cwd).ok_or_else(|| anyhow::anyhow!("Not in a git repository"))?;

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
            // Repositories without any commit (e.g. freshly cloned or
            // created locally with no remote) reject --detach; retry
            // with --orphan (git >= 2.42) before giving up.
            let retry = std::process::Command::new("git")
                .args([
                    "worktree",
                    "add",
                    "--orphan",
                    worktree_path.to_str().unwrap(),
                ])
                .current_dir(&repo_root)
                .output();
            match retry {
                Ok(out) if out.status.success() => {}
                Ok(out) => {
                    let stderr = String::from_utf8_lossy(&out.stderr);
                    anyhow::bail!("git worktree add failed: {}", stderr.trim());
                }
                Err(e) => anyhow::bail!("git worktree add retry failed: {e}"),
            }
        }

        tracing::info!(
            "Created worktree '{}' at {}",
            worktree_name,
            worktree_path.display()
        );

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
            .map(|c| {
                if c.is_alphanumeric() || c == '-' || c == '_' {
                    c
                } else {
                    '-'
                }
            })
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

    pub fn create_task(
        &self,
        title: &str,
        description: &str,
        worktree: bool,
        runtime: Option<String>,
        initial_phase: Option<TaskPhase>,
    ) -> Result<Task> {
        let now = loom_core::rfc3339_now();
        let task_id = Uuid::new_v4().to_string();

        // Initial scoring with agent role for Plan phase
        let mut complexity = self
            .classifier
            .classify_with_role(description, Some("trellis-brainstorm"));

        // Apply config thresholds if provided
        if let Some(thresholds) = self.router.thresholds() {
            let new_level = derive_level(complexity.numeric, thresholds);
            complexity.level = new_level;
            complexity.model_tier = derive_tier(new_level);
        }

        // Apply phase default for Plan (Flagship), taking higher tier
        let phase_default_tier = default_model_tier(&TaskPhase::Plan);
        if phase_default_tier as u8 > complexity.model_tier as u8 {
            complexity.model_tier = phase_default_tier;
        }

        let entering_spec = initial_phase.as_ref() == Some(&TaskPhase::Spec);
        let task_phase = initial_phase.unwrap_or(TaskPhase::Plan);
        let routing_decision = self.router.decide(complexity.clone());

        self.record_outcome(
            "create_task",
            &task_id,
            Some("Plan"),
            &complexity,
            Some(&routing_decision),
        );

        // Determine if worktree should be created
        let config = self.workflow_config();
        let create_worktree = worktree && config.worktree_auto;

        // Worktree isolation is best-effort: a non-git directory, a
        // repository without commits (e.g. never pushed anywhere), or
        // missing git must not block task creation. The task then runs
        // in the project directory instead of an isolated worktree.
        let worktree_path = if create_worktree {
            match self.create_worktree(&task_id, title) {
                Ok(path) => Some(path),
                Err(e) => {
                    tracing::warn!("worktree skipped for task {task_id}: {e}");
                    None
                }
            }
        } else {
            None
        };

        let task = Task {
            task_id: task_id.clone(),
            title: title.to_string(),
            description: description.to_string(),
            phase: task_phase.clone(),
            phase_history: vec![PhaseHistoryEntry {
                phase: format!("{task_phase:?}"),
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
            spec_path: if entering_spec {
                Some(format!("{task_id}.spec.md"))
            } else {
                None
            },
            findings_path: None,
            phase_results: Vec::new(),
            tags: Vec::new(),
            worktree_path: worktree_path.map(|p| p.to_string_lossy().to_string()),
            runtime,
            last_run: None,
            created_at: now.clone(),
            updated_at: now,
            status: TaskStatus::Pending,
        };

        self.write_task(&task)?;

        // Default spec file for loop entry: create the empty placeholder
        // next to the task manifest so the simulate stage has a home.
        if entering_spec {
            let spec_path = self.tasks_dir.join(format!("{task_id}.spec.md"));
            let _ = std::fs::write(&spec_path, "");
        }

        Ok(task)
    }

    pub fn advance_phase(&self, task_id: &str) -> Result<Task> {
        let mut task = self
            .get(task_id)?
            .with_context(|| format!("task '{task_id}' not found"))?;

        // Handle Spec gate: when advancing from Spec, apply ComplexityScore gate
        let (next, agent_role) = if matches!(task.phase, TaskPhase::Spec) {
            spec_gate_next_phase(&task, &self.classifier)
        } else {
            let next = next_phase(&task.phase)
                .with_context(|| format!("task '{task_id}' is already in Finish phase"))?;
            // Determine agent role based on phase (could be overridden by explicit agent assignment)
            let agent_role = match next {
                TaskPhase::Spec => Some("trellis-brainstorm"),
                TaskPhase::Simulate => Some("trellis-brainstorm"),
                TaskPhase::Plan => Some("trellis-brainstorm"), // default, may be overridden
                TaskPhase::Implement => Some("trellis-implement"),
                TaskPhase::Verify => Some("trellis-check"),
                TaskPhase::Finish => Some("trellis-update-spec"),
            };
            (next, agent_role)
        };

        let now = loom_core::rfc3339_now();
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

        // Re-score with dynamic signals
        let mut complexity =
            self.classifier
                .classify_dynamic(&task.description, agent_role, &dynamic_signals);

        // Apply config thresholds if provided (overrides level/tier from numeric)
        if let Some(thresholds) = self.router.thresholds() {
            let new_level = derive_level(complexity.numeric, thresholds);
            complexity.level = new_level;
            complexity.model_tier = derive_tier(new_level);
        }

        // Apply phase default, taking the higher tier (Flagship > Mid > Slm)
        let phase_default_tier = default_model_tier(&task.phase);
        if phase_default_tier as u8 > complexity.model_tier as u8 {
            complexity.model_tier = phase_default_tier;
        }

        task.complexity_score = Some(complexity.clone());
        task.routing_decision = Some(self.router.decide(complexity.clone()));

        self.record_outcome(
            "advance_phase",
            task_id,
            Some(&format!("{next:?}")),
            &complexity,
            task.routing_decision.as_ref(),
        );

        // Also store scoring in the phase history entry we just added
        if let Some(last) = task.phase_history.last_mut() {
            last.complexity_score = Some(complexity.clone());
            last.routing_decision = Some(self.router.decide(complexity));
        }

        if matches!(task.phase, TaskPhase::Finish) {
            task.status = TaskStatus::Completed;
            self.close_outcomes(task_id);
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
                    decided_at: loom_core::rfc3339_now(),
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
                        .or_else(|| task.routing_decision.as_ref().map(|d| d.recommended_tier))
                        .unwrap_or(ModelTier::Mid),
                    recommended_provider: prov.to_string(),
                    recommended_model: model_id,
                    reason: format!("User override: provider={prov}"),
                    overrides_role_default: true,
                    decided_at: loom_core::rfc3339_now(),
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

        task.updated_at = loom_core::rfc3339_now();
        self.write_task(&task)?;
        Ok(task)
    }

    pub fn set_last_agent(&self, task_id: &str, agent: &str) -> Result<Task> {
        let mut task = self
            .get(task_id)?
            .with_context(|| format!("task '{task_id}' not found"))?;
        task.last_agent = Some(agent.to_string());
        task.updated_at = loom_core::rfc3339_now();
        self.write_task(&task)?;
        Ok(task)
    }

    /// Set the linked issue on a task.
    pub fn set_linked_issue(&self, task_id: &str, issue_ref: &str) -> Result<Task> {
        let mut task = self
            .get(task_id)?
            .with_context(|| format!("task '{task_id}' not found"))?;
        task.linked_issue = Some(issue_ref.to_string());
        task.updated_at = loom_core::rfc3339_now();
        self.write_task(&task)?;
        Ok(task)
    }

    /// Stamp the outcome of a daemon-tracked dispatch run onto the task.
    pub fn set_last_run(&self, task_id: &str, run: &loom_core::TaskLastRun) -> Result<Task> {
        let mut task = self
            .get(task_id)?
            .with_context(|| format!("task '{task_id}' not found"))?;
        task.last_run = Some(run.clone());
        task.updated_at = loom_core::rfc3339_now();
        self.write_task(&task)?;
        Ok(task)
    }

    /// Set per-phase sub-agent configuration.
    pub fn set_sub_agents(&self, task_id: &str, config: loom_core::SubAgentConfig) -> Result<Task> {
        let mut task = self
            .get(task_id)?
            .with_context(|| format!("task '{task_id}' not found"))?;
        task.sub_agents = Some(config);
        task.updated_at = loom_core::rfc3339_now();
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
        task.updated_at = loom_core::rfc3339_now();
        self.write_task(&task)?;
        Ok(task)
    }

    /// Replace the tag set for a task.
    pub fn set_tags(&self, task_id: &str, tags: &[String]) -> Result<Task> {
        let mut task = self
            .get(task_id)?
            .with_context(|| format!("task '{task_id}' not found"))?;
        task.tags = tags.to_vec();
        task.updated_at = loom_core::rfc3339_now();
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

#[cfg(test)]
mod tests {
    use super::TaskManager;
    use loom_core::TaskPhase;

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
        let task = manager
            .create_task("title", "description", true, None, None)
            .unwrap();
        assert!(task.tags.is_empty());

        let updated = manager
            .set_tags(
                &task.task_id,
                &["plan".to_string(), "plan-task:t1".to_string()],
            )
            .unwrap();
        assert_eq!(
            updated.tags,
            vec!["plan".to_string(), "plan-task:t1".to_string()]
        );

        let reloaded = manager.get(&task.task_id).unwrap().unwrap();
        assert_eq!(reloaded.tags, updated.tags);

        assert!(manager.set_tags("no-such-task", &[]).is_err());
    }

    #[test]
    fn remove_task_deletes_manifest_and_sidecars() {
        use std::io::Write;
        let (_tmp, manager) = manager();
        let task = manager
            .create_task("title", "description", true, None, None)
            .unwrap();

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
        let task = manager
            .create_task("Test task", "Test description", false, None, None)
            .unwrap();
        assert!(task.worktree_path.is_none());
    }

    #[test]
    fn create_task_survives_worktree_failure() {
        // Hermetic: a regular file at the worktree base path makes
        // worktree creation fail deterministically without relying on
        // git or touching the real ~/.loom.
        let dir = tempfile::tempdir().unwrap();
        let loom = dir.path().join("loom");
        std::fs::create_dir_all(&loom).unwrap();
        let blocker = dir.path().join("blocker");
        std::fs::write(&blocker, "x").unwrap();
        std::fs::write(
            loom.join("config.toml"),
            format!(
                "[workflow]\nworktree_auto = true\nworktree_base = \"{}\"\n",
                blocker.join("worktrees").display()
            ),
        )
        .unwrap();
        let manager = TaskManager::new(loom.to_str().unwrap());

        let task = manager.create_task("t", "d", true, None, None).unwrap();
        assert_eq!(task.title, "t");
        assert!(task.worktree_path.is_none());
        // The task must be fully usable despite the skipped worktree.
        assert!(manager.get(&task.task_id).unwrap().is_some());
    }

    #[test]
    fn test_spec_gate_low_score_advances_to_plan() {
        let (_tmp, manager) = manager();
        // Create a task directly in Spec phase with low complexity (short description)
        let mut task = manager
            .create_task("Spec Gate Low", "Fix typo", true, None, None)
            .unwrap();
        task.phase = TaskPhase::Spec;
        // Override complexity score to Low (Slm tier)
        use loom_core::{ComplexityLevel, ComplexityScore, ModelTier};
        task.complexity_score = Some(ComplexityScore {
            level: ComplexityLevel::Low,
            numeric: 0.1,
            model_tier: ModelTier::Slm,
            scored_at: loom_core::rfc3339_now(),
            signals: Default::default(),
        });
        manager.write_task(&task).unwrap();

        let advanced = manager.advance_phase(&task.task_id).unwrap();
        // Low score should skip Simulate and go directly to Plan
        assert_eq!(advanced.phase, TaskPhase::Plan);
    }

    #[test]
    fn test_spec_gate_mid_score_advances_to_simulate() {
        let (_tmp, manager) = manager();
        // Create a task directly in Spec phase with mid complexity
        let mut task = manager
            .create_task("Spec Gate Mid", "Refactor authentication middleware across modules", true, None, None)
            .unwrap();
        task.phase = TaskPhase::Spec;
        // Override complexity score to Medium (Mid tier)
        use loom_core::{ComplexityLevel, ComplexityScore, ModelTier};
        task.complexity_score = Some(ComplexityScore {
            level: ComplexityLevel::Medium,
            numeric: 0.5,
            model_tier: ModelTier::Mid,
            scored_at: loom_core::rfc3339_now(),
            signals: Default::default(),
        });
        manager.write_task(&task).unwrap();

        let advanced = manager.advance_phase(&task.task_id).unwrap();
        // Mid score should advance to Simulate normally
        assert_eq!(advanced.phase, TaskPhase::Simulate);
    }

    #[test]
    fn test_spec_gate_absent_score_classifies_description() {
        let (_tmp, manager) = manager();
        // Create a task in Spec phase without a stored complexity score
        // The classifier should run on the description
        // Use a complex description that will definitely trigger Mid tier
        let complex_desc = "Design and implement a distributed consensus protocol with leader election, log replication, \
            cluster membership changes, and crash recovery across multiple data centers. \
            This requires cross-module coordination, persistent state machines, network \
            partition handling, and extensive integration testing with fault injection.";
        let mut task = manager
            .create_task("Spec Gate Absent", complex_desc, true, None, None)
            .unwrap();
        task.phase = TaskPhase::Spec;
        task.complexity_score = None; // Force absent score
        manager.write_task(&task).unwrap();

        let advanced = manager.advance_phase(&task.task_id).unwrap();
        // The description is complex enough to be Mid or above, so it should go to Simulate
        // (The exact tier depends on the classifier, but it should not be Plan for this complex description)
        // We just verify it advances to either Simulate or Plan based on classification
        assert!(matches!(advanced.phase, TaskPhase::Simulate | TaskPhase::Plan));
        // But the key thing: it should NOT panic and should use the classifier
        // For this complex description, it should advance to Simulate
        assert_eq!(advanced.phase, TaskPhase::Simulate);
    }
}
