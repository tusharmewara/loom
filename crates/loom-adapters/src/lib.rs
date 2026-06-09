//! Preset tool adapters built on [`loom_generic_adapter::GenericAdapter`].
//!
//! | Module | Tool |
//! |--------|------|
//! | `jcode` | jcode CLI |
//! | `crush` | Crush (HTTP control plane) |
//! | `claude_code` | Claude Code (`~/.claude/`) |
//! | `opencode` | OpenCode |
//! | `goose` | Goose |
//! | `codex` | Codex |
//! | `cursor-cli` | Cursor CLI |
//! | `gemini-cli` | Gemini CLI |
//! | `aider` | Aider |
//! | `kiro` | Kiro (VS Code fork) |

pub mod aider;
pub mod claude_code;
pub mod codex;
pub mod crush;
pub mod cursor_cli;
pub mod gemini_cli;
pub mod goose;
pub mod jcode;
pub mod kiro;
pub mod opencode;

use anyhow::Result;
use loom_core::{DebugAdapterInfo, LspServerInfo, RoutingDecision};
use loom_generic_adapter::config::{DebugSection, LspSection, ToolToml};
use std::path::Path;

/// Shared adapter operations for ecosystem-registered tools.
pub trait LoomToolAdapter: Send + Sync {
    fn tool_name(&self) -> &'static str;
    fn default_toml(&self, home: &str) -> ToolToml;
    fn apply_routing_decision(&self, decision: &RoutingDecision) -> Result<()>;
    fn sync_from_native(&self, loom_dir: &str, home: &str) -> Result<SyncReport>;

    /// Push Loom-managed resources (MCP servers, skills) into this tool's native
    /// filesystem locations so the tool can use them without `loom exec` wrapping.
    ///
    /// The default implementation uses [`sync_mcp_to_native`] and [`sync_skills_to_native`].
    fn sync_to_native(&self, loom_dir: &str, home: &str) -> Result<SyncReport> {
        let mut report = SyncReport::default();

        // Push shared MCP servers into the tool's native config
        if let Some(mcp) = self.default_toml(home).mcp {
            if let Some(ref config_path) = mcp.config_path {
                match sync_mcp_to_native(loom_dir, config_path) {
                    Ok(n) => {
                        report.mcp_servers_synced = n;
                        if n > 0 {
                            report.notes.push(format!(
                                "synced {n} shared MCP server(s) to {}",
                                config_path
                            ));
                        }
                    }
                    Err(e) => report.notes.push(format!(
                        "MCP sync to {} failed: {e}",
                        config_path
                    )),
                }
            }
        }

        // Push shared skills into the tool's native skills directory
        if let Some(skills) = self.default_toml(home).skills {
            if let Some(ref native_dir) = skills.native_dir {
                match sync_skills_to_native(loom_dir, native_dir) {
                    Ok(n) => {
                        report.skills_synced = n;
                        if n > 0 {
                            report.notes.push(format!(
                                "synced {n} shared skill(s) to {}",
                                native_dir
                            ));
                        }
                    }
                    Err(e) => report.notes.push(format!(
                        "skills sync to {} failed: {e}",
                        native_dir
                    )),
                }
            }
        }

        Ok(report)
    }

    /// Returns LSP server configuration for this adapter, if any.
    fn lsp_config(&self, home: &str) -> Option<LspSection> {
        self.default_toml(home).lsp
    }

