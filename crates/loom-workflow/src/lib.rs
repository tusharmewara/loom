pub mod orchestrator;
pub mod phases;
pub mod routing;
pub mod task;
pub mod task_manager;

use loom_core::HealthCheck;
use orchestrator::TaskOrchestrator;

pub struct WorkflowEngine {
    #[allow(dead_code)]
    agents_dir: String,
    orchestrator: TaskOrchestrator,
}

impl WorkflowEngine {
    pub fn new(agents_dir: &str) -> Self {
        Self {
            agents_dir: agents_dir.to_string(),
            orchestrator: TaskOrchestrator::new(agents_dir),
        }
    }

    pub fn orchestrator(&self) -> &TaskOrchestrator {
        &self.orchestrator
    }
}

impl HealthCheck for WorkflowEngine {
    fn health(&self) -> anyhow::Result<loom_core::HealthStatus> {
        let count = self.orchestrator.list_tasks().unwrap_or_default().len();
        Ok(loom_core::HealthStatus {
            crate_name: "loom-workflow",
            version: env!("CARGO_PKG_VERSION"),
            uptime_seconds: 0,
            last_error: None,
            performance_counters: {
                let mut m = std::collections::HashMap::new();
                m.insert("tasks".into(), count as u64);
                m
            },
        })
    }
}
