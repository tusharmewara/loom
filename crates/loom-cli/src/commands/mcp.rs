use crate::cli::McpAction;
use std::path::Path;

pub async fn run(loom_dir: &Path, action: &McpAction) -> anyhow::Result<()> {
    match action {
        McpAction::List { json } => {
            let mcp_dir = loom_dir.join("mcp");
            let mut servers = Vec::new();
            if mcp_dir.exists() {
                for entry in std::fs::read_dir(&mcp_dir)? {
                    let entry = entry?;
                    let name = entry.file_name().to_string_lossy().to_string();
                    if name.ends_with(".toml") {
                        let content = std::fs::read_to_string(entry.path())?;
                        let config: toml::Value = toml::from_str(&content).unwrap_or(toml::Value::Table(Default::default()));
                        servers.push(serde_json::json!({
                            "id": name.strip_suffix(".toml").unwrap_or(&name),
                            "name": config.get("name").and_then(|v| v.as_str()).unwrap_or(""),
                            "transport": config.get("transport").cloned(),
                        }));
                    }
                }
            }
            if *json {
                println!("{}", serde_json::to_string_pretty(&serde_json::json!({
                    "schema_version": 1,
                    "command": "mcp.list",
                    "count": servers.len(),
                    "servers": servers
                }))?);
            } else {
                if servers.is_empty() {
                    println!("No MCP servers registered.");
                } else {
                    for s in &servers {
                        println!("{}  ({})", s["id"], s["name"]);
                    }
                }
            }
        }
        McpAction::Show { id, json } => {
            let path = loom_dir.join("mcp").join(format!("{}.toml", id));
            if path.exists() {
                let content = std::fs::read_to_string(&path)?;
                if *json {
                    let config: toml::Value = toml::from_str(&content)?;
                    println!("{}", serde_json::to_string_pretty(&serde_json::json!({
                        "schema_version": 1,
                        "command": "mcp.show",
                        "id": id,
                        "config": config
                    }))?);
                } else {
                    println!("{}", content);
                }
            } else {
                anyhow::bail!("MCP server '{}' not found", id);
            }
        }
        McpAction::Add { name } => {
            let path = loom_dir.join("mcp").join(format!("{}.toml", name));
            if path.exists() {
                anyhow::bail!("MCP server '{}' already exists", name);
            }
            std::fs::write(&path, format!(
                r#"id = "{name}"
name = "{name}"
[transport]
transport_type = "Stdio"
command = ""
"#
            ))?;
            println!("Added MCP server '{}'", name);
        }
        McpAction::Remove { id } => {
            let path = loom_dir.join("mcp").join(format!("{}.toml", id));
            if path.exists() {
                std::fs::remove_file(&path)?;
                println!("Removed MCP server '{}'", id);
            } else {
                anyhow::bail!("MCP server '{}' not found", id);
            }
        }
    }
    Ok(())
}
