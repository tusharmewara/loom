use crate::agents_dir;
use anyhow::Result;
use clap::Args;
use loom_sessions::{AcpClient, AgentMetadata};
use std::path::Path;

#[derive(Args, Debug)]
pub struct AddArgs {
    pub agent_id: String,
}

impl AddArgs {
    pub async fn handle(&self, agents_dir: &str) -> Result<()> {
        let registry_url = "https://cdn.agentclientprotocol.com/registry";
        let client = AcpClient::new(registry_url, std::time::Duration::from_secs(0))
            .await
            .map_err(|e| anyhow::anyhow!("Failed to create ACP client: {}", e))?;
        let agents = client
            .discover_agents()
            .await
            .map_err(|e| anyhow::anyhow!("Failed to discover ACP agents: {}", e))?;

        let matching: Vec<_> = agents
            .iter()
            .filter(|a| a.id == self.agent_id)
            .cloned()
            .collect();

        if matching.is_empty() {
            anyhow::bail!("Agent `{}` not found in ACP registry", self.agent_id);
        }

        let entry = &matching[0];
        self.write_agent_entry(agents_dir, entry)?;

        println!("Registered ACP agent `{}` from registry", self.agent_id);
        Ok(())
    }

    fn write_agent_entry(&self, agents_dir: &str, entry: &AgentMetadata) -> Result<()> {
        let config_dir = Path::new(agents_dir).join("config");
        std::fs::create_dir_all(&config_dir)?;

        let agent_servers_path = config_dir.join("agents.toml");
        let mut agent_servers: toml::Table = if agent_servers_path.exists() {
            let raw = std::fs::read_to_string(&agent_servers_path)?;
            toml::from_str(&raw)?
        } else {
            toml::Table::new()
        };

        let mut agent_entry = toml::Table::new();
        agent_entry.insert(
            "source".to_string(),
            toml::Value::String("acp-registry".to_string()),
        );

        if let Some(map) = entry.distribution.as_object() {
            for (runner_key, val) in map.iter().take(1) {
                agent_entry.insert(
                    "runner".to_string(),
                    toml::Value::String(runner_key.clone()),
                );
                if let Some(pkg) = val.get("package").and_then(|v| v.as_str()) {
                    agent_entry.insert(
                        "package".to_string(),
                        toml::Value::String(pkg.to_string()),
                    );
                }
                if let Some(args) = val.get("args").and_then(|v| v.as_array()) {
                    let args_vec: Vec<String> = args
                        .iter()
                        .filter_map(|v| v.as_str().map(String::from))
                        .collect();
                    agent_entry.insert(
                        "args".to_string(),
                        toml::Value::Array(
                            args_vec
                                .into_iter()
                                .map(toml::Value::String)
                                .collect::<Vec<_>>(),
                        ),
                    );
                }
                break;
            }
        }

        agent_servers.insert(
            entry.id.clone(),
            toml::Value::Table(agent_entry),
        );
        let toml_str = toml::to_string(&agent_servers)?;
        std::fs::write(agent_servers_path, toml_str)?;

        println!("Registered ACP agent `{}` from registry", entry.id);
        Ok(())
    }
}
