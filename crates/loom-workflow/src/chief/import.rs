//! Import a validated [`TaskPlan`] as concrete workflow tasks.
//!
//! Each plan task becomes one workflow task, created in dependency
//! order. Plan metadata that has no workflow field (dependencies,
//! acceptance gates) is encoded into the description text; the plan
//! identity travels in tags (`plan`, `plan-task:{id}`).

use crate::chief::dag::TaskDag;
use crate::chief::plan::TaskPlan;
use crate::task_manager::TaskManager;
use anyhow::Result;
use loom_core::Task;

/// Import a plan as workflow tasks, in dependency order.
///
/// The DAG is built (and therefore fully validated) before anything
/// is created, so an invalid plan creates no tasks. Plan-task fields
/// without a workflow counterpart are folded into the description;
/// agent hints become `last_agent`; plan identity becomes tags.
pub fn import_plan(manager: &TaskManager, plan: &TaskPlan) -> Result<Vec<Task>> {
    let dag = TaskDag::from_plan(plan)?;
    let mut created = Vec::new();
    for batch in dag.execution_batches()? {
        for id in &batch {
            let plan_task = plan
                .tasks
                .iter()
                .find(|t| &t.id == id)
                .ok_or_else(|| anyhow::anyhow!("plan task '{id}' vanished after validation"))?;
            let mut description = plan_task.description.clone();
            if !plan_task.acceptance.is_empty() {
                description.push_str("\n\nAcceptance:\n");
                for gate in &plan_task.acceptance {
                    description.push_str(&format!("- {gate}\n"));
                }
            }
            if !plan_task.depends_on.is_empty() {
                description.push_str(&format!(
                    "\nDepends on: {}",
                    plan_task.depends_on.join(", ")
                ));
            }
            let mut task = manager.create_task(&plan_task.title, &description, true, None, None)?;
            if let Some(agent) = plan_task.agent.as_deref() {
                task = manager.set_last_agent(&task.task_id, agent)?;
            }
            task = manager.set_tags(
                &task.task_id,
                &["plan".to_string(), format!("plan-task:{}", plan_task.id)],
            )?;
            created.push(task);
        }
    }
    Ok(created)
}

#[cfg(test)]
mod tests {
    use super::import_plan;
    use crate::chief::plan::{PlanParser, PlanTask, TaskPlan};
    use crate::task_manager::TaskManager;

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

    fn two_task_plan() -> TaskPlan {
        PlanParser::parse(
            r#"{"goal": "ship it", "tasks": [
                {"id": "t2", "title": "Second", "description": "needs t1",
                 "depends_on": ["t1"], "agent": "goose", "acceptance": ["make check"]},
                {"id": "t1", "title": "First", "description": "base work",
                 "depends_on": []}
            ]}"#,
        )
        .unwrap()
    }

    #[test]
    fn import_plan_creates_tagged_tasks_in_dependency_order() {
        let (_tmp, manager) = manager();
        let tasks = import_plan(&manager, &two_task_plan()).unwrap();

        assert_eq!(tasks.len(), 2);
        assert_eq!(tasks[0].title, "First");
        assert_eq!(tasks[1].title, "Second");
        for task in &tasks {
            assert!(
                task.tags.contains(&"plan".to_string()),
                "tags: {:?}",
                task.tags
            );
        }
        assert!(tasks[0].tags.contains(&"plan-task:t1".to_string()));
        assert!(tasks[1].tags.contains(&"plan-task:t2".to_string()));
        assert_eq!(tasks[0].last_agent, None);
        assert_eq!(tasks[1].last_agent.as_deref(), Some("goose"));
        assert!(tasks[1].description.contains("t1"));
        assert!(tasks[1].description.contains("make check"));
    }

    #[test]
    fn import_plan_rejects_invalid_plans_without_creating_tasks() {
        let (_tmp, manager) = manager();
        let plan = TaskPlan {
            goal: "g".to_string(),
            tasks: vec![PlanTask {
                id: "t1".to_string(),
                title: "x".to_string(),
                description: "d".to_string(),
                depends_on: vec!["ghost".to_string()],
                agent: None,
                acceptance: vec![],
            }],
        };
        assert!(import_plan(&manager, &plan).is_err());
        assert!(manager.list().unwrap().is_empty());
    }
}
