//! Preset tool adapters built on [`loom_generic_adapter::GenericAdapter`].
//!
//! | Module | Tool |
//! |--------|------|
//! | `jcode` | jcode CLI |
//! | `crush` | Crush (HTTP control plane) |
//! | `claude_code` | Claude Code (`~/.claude/`) |
//! | `opencode` | OpenCode |
//! | `goose` | Goose |

pub mod claude_code;
pub mod crush;
pub mod goose;
pub mod jcode;
pub mod opencode;

use anyhow::Result;
use loom_core::RoutingDecision;
use loom_generic_adapter::config::ToolToml;

/// Shared adapter operations for ecosystem-registered tools.
pub trait LoomToolAdapter: Send + Sync {
    fn tool_name(&self) -> &'static str;
    fn default_toml(&self, home: &str) -> ToolToml;
    fn apply_routing_decision(&self, decision: &RoutingDecision) -> Result<()>;
    fn sync_from_native(&self, agents_dir: &str, home: &str) -> Result<SyncReport>;
}

pub fn write_tool_registration(
    adapter: &dyn LoomToolAdapter,
    agents_dir: &str,
    home: &str,
) -> Result<()> {
    let tools_dir = std::path::Path::new(agents_dir).join("tools");
    std::fs::create_dir_all(&tools_dir)?;
    let path = tools_dir.join(format!("{}.toml", adapter.tool_name()));
    let toml = toml::to_string_pretty(&adapter.default_toml(home))?;
    std::fs::write(&path, toml)?;
    tracing::info!(tool = adapter.tool_name(), path = %path.display(), "registered tool");
    Ok(())
}

#[derive(Debug, Clone, Default)]
pub struct SyncReport {
    pub skills_imported: usize,
    pub mcp_servers_imported: usize,
    pub providers_imported: usize,
    pub notes: Vec<String>,
}

pub fn adapter_for(tool: &str) -> Option<Box<dyn LoomToolAdapter>> {
    match tool {
        "jcode" => Some(Box::new(jcode::JcodeAdapter)),
        "crush" => Some(Box::new(crush::CrushAdapter)),
        "claude-code" | "claude_code" => Some(Box::new(claude_code::ClaudeCodeAdapter)),
        "opencode" => Some(Box::new(opencode::OpencodeAdapter)),
        "goose" => Some(Box::new(goose::GooseAdapter)),
        _ => None,
    }
}

pub fn supported_tools() -> &'static [&'static str] {
    &["jcode", "crush", "claude-code", "opencode", "goose"]
}

pub fn sync_tools_dir(agents_dir: &str) -> Result<SyncReport> {
    let mut report = SyncReport::default();
    let mgr = loom_mcp::McpManager::new(agents_dir)?;
    let registry = mgr.registry();
    let n = loom_mcp::import::import_from_tools_dir(
        std::path::Path::new(agents_dir),
        &|entry| {
            registry.upsert(&entry)?;
            Ok(())
        },
    )?;
    report.mcp_servers_imported = n;
    if n > 0 {
        report.notes.push(format!("imported {n} MCP server(s) from tools/"));
    }
    Ok(report)
}
