//! Plan contract model + parser.
//!
//! The Chief planner harness produces a `TaskPlan` (JSON) via structured
//! output. `PlanParser::parse` validates the contract before it flows into the
//! DAG and scheduler.

use anyhow::Result;
use serde::{Deserialize, Serialize};

/// Schema version of the plan contract.
pub const PLAN_SCHEMA_VERSION: u32 = 1;

/// One atomic unit of work produced by the Chief planner.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlanTask {
    /// Unique id referenced by `depends_on` (e.g. "t1").
    pub id: String,
    pub title: String,
    /// Scope contract: what to build and — critically — what NOT to touch.
    pub description: String,
    #[serde(default)]
    pub depends_on: Vec<String>,
    /// Optional harness hint; omitted => assigned from routing tier at dispatch.
    #[serde(default)]
    pub agent: Option<String>,
    /// Commands/gates that prove the task done (invoked after the agent exists).
    #[serde(default)]
    pub acceptance: Vec<String>,
}

/// The full agentic plan: a goal plus a task graph.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskPlan {
    pub goal: String,
    pub tasks: Vec<PlanTask>,
}

/// Validates and parses the plan JSON contract.
///
/// Validation rules:
/// - `goal` is non-empty (after trimming whitespace)
/// - task `id`s are unique and non-empty (after trimming whitespace)
/// - every `depends_on` entry resolves to an existing task id
pub struct PlanParser;

impl PlanParser {
    pub fn parse(json: &str) -> Result<TaskPlan> {
        let plan: TaskPlan = serde_json::from_str(json)?;
        if plan.goal.trim().is_empty() {
            anyhow::bail!("plan goal must be non-empty");
        }
        let mut seen = std::collections::HashSet::new();
        for task in &plan.tasks {
            if task.id.trim().is_empty() {
                anyhow::bail!("task id must be non-empty");
            }
            if !seen.insert(task.id.clone()) {
                anyhow::bail!("duplicate task id: {}", task.id);
            }
        }
        for task in &plan.tasks {
            if let Some(dep) = task.depends_on.iter().find(|d| !seen.contains(*d)) {
                anyhow::bail!("task '{}' depends on unknown task '{}'", task.id, dep);
            }
        }
        Ok(plan)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const VALID_PLAN: &str = r#"{
        "goal": "Add dark mode to settings pane",
        "tasks": [
            {
                "id": "t1",
                "title": "Add theme design tokens",
                "description": "Define tokens; do not touch UI code.",
                "depends_on": [],
                "agent": "goose",
                "acceptance": ["cargo test -p loom-ui tokens"]
            },
            {
                "id": "t2",
                "title": "Wire toggle into settings",
                "description": "Use tokens from t1.",
                "depends_on": ["t1"],
                "agent": "jcode",
                "acceptance": ["cargo build"]
            }
        ]
    }"#;

    #[test]
    fn parses_valid_plan() {
        let plan = PlanParser::parse(VALID_PLAN).unwrap();
        assert_eq!(plan.goal, "Add dark mode to settings pane");
        assert_eq!(plan.tasks.len(), 2);
        assert_eq!(plan.tasks[1].depends_on, vec!["t1".to_string()]);
        assert_eq!(plan.tasks[0].agent.as_deref(), Some("goose"));
        assert_eq!(plan.tasks[0].acceptance.len(), 1);
    }

    #[test]
    fn omitted_optional_fields_deserialize_to_defaults() {
        let plan = r#"{
            "goal": "g",
            "tasks": [
                {
                    "id": "t1",
                    "title": "a",
                    "description": "d"
                }
            ]
        }"#;
        let parsed = PlanParser::parse(plan).unwrap();
        assert!(parsed.tasks[0].depends_on.is_empty());
        assert!(parsed.tasks[0].agent.is_none());
        assert!(parsed.tasks[0].acceptance.is_empty());
    }

    #[test]
    fn rejects_empty_goal() {
        for goal in ["\"\"", "\"   \""] {
            let plan = format!(
                r#"{{
                    "goal": {goal},
                    "tasks": [
                        {{ "id": "t1", "title": "a", "description": "d", "depends_on": [], "agent": "x", "acceptance": [] }}
                    ]
                }}"#
            );
            let err = PlanParser::parse(&plan).unwrap_err();
            assert!(err.to_string().contains("goal"));
        }
    }

    #[test]
    fn rejects_empty_task_id() {
        let plan = r#"{
            "goal": "g",
            "tasks": [
                { "id": "", "title": "a", "description": "d", "depends_on": [], "agent": "x", "acceptance": [] }
            ]
        }"#;
        let err = PlanParser::parse(plan).unwrap_err();
        assert!(err.to_string().contains("id"));
    }

    #[test]
    fn rejects_unknown_dependency() {
        let plan = r#"{
            "goal": "g",
            "tasks": [
                {
                    "id": "t1",
                    "title": "a",
                    "description": "d",
                    "depends_on": ["ghost"],
                    "agent": "x",
                    "acceptance": []
                }
            ]
        }"#;
        let err = PlanParser::parse(plan).unwrap_err();
        assert!(err.to_string().contains("ghost"));
    }

    #[test]
    fn rejects_duplicate_ids() {
        let plan = r#"{
            "goal": "g",
            "tasks": [
                { "id": "t1", "title": "a", "description": "d", "depends_on": [], "agent": "x", "acceptance": [] },
                { "id": "t1", "title": "b", "description": "d", "depends_on": [], "agent": "x", "acceptance": [] }
            ]
        }"#;
        assert!(PlanParser::parse(plan).is_err());
    }
}
