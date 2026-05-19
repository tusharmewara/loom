//! opencode adapter for Loom ecosystem
//!
//! Converts opencode-specific TOML configs to Loom-compatible Skill and MCP formats
//!
use anyhow::{Context, Result};
use loom_core::{McpServerEntry, SkillEntry};
use std::collections::HashMap;
use std::path::Path;

/// Load opencode config from tool's `mcp.toml`
pub fn load_opencode_config(agents_dir: &str) -> Result<(McpServerEntry, SkillEntry)> {
    // 1. Find opencode's config path
    let config_path = Path::new(agents_dir).join(".opencode/mcp.toml");
    if !config_path.exists() {
        return Err(anyhow::anyhow!("opencode config not found: {}", config_path.display()));
    }

    // 2. Parse TOML
    let content = std::fs::read_to_string(&config_path)?.trim().to_string();
    let toml: HashMap<String, toml::Value> = toml::from_str(&content)?;

    // 3. Extract MCP server entry
    let mcp_toml = toml.get("mcp")?.as_object()?;
    let id = mcp_toml.get("id")?.as_str()?.to_string();
    let name = mcp_toml.get("name")?.as_str()?.unwrap_or(&id);
    let transport = mcp_toml.get("transport_type")?.as_str()?.to_string();
    
    // Convert to MCP format
    let server_entry = McpServerEntry::new(
        id,
        name,
        transport,
        mcp_toml.get("command")?.as_str()?,
        mcp_toml.get("args")?.as_array()?.iter().map(|s| s.as_str()?).collect(),
        mcp_toml.get("url")?.as_str()?,
        mcp_toml.get("env")?.as_object()?.clone()?,
        true
    );

    // 4. Extract skill entry (if any)
    let skill_toml = toml.get("skill")?.as_object()?;
    let skill_id = skill_toml.get("id")?.as_str()?.to_string();
    let skill_name = skill_toml.get("name")?.as_str()?.unwrap_or(&skill_id);
    let skill_content = skill_toml.get("content")?.as_str()?.to_string();

    let skill_entry = SkillEntry::new(
        skill_id,
        skill_name,
        skill_content,
        skill_toml.get("tags")?.as_array()?.iter().map(|s| s.as_str().unwrap().to_string()).collect(),
        skill_toml.get("version")?.as_str()?.unwrap_or("1.0.0"),
    );

    Ok((server_entry, skill_entry))
}