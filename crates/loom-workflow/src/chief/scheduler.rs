//! Fan-out dispatch scheduler.
//!
//! Binds each ready task (from `TaskDag::execution_batches`) to a worker
//! harness and its dedicated worktree path, producing per-batch fan-out plans
//! that `AgentPool::execute_all` (loom-parallel) can execute.

use crate::chief::dag::TaskDag;
use crate::chief::harness::HarnessSelection;
use anyhow::Result;
use std::collections::HashMap;

/// One executable unit: task + harness + worktree.
#[derive(Debug, Clone, PartialEq)]
pub struct DispatchEntry {
    pub task_id: String,
    pub harness: String,
    pub worktree_path: String,
}

/// Map DAG batches to fan-out batches. Workers are assigned round-robin across
/// the selection's worker pool; receipts must exist for every task.
pub fn build_dispatch(
    dag: &TaskDag,
    selection: &HarnessSelection,
    worktrees: &HashMap<String, String>,
) -> Result<Vec<Vec<DispatchEntry>>> {
    if selection.workers.is_empty() && !dag.tasks.is_empty() {
        anyhow::bail!("no worker harnesses available for dispatch");
    }
    let mut batches = Vec::new();
    let mut worker_i = 0usize;
    for batch_ids in dag.execution_batches()? {
        let mut entries = Vec::new();
        for task_id in batch_ids {
            let worktree_path = worktrees
                .get(&task_id)
                .ok_or_else(|| anyhow::anyhow!("no worktree for task '{task_id}'"))?;
            let harness = selection.workers[worker_i % selection.workers.len()].clone();
            worker_i += 1;
            entries.push(DispatchEntry {
                task_id,
                harness,
                worktree_path: worktree_path.clone(),
            });
        }
        batches.push(entries);
    }
    Ok(batches)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chief::dag::TaskDag;
    use crate::chief::harness::HarnessSelection;
    use crate::chief::plan::{PlanTask, TaskPlan};

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

    fn pl() -> TaskPlan {
        TaskPlan {
            goal: "g".to_string(),
            tasks: vec![
                task("t1", vec![]),
                task("t2", vec![]),
                task("t3", vec!["t1".to_string(), "t2".to_string()]),
            ],
        }
    }

    fn worktrees(map: &[(&str, &str)]) -> HashMap<String, String> {
        map.iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    #[test]
    fn fan_out_binds_task_harness_and_worktree_per_batch() {
        let dag = TaskDag::from_plan(&pl()).unwrap();
        let selection = HarnessSelection {
            chief: "codex".to_string(),
            workers: vec!["jcode".to_string(), "goose".to_string()],
        };
        let worktrees = worktrees(&[
            ("t1", "/tmp/wt/t1"),
            ("t2", "/tmp/wt/t2"),
            ("t3", "/tmp/wt/t3"),
        ]);

        let batches = build_dispatch(&dag, &selection, &worktrees).unwrap();
        assert_eq!(batches.len(), 2);
        assert_eq!(batches[0].len(), 2);
        assert_eq!(batches[1].len(), 1);
        assert_eq!(batches[1][0].task_id, "t3");

        let first: Vec<&str> = batches[0].iter().map(|e| e.harness.as_str()).collect();
        assert!(first.contains(&"goose"));
        assert!(first.contains(&"jcode"));
        assert!(batches
            .iter()
            .flatten()
            .all(|e| e.worktree_path == worktrees[&e.task_id]));
    }

    #[test]
    fn missing_worktree_is_an_error() {
        let dag = TaskDag::from_plan(&pl()).unwrap();
        let selection = HarnessSelection {
            chief: "codex".to_string(),
            workers: vec!["jcode".to_string()],
        };
        let worktrees = worktrees(&[("t1", "/tmp/wt/t1")]);
        let err = build_dispatch(&dag, &selection, &worktrees).unwrap_err();
        assert!(err.to_string().contains("t2"));
    }

    #[test]
    fn no_workers_with_pending_tasks_is_an_error() {
        let dag = TaskDag::from_plan(&pl()).unwrap();
        let selection = HarnessSelection {
            chief: "codex".to_string(),
            workers: vec![],
        };
        let worktrees = worktrees(&[("t1", "/tmp/wt/t1")]);
        assert!(build_dispatch(&dag, &selection, &worktrees).is_err());
    }
}
