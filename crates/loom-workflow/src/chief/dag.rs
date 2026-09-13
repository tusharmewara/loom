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
    #[allow(unused_variables)]
    pub fn from_plan(plan: &TaskPlan) -> Result<TaskDag> {
        unimplemented!("TaskDag::from_plan")
    }

    /// Level-order batches: batch N+1 runs only after every task in batch N.
    /// Unblocked tasks within a batch sort by id for deterministic output.
    pub fn execution_batches(&self) -> Result<Vec<Vec<String>>> {
        unimplemented!("TaskDag::execution_batches")
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