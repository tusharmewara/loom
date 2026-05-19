use crate::cli::WorkflowAction;
use loom_workflow::WorkflowEngine;
use std::path::Path;

pub async fn run(loom_dir: &Path, action: &WorkflowAction) -> anyhow::Result<()> {
    let engine = WorkflowEngine::new(loom_dir.to_str().unwrap());
    let orchestrator = engine.orchestrator();

    match action {
        WorkflowAction::List { json } => {
            let tasks = orchestrator.list_tasks()?;
            if *json {
                let task_summaries: Vec<_> = tasks.iter().map(|t| {
                    serde_json::json!({
                        "task_id": t.task_id,
                        "title": t.title,
                        "phase": format!("{:?}", t.phase),
                        "status": format!("{:?}", t.status),
                    })
                }).collect();
                println!("{}", serde_json::to_string_pretty(&serde_json::json!({
                    "schema_version": 1,
                    "command": "workflow.list",
                    "count": task_summaries.len(),
                    "tasks": task_summaries
                }))?);
            } else {
                if tasks.is_empty() {
                    println!("No workflow tasks.");
                } else {
                    for task in tasks {
                        println!(
                            "{}  phase={:?}  status={:?}  title={}",
                            task.task_id, task.phase, task.status, task.title
                        );
                    }
                }
            }
        }
        WorkflowAction::Show { task_id, json } => {
            let task = orchestrator
                .get_task(task_id)?
                .ok_or_else(|| anyhow::anyhow!("Task '{task_id}' not found"))?;
            if *json {
                println!("{}", serde_json::to_string_pretty(&serde_json::json!({
                    "schema_version": 1,
                    "command": "workflow.show",
                    "task": task
                }))?);
            } else {
                println!("{}", serde_json::to_string_pretty(&task)?);
            }
        }
        WorkflowAction::Create { title, description } => {
            let title = title
                .clone()
                .unwrap_or_else(|| "Untitled task".to_string());
            let description = description
                .clone()
                .unwrap_or_else(|| "New workflow task".to_string());
            let task = orchestrator.create_task(&title, &description)?;
            println!("Created task {}", task.task_id);
            println!("{}", serde_json::to_string_pretty(&task)?);
        }
        WorkflowAction::Advance { task_id } => {
            let task = orchestrator.advance_phase(task_id)?;
            println!("Advanced task {} to phase {:?}", task.task_id, task.phase);
            println!("{}", serde_json::to_string_pretty(&task)?);
        }
    }
    Ok(())
}