    /// Returns LSP servers this adapter supports.
    fn lsp_servers(&self, home: &str) -> Vec<LspServerInfo> {
        self.lsp_config(home)
            .map(|cfg| {
                cfg.servers
                    .into_iter()
                    .map(|s| LspServerInfo {
                        language: s.language,
                        command: s.command,
                        args: s.args,
                        file_extensions: s.file_extensions,
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Resolve the best LSP server for a given file extension.
    fn resolve_lsp_server(&self, extension: &str, home: &str) -> Option<LspServerInfo> {
        self.lsp_servers(home)
            .into_iter()
            .find(|s| s.file_extensions.iter().any(|e| e == extension))
    }

    /// Returns debug adapter configuration for this adapter, if any.
    fn debug_config(&self, home: &str) -> Option<DebugSection> {
        self.default_toml(home).debug
    }

    /// Returns debug adapters this adapter supports.
    fn debug_adapters(&self, home: &str) -> Vec<DebugAdapterInfo> {
        self.debug_config(home)
            .map(|cfg| {
                cfg.adapters
                    .into_iter()
                    .map(|a| DebugAdapterInfo {
                        language: a.language,
                        command: a.command,
                        args: a.args,
                        name: a.name,
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Resolve the best debug adapter for a given language.
    fn resolve_debug_adapter(&self, language: &str, home: &str) -> Option<DebugAdapterInfo> {
        self.debug_adapters(home)
            .into_iter()
            .find(|a| a.language == language)
    }
}

/// Write shared MCP servers from Loom's registry into a tool-native MCP config file.
/// Merges with any existing config — tool-specific servers are preserved.
pub fn sync_mcp_to_native(loom_dir: &str, native_config_path: &str) -> Result<usize> {
    let path = Path::new(native_config_path);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    // Read shared MCP servers from Loom's registry
    let mgr = loom_mcp::McpManager::new(loom_dir)?;
    let registry = mgr.registry();
    let shared_servers = registry.filter_shared(true)?;

    if shared_servers.is_empty() {
        return Ok(0);
    }

    // Read existing config (if any)
    let mut existing: serde_json::Value = if path.exists() {
        let content = std::fs::read_to_string(path)?;
        serde_json::from_str(&content).unwrap_or(serde_json::Value::Object(Default::default()))
    } else {
        serde_json::Value::Object(Default::default())
    };

    // Merge shared servers under a "loom_shared" key so they're clearly namespaced
    let obj = existing.as_object_mut().unwrap();
    let mut loom_servers = serde_json::Map::new();
    for entry in &shared_servers {
        let server_obj = serde_json::json!({
            "command": entry.transport.command,
            "args": entry.transport.args,
            "env": entry.transport.env,
        });
        loom_servers.insert(entry.id.clone(), server_obj);
    }

    if !loom_servers.is_empty() {
        // Write into a "loom_shared" key so the tool can distinguish loom-managed servers
        // Also write into mcpServers if it doesn't already have that entry
        let mcp_servers = obj
            .entry("mcpServers")
            .or_insert_with(|| serde_json::json!({}));
        if let Some(mcp_obj) = mcp_servers.as_object_mut() {
            for (id, server) in &loom_servers {
                // Only add if not already defined by the tool (tool-specific takes precedence)
                mcp_obj.entry(id).or_insert_with(|| server.clone());
            }
        }
    }

    // Write back
    let content = serde_json::to_string_pretty(&existing)?;
    std::fs::write(path, content)?;

    Ok(shared_servers.len())
}

/// Symlink Loom's shared skills directory into a tool-native skills directory.
/// Existing skill files in the tool's directory are preserved (loom skills are merged in).
pub fn sync_skills_to_native(loom_dir: &str, native_skills_dir: &str) -> Result<usize> {
    let loom_skills = Path::new(loom_dir).join("skills");
    if !loom_skills.exists() {
        return Ok(0);
    }

    let native_dir = Path::new(native_skills_dir);
    std::fs::create_dir_all(native_dir)?;

    let mut count = 0;
    for entry in std::fs::read_dir(&loom_skills)? {
        let entry = entry?;
        let name = entry.file_name();
        let loom_skill_path = entry.path();

        // Only process directories with SKILL.md
        if !loom_skill_path.is_dir() {
            continue;
        }
        if !loom_skill_path.join("SKILL.md").exists() {
            continue;
        }

        let native_skill_path = native_dir.join(&name);

        // If the tool already has this skill, skip (native takes precedence)
        if native_skill_path.exists() {
            continue;
        }

        // Create a symlink (Unix) or copy (fallback)
        #[cfg(unix)]
        {
            if std::os::unix::fs::symlink(&loom_skill_path, &native_skill_path).is_ok() {
                count += 1;
            }
        }
        #[cfg(not(unix))]
        {
            if copy_dir_recursive(&loom_skill_path, &native_skill_path).is_ok() {
                count += 1;
            }
        }
    }

    Ok(count)
}

#[cfg(not(unix))]
fn copy_dir_recursive(src: &Path, dst: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dst)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let file_type = entry.file_type()?;
        let src_path = entry.path();
        let dst_path = dst.join(entry.file_name());
        if file_type.is_dir() {
            copy_dir_recursive(&src_path, &dst_path)?;
        } else {
            std::fs::copy(&src_path, &dst_path)?;
        }
    }
    Ok(())
}

pub fn write_tool_registration(
    adapter: &dyn LoomToolAdapter,
    loom_dir: &str,
    home: &str,
) -> Result<()> {
    let tools_dir = std::path::Path::new(loom_dir).join("tools");
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
    pub skills_synced: usize,
    pub mcp_servers_imported: usize,
    pub mcp_servers_synced: usize,
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
        "codex" => Some(Box::new(codex::CodexAdapter)),
        "cursor-cli" | "cursor" => Some(Box::new(cursor_cli::CursorCliAdapter)),
        "gemini-cli" | "gemini" => Some(Box::new(gemini_cli::GeminiCliAdapter)),
        "kiro" => Some(Box::new(kiro::KiroAdapter)),
        "aider" => Some(Box::new(aider::AiderAdapter)),
        _ => None,
    }
}

pub fn supported_tools() -> &'static [&'static str] {
    &["jcode", "crush", "claude-code", "opencode", "goose", "codex", "cursor", "gemini-cli", "aider", "kiro"]
}

pub fn sync_tools_dir(loom_dir: &str) -> Result<SyncReport> {
    let mut report = SyncReport::default();
    let mgr = loom_mcp::McpManager::new(loom_dir)?;
    let registry = mgr.registry();
    let n = loom_mcp::import::import_from_tools_dir(
        std::path::Path::new(loom_dir),
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
