pub mod dispatch;
pub mod chief;
pub mod forge;
pub mod orchestrator;
pub mod phases;
pub mod routing;
pub mod task;
pub mod task_manager;
pub mod template_manager;

use loom_core::HealthCheck;
pub use orchestrator::TaskOrchestrator;
pub use template_manager::TemplateManager;
pub use task_manager::TaskManager;
pub use forge::{ForgeManager, PrInfo, WebhookResult};

pub struct WorkflowEngine {
    #[allow(dead_code)]
    loom_dir: String,
    orchestrator: TaskOrchestrator,
}

impl WorkflowEngine {
    pub fn new(loom_dir: &str) -> Self {
        Self {
            loom_dir: loom_dir.to_string(),
            orchestrator: TaskOrchestrator::new(loom_dir),
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
