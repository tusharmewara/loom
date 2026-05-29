use crate::task_manager::TaskManager;
use anyhow::Result;
use loom_core::Task;

pub struct TaskOrchestrator {
    manager: TaskManager,
}

impl TaskOrchestrator {
    pub fn new(loom_dir: &str) -> Self {
        Self {
            manager: TaskManager::new(loom_dir),
        }
    }

    pub fn manager(&self) -> &TaskManager {
        &self.manager
    }

    pub fn create_task(&self, title: &str, description: &str) -> Result<Task> {
        self.manager.create_task(title, description)
    }

    pub fn advance_phase(&self, task_id: &str) -> Result<Task> {
        self.manager.advance_phase(task_id)
    }

    pub fn list_tasks(&self) -> Result<Vec<Task>> {
        self.manager.list()
    }

    pub fn get_task(&self, task_id: &str) -> Result<Option<Task>> {
        self.manager.get(task_id)
    }

    pub fn assign_routing(&self, task_id: &str) -> Result<Task> {
        self.manager.assign_routing(task_id)
    }

    pub fn set_last_agent(&self, task_id: &str, agent: &str) -> Result<Task> {
        self.manager.set_last_agent(task_id, agent)
    }

    pub fn assign_routing_with_overrides(
        &self,
        task_id: &str,
        provider: Option<&str>,
        model: Option<&str>,
    ) -> Result<Task> {
        self.manager
            .assign_routing_with_overrides(task_id, provider, model)
    }

    /// Link an issue to a task.
    pub fn set_linked_issue(&self, task_id: &str, issue_ref: &str) -> Result<Task> {
        self.manager.set_linked_issue(task_id, issue_ref)
    }

    /// Set per-phase sub-agent configuration.
    pub fn set_sub_agents(&self, task_id: &str, config: loom_core::SubAgentConfig) -> Result<Task> {
        self.manager.set_sub_agents(task_id, config)
    }

    /// Resolve the agent for the current phase (respects sub-agent config).
    pub fn resolve_agent(&self, task_id: &str) -> Result<Option<String>> {
        self.manager.resolve_agent(task_id)
    }
}
