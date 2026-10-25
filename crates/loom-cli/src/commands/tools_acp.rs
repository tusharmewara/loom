use anyhow::Result;
use clap::Args;
use loom_sessions::acp_launch::{pick_distribution, registry_url};
use loom_sessions::{AcpClient, AgentMetadata};
use std::path::Path;

#[derive(Args, Debug)]
pub struct AddArgs {
    pub agent_id: String,
}

impl AddArgs {
    pub async fn handle(&self, loom_dir: &str) -> Result<()> {
        let client = AcpClient::new(&registry_url(loom_dir), std::time::Duration::from_secs(0))
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
            anyhow::bail!(
                "Agent `{}` not found in ACP registry (run `loom acp registry` to list available agents)",
                self.agent_id
            );
        }

        let entry = &matching[0];
        self.write_agent_entry(loom_dir, entry)?;

        println!("Registered ACP agent `{}` from registry", self.agent_id);
        Ok(())
    }

    fn write_agent_entry(&self, loom_dir: &str, entry: &AgentMetadata) -> Result<()> {
        let config_dir = Path::new(loom_dir).join("config");
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

        // Deterministic distribution choice: package runners first
        // (they install on demand), binary last.
        match pick_distribution(entry) {
            Some(distribution) => {
                for (key, value) in distribution.to_toml_entry() {
                    agent_entry.insert(key, value);
                }
            }
            None => {
                anyhow::bail!(
                    "Agent `{}` has no distribution available for this platform",
                    entry.id
                );
            }
        }

        agent_servers.insert(entry.id.clone(), toml::Value::Table(agent_entry));
        let toml_str = toml::to_string(&agent_servers)?;
        std::fs::write(agent_servers_path, toml_str)?;

        Ok(())
    }
}
