//! TaskDag: dependency graph built from a plan's `depends_on` edges.
//!
//! Transforms the flat `PlanTask` list into a schedulable DAG. Ready tasks
//! (all dependencies satisfied) batch together so fan-out runs them in
//! parallel, each in its own worktree.

use crate::chief::plan::{PlanTask, TaskPlan};
use anyhow::Result;

#[derive(Debug)]
pub struct TaskDag {
    /// Tasks in plan order, indexed by position.
    pub tasks: Vec<PlanTask>,
}

impl TaskDag {
    /// Build a DAG from the plan; rejects unknown `depends_on` refs and cycles.
    pub fn from_plan(plan: &TaskPlan) -> Result<TaskDag> {
        let ids: std::collections::HashSet<&str> =
            plan.tasks.iter().map(|t| t.id.as_str()).collect();
        for task in &plan.tasks {
            if let Some(dep) = task
                .depends_on
                .iter()
                .find(|d| !ids.contains(d.as_str()))
            {
                anyhow::bail!("task '{}' depends on unknown task '{}'", task.id, dep);
            }
        }
        let dag = TaskDag {
            tasks: plan.tasks.clone(),
        };
        dag.execution_batches()?; // eager acyclicity validation
        Ok(dag)
    }

    /// Level-order batches: batch N+1 runs only after every task in batch N.
    /// Unblocked tasks within a batch sort by id for deterministic output.
    pub fn execution_batches(&self) -> Result<Vec<Vec<String>>> {
        let mut remaining: Vec<usize> = (0..self.tasks.len()).collect();
        let mut batches = Vec::new();
        let mut unmet: Vec<usize> = self
            .tasks
            .iter()
            .map(|t| t.depends_on.len())
            .collect();

        while !remaining.is_empty() {
            let mut ready: Vec<usize> = remaining
                .iter()
                .copied()
                .filter(|&i| unmet[i] == 0)
                .collect();
            if ready.is_empty() {
                let stuck: Vec<&str> = remaining
                    .iter()
                    .map(|&i| self.tasks[i].id.as_str())
                    .collect();
                anyhow::bail!("dependency cycle among tasks: {:?}", stuck);
            }
            ready.sort_by_key(|&i| self.tasks[i].id.as_str());
            let ready_set: std::collections::HashSet<usize> = ready.iter().copied().collect();
            for &i in &ready {
                for j in &remaining {
                    if self.tasks[*j]
                        .depends_on
                        .contains(&self.tasks[i].id)
                    {
                        unmet[*j] -= 1;
                    }
                }
            }
            batches.push(
                ready
                    .iter()
                    .map(|&i| self.tasks[i].id.clone())
                    .collect::<Vec<_>>(),
            );
            remaining.retain(|i| !ready_set.contains(i));
        }
        Ok(batches)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chief::plan::PlanTask;

    fn task(id: &str, depends_on: Vec<String>) -> PlanTask {
        PlanTask {
            id: id.to_string(),
            title: id.to_string(),
            description: "d".to_string(),
            depends_on,
            agent: None,
            acceptance: vec![],
        }
    }

    #[test]
    fn batches_parallel_tasks_together() {
        let plan = TaskPlan {
            goal: "g".to_string(),
            tasks: vec![
                task("t1", vec![]),
                task("t2", vec![]),
                task("t3", vec!["t1".to_string(), "t2".to_string()]),
            ],
        };
        let dag = TaskDag::from_plan(&plan).unwrap();
        assert_eq!(
            dag.execution_batches().unwrap(),
            vec![vec!["t1".to_string(), "t2".to_string()], vec!["t3".to_string()]]
        );
    }

    #[test]
    fn linear_chain_is_sequential_single_element_batches() {
        let plan = TaskPlan {
            goal: "g".to_string(),
            tasks: vec![
                task("t1", vec![]),
                task("t2", vec!["t1".to_string()]),
                task("t3", vec!["t2".to_string()]),
            ],
        };
        let dag = TaskDag::from_plan(&plan).unwrap();
        assert_eq!(
            dag.execution_batches().unwrap(),
            vec![
                vec!["t1".to_string()],
                vec!["t2".to_string()],
                vec!["t3".to_string()]
            ]
        );
    }

    #[test]
    fn rejects_cycles() {
        let plan = TaskPlan {
            goal: "g".to_string(),
            tasks: vec![
                task("t1", vec!["t2".to_string()]),
                task("t2", vec!["t1".to_string()]),
            ],
        };
        let err = TaskDag::from_plan(&plan).unwrap_err();
        assert!(err.to_string().contains("cycle"));
    }
}
