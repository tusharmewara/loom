use crate::task_manager::TaskManager;
use anyhow::Result;
use loom_core::Task;

pub struct TaskOrchestrator {
    manager: TaskManager,
}

impl TaskOrchestrator {
    pub fn new(agents_dir: &str) -> Self {
        Self {
            manager: TaskManager::new(agents_dir),
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
}
