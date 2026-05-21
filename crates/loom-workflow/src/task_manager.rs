use crate::phases::{default_model_tier, next_phase};
use crate::routing::{Classifier, DecisionEngine};
use anyhow::{Context, Result};
use loom_core::{
    config::ConfigReader, ComplexityLevel, ModelTier, PhaseHistoryEntry, RoutingDecision, Task,
    TaskPhase, TaskStatus,
};
use std::path::{Path, PathBuf};
use uuid::Uuid;

pub struct TaskManager {
    tasks_dir: PathBuf,
    classifier: Classifier,
    router: DecisionEngine,
}

impl TaskManager {
    pub fn new(loom_dir: &str) -> Self {
        let tasks_dir = Path::new(loom_dir).join("workflow").join("tasks");
        let _ = std::fs::create_dir_all(&tasks_dir);

        // Load routing config from ~/.loom/config.toml (or overlays)
        let config_reader = ConfigReader::new(loom_dir);
        let routing_config = config_reader.load().ok().and_then(|cfg| cfg.routing);

        let router = DecisionEngine::with_loom_dir(loom_dir, routing_config)
            .unwrap_or_else(|_| DecisionEngine::new());

        Self {
            tasks_dir,
            classifier: Classifier::new(),
            router,
        }
    }

    fn task_path(&self, task_id: &str) -> PathBuf {
        self.tasks_dir.join(format!("{task_id}.json"))
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

    pub fn create_task(&self, title: &str, description: &str) -> Result<Task> {
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
