//! Runtime management commands.

use crate::cli::RuntimeAction;
use anyhow::Result;
use loom_core::config::ConfigReader;
pub async fn run(loom_dir: &str, action: &RuntimeAction) -> Result<()> {
    let config_reader = ConfigReader::new(loom_dir);

    match action {
        RuntimeAction::List { json } => {
            let config = config_reader.load()?;
            let workflow = config.workflow.unwrap_or_default();
            let default_runtime = workflow.runtime.clone(); // String

            let runtimes = vec![
                RuntimeInfo {
                    name: "process".to_string(),
                    available: true,
                    current: default_runtime == "process",
                    description: "Host process execution (default)".to_string(),
                },
                RuntimeInfo {
                    name: "docker".to_string(),
                    available: false, // Would check Docker daemon
                    current: default_runtime == "docker",
                    description: "Docker container execution".to_string(),
                },
            ];

            if *json {
                println!("{}", serde_json::to_string_pretty(&runtimes)?);
            } else {
                println!("Available runtimes:");
                for rt in &runtimes {
                    let marker = if rt.current { "*" } else { " " };
                    let status = if rt.available { "✓" } else { "○" };
                    println!(
                        "  {}{} {} ({}) - {}",
                        marker, rt.name, status, rt.description, rt.current
                    );
                }
            }
        }
        RuntimeAction::SetDefault { name } => {
            let mut config = config_reader.load()?;
            let mut workflow = config.workflow.unwrap_or_default();

            if name != "process" && name != "docker" {
                anyhow::bail!("Unknown runtime: '{}'. Available: process, docker", name);
            }

            workflow.runtime = name.clone();
            config.workflow = Some(workflow);

            config_reader.save(&config)?;
            println!("Default runtime set to '{}'", name);
        }
        RuntimeAction::Status => {
            let config = config_reader.load()?;
            let workflow = config.workflow.unwrap_or_default();
            let default_runtime = workflow.runtime.clone();

            println!("Runtime Status:");
            println!("  Default: {}", default_runtime);
            println!("  Process: Available");

            // Check Docker availability
            let docker_available = std::process::Command::new("docker")
                .arg("version")
                .output()
                .map(|o| o.status.success())
                .unwrap_or(false);
            println!(
                "  Docker: {}",
                if docker_available {
                    "Available"
                } else {
                    "Unavailable"
                }
            );
        }
    }
    Ok(())
}

#[derive(Debug, Clone, serde::Serialize)]
struct RuntimeInfo {
    name: String,
    available: bool,
    current: bool,
    description: String,
}
