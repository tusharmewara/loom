use anyhow::{Context, Result};
use loom_generic_adapter::config::{ToolSection, ToolToml};
use loom_ipc::IpcRequest;
use std::path::Path;

/// `loom thread <tool>` — register tool with preset TOML when available.
pub async fn run(
    tool_name: &str,
    apply_routing_prompt: Option<&str>,
    loom_dir: &str,
) -> Result<()> {
    let home = std::env::var("HOME").context("HOME not set")?;
    let tools_dir = Path::new(loom_dir).join("tools");
    let toml_path = tools_dir.join(format!("{tool_name}.toml"));

    std::fs::create_dir_all(&tools_dir)
        .with_context(|| format!("Failed to create tools directory: {}", tools_dir.display()))?;

    let is_new = !toml_path.exists();
    let action = if is_new { "Registered" } else { "Updated" };

    if let Some(adapter) = loom_adapters::adapter_for(tool_name) {
        loom_adapters::write_tool_registration(adapter.as_ref(), loom_dir, &home)?;
        println!(
            "✓ {action} preset adapter '{tool_name}' at {}",
            toml_path.display()
        );
        let report = adapter.sync_from_native(loom_dir, &home)?;
        if report.mcp_servers_imported > 0 {
            println!("  MCP servers imported: {}", report.mcp_servers_imported);
        }
        if report.providers_imported > 0 {
            println!("  Auth providers imported: {}", report.providers_imported);
        }
        for note in &report.notes {
            println!("  {note}");
        }
    } else {
        let config = ToolToml {
            tool: ToolSection {
                name: tool_name.to_string(),
                version: "1.0.0".to_string(),
            },
            skills: None,
            mcp: None,
            auth: None,
            hooks: None,
            routing: None,
            sessions: None,
        };
        let toml_str = toml::to_string_pretty(&config)?;
        std::fs::write(&toml_path, &toml_str)?;
        println!("✓ {action} tool '{tool_name}' at {}", toml_path.display());
    }

    let agents_path = Path::new(loom_dir);
    if let Some(resp) = crate::ipc_client::send_daemon_command_if_running(
        agents_path,
        IpcRequest::new("register_tool").with_params(serde_json::json!({ "name": tool_name })),
    )
    .await
    {
        println!("  Daemon: {}", resp.status);
    } else {
        println!("  Tip: run `loom daemon start` for live registration sync");
    }

    if let Some(prompt) = apply_routing_prompt {
        if let Some(adapter) = loom_adapters::adapter_for(tool_name) {
            let classifier = loom_routing::Classifier::new();
            let engine = loom_routing::DecisionEngine::with_loom_dir(loom_dir, None)
                .unwrap_or_else(|_| loom_routing::DecisionEngine::new());
            let score = classifier.classify(prompt);
            let decision = engine.decide(score);
            adapter.apply_routing_decision(&decision)?;
            println!(
                "  Routing applied: {} / {}",
                decision.recommended_provider, decision.recommended_model
            );
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use loom_generic_adapter::config::ToolToml;

    #[tokio::test]
    async fn test_thread_registers_new_tool() {
        let dir = tempfile::tempdir().unwrap();
        let loom_dir = dir.path().to_str().unwrap();
        run("cursor", None, loom_dir).await.unwrap();
        assert!(dir.path().join("tools").join("cursor.toml").exists());
    }

    #[tokio::test]
    async fn test_jcode_preset_has_skills_section() {
        let dir = tempfile::tempdir().unwrap();
        let loom_dir = dir.path().to_str().unwrap();
        run("jcode", None, loom_dir).await.unwrap();
        let content = std::fs::read_to_string(dir.path().join("tools").join("jcode.toml")).unwrap();
        let config: ToolToml = toml::from_str(&content).unwrap();
        assert!(config.skills.is_some());
        assert!(config.mcp.is_some());
    }
}
