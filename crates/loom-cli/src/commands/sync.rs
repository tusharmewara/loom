use anyhow::Result;
use loom_adapters::{adapter_for, supported_tools};

/// `loom sync [--tool <name>]`
///
/// Pushes Loom-managed resources (MCP servers, skills) into each registered
/// tool's native filesystem locations. This enables tools to use shared
/// Loom resources without `loom exec` wrapping.
///
/// When `--tool` is specified, only that tool is synced.
/// Otherwise, all supported tools are synced.
pub async fn run(
    tool_filter: Option<&str>,
    loom_dir: &str,
    json: bool,
) -> Result<()> {
    let home = std::env::var("HOME").expect("HOME not set");

    let tools: Vec<&str> = if let Some(name) = tool_filter {
        if !supported_tools().contains(&name) {
            anyhow::bail!(
                "Unknown tool '{name}'. Supported tools: {}",
                supported_tools().join(", ")
            );
        }
        vec![name]
    } else {
        supported_tools().to_vec()
    };

    let mut all_notes = Vec::new();
    let mut total_mcp = 0usize;
    let mut total_skills = 0usize;

    for tool_name in &tools {
        if let Some(adapter) = adapter_for(tool_name) {
            match adapter.sync_to_native(loom_dir, &home) {
                Ok(report) => {
                    total_mcp += report.mcp_servers_synced;
                    total_skills += report.skills_synced;
                    if !report.notes.is_empty() {
                        all_notes.extend(report.notes);
                    }
                    if !json {
                        println!("✓ {tool_name}: {} MCP, {} skills synced",
                            report.mcp_servers_synced,
                            report.skills_synced,
                        );
                    }
                }
                Err(e) => {
                    if json {
                        all_notes.push(format!("{tool_name}: sync failed — {e}"));
                    } else {
                        eprintln!("⚠ {tool_name}: sync failed — {e}");
                    }
                }
            }
        }
    }

    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "command": "sync",
                "tools_synced": tools.len(),
                "total_mcp_servers": total_mcp,
                "total_skills": total_skills,
                "notes": all_notes,
            }))?
        );
    }

    Ok(())
}
