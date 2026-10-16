//! Auto-import MCP servers from tool TOML configuration files.
//!
//! Tools like jcode, Crush, etc. declare their MCP server configurations
//! in `mcp_config` sections of their own TOML files.  This module scans
//! those paths and imports the server entries into the registry.

use anyhow::{Context, Result};
use loom_core::{HealthStatusStr, McpServerEntry, McpTransport, McpTransportType};
use serde::Deserialize;
use std::collections::{HashMap, HashSet};
use std::path::Path;
use tracing::{debug, info, warn};

/// Schema for a tool's `mcp_config` section in TOML.
#[derive(Debug, Clone, Deserialize)]
pub struct ToolMcpConfig {
    /// Map of server id → server-config TOML table.
    #[serde(default)]
    pub servers: HashMap<String, ToolMcpServer>,

    /// Optional list of file paths to scan for additional configs.
    #[serde(default)]
    pub mcp_config_paths: Vec<String>,
}

/// Schema for a single MCP server declaration inside a tool TOML.
#[derive(Debug, Clone, Deserialize)]
pub struct ToolMcpServer {
    /// Human-readable server name (optional; falls back to id).
    pub name: Option<String>,

    /// Transport type — "stdio", "http", or "sse".
    #[serde(default = "default_transport_type")]
    pub transport_type: String,

    /// Command for stdio transport.
    pub command: Option<String>,

    /// Arguments for stdio transport.
    #[serde(default)]
    pub args: Vec<String>,

    /// URL for HTTP/SSE transport.
    pub url: Option<String>,

    /// Whether this server is shared across tools.
    #[serde(default = "default_true")]
    pub shared: bool,

    /// Tags for categorisation.
    #[serde(default)]
    pub tags: Vec<String>,

    /// Environment variables.
    #[serde(default)]
    pub env: HashMap<String, String>,

    /// Compatible tool names.
    #[serde(default)]
    pub compatible_tools: Vec<String>,
}

fn default_transport_type() -> String {
    "stdio".to_string()
}

fn default_true() -> bool {
    true
}

/// Auto-import MCP servers from tool TOML files.
///
/// # Arguments
/// * `global_config` — a TOML string from the tool's own config (e.g.
///   the tool's `config.toml` or the ecosystem `mcp_config`).
/// * `registry_upsert` — callback to insert/update the parsed entries
///   into the registry.
///
/// Returns the number of servers imported.
pub fn import_from_toml(
    toml_content: &str,
    registry_upsert: &dyn Fn(McpServerEntry) -> Result<()>,
) -> Result<usize> {
    let config: ToolMcpConfig =
        toml::from_str(toml_content).context("Failed to parse tool MCP config TOML")?;

    let mut count = 0;
    for (id, tool_srv) in &config.servers {
        let entry = convert_to_entry(id, tool_srv)?;
        registry_upsert(entry)?;
        count += 1;
    }
    info!("Auto-import: imported {} MCP servers from TOML", count);
    Ok(count)
}

/// Import servers from external TOML file paths specified in the config.
///
/// Each path is resolved relative to the `base_dir`.
pub fn import_from_paths(
    config: &ToolMcpConfig,
    base_dir: &Path,
    registry_upsert: &dyn Fn(McpServerEntry) -> Result<()>,
) -> Result<usize> {
    let mut total = 0;
    let mut seen = HashSet::new();

    for rel_path in &config.mcp_config_paths {
        let full_path = base_dir.join(rel_path);
        if !full_path.exists() {
            warn!(
                "Auto-import: mcp_config_path '{}' not found (skipping)",
                full_path.display()
            );
            continue;
        }

        // Guard against circular imports via canonical path
        let canonical = std::fs::canonicalize(&full_path).unwrap_or_else(|_| full_path.clone());
        if !seen.insert(canonical.clone()) {
            debug!("Auto-import: skipping duplicate path {:?}", canonical);
            continue;
        }

        match std::fs::read_to_string(&full_path)
            .with_context(|| format!("Failed to read {}", full_path.display()))
        {
            Ok(content) => match import_from_toml(&content, registry_upsert) {
                Ok(n) => {
                    total += n;
                    info!("Auto-import: imported {} servers from {:?}", n, full_path);
                }
                Err(e) => {
                    warn!("Auto-import: failed to import from {:?}: {}", full_path, e);
                }
            },
            Err(e) => {
                warn!("Auto-import: failed to read {:?}: {}", full_path, e);
            }
        }
    }

    Ok(total)
}

/// Import servers from every `mcp.toml` file found in `loom_dir/tools/`.
///
/// This discovers tool-specific MCP configs that tools leave in the
/// shared ecosystem directory.
pub fn import_from_tools_dir(
    loom_dir: &Path,
    registry_upsert: &dyn Fn(McpServerEntry) -> Result<()>,
) -> Result<usize> {
    let tools_dir = loom_dir.join("tools");
    if !tools_dir.exists() {
        debug!("Auto-import: tools dir {:?} does not exist", tools_dir);
        return Ok(0);
    }

    let mut total = 0;
    let entries = std::fs::read_dir(&tools_dir)
        .with_context(|| format!("Failed to read tools dir {:?}", tools_dir))?;

    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            let mcp_config = path.join("mcp.toml");
            if mcp_config.exists() {
                match std::fs::read_to_string(&mcp_config) {
                    Ok(content) => match import_from_toml(&content, registry_upsert) {
                        Ok(n) => {
                            total += n;
                            info!("Auto-import: imported {} servers from {:?}", n, mcp_config);
                        }
                        Err(e) => {
                            warn!("Auto-import: failed to import from {:?}: {}", mcp_config, e);
                        }
                    },
                    Err(e) => {
                        warn!("Auto-import: failed to read {:?}: {}", mcp_config, e);
                    }
                }
            }
        }
    }

    Ok(total)
}

/// Convert a `ToolMcpServer` TOML entry into a full `McpServerEntry`.
fn convert_to_entry(id: &str, tool_srv: &ToolMcpServer) -> Result<McpServerEntry> {
    let transport_type = match tool_srv.transport_type.to_lowercase().as_str() {
        "stdio" => McpTransportType::Stdio,
        "http" => McpTransportType::Http,
        "sse" => McpTransportType::Sse,
        other => anyhow::bail!(
            "Unknown transport type '{}' for server '{}'. Expected stdio, http, or sse.",
            other,
            id
        ),
    };

    let last_health_check = String::new();

    Ok(McpServerEntry {
        id: id.to_string(),
        name: tool_srv.name.clone().unwrap_or_else(|| id.to_string()),
        transport: McpTransport {
            transport_type,
            command: tool_srv.command.clone(),
            args: tool_srv.args.clone(),
            url: tool_srv.url.clone(),
            env: tool_srv.env.clone(),
        },
        shared: tool_srv.shared,
        tools_filter: vec![],
        health_status: HealthStatusStr::Unknown,
        last_health_check,
        tags: tool_srv.tags.clone(),
        compatible_tools: tool_srv.compatible_tools.clone(),
        required_by_skills: vec![],
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    #[test]
    fn test_import_from_toml() {
        let toml = r#"
[servers]
[servers.my-server]
name = "My Server"
transport_type = "stdio"
command = "node"
args = ["server.js"]
shared = true
tags = ["database", "production"]
compatible_tools = ["jcode", "crush"]

[servers.my-server.env]
API_KEY = "test-key"
"#;

        let imported = Mutex::new(Vec::new());
        let count = import_from_toml(toml, &|entry| {
            loom_core::lock_ok!(imported).push(entry);
            Ok(())
        })
        .unwrap();

        assert_eq!(count, 1);
        let imported = loom_core::lock_ok!(imported);
        assert_eq!(imported[0].id, "my-server");
        assert_eq!(imported[0].name, "My Server");
        assert!(matches!(
            imported[0].transport.transport_type,
            McpTransportType::Stdio
        ));
        assert_eq!(imported[0].transport.command.as_deref(), Some("node"));
        assert_eq!(imported[0].transport.args, vec!["server.js"]);
        assert!(imported[0].shared);
        assert!(imported[0].tags.contains(&"database".to_string()));
    }

    #[test]
    fn test_import_with_url_transport() {
        let toml = r#"
[servers]
[servers.remote-api]
name = "Remote API"
transport_type = "http"
url = "https://api.example.com/mcp"
shared = true
"#;

        let imported = Mutex::new(Vec::new());
        let count = import_from_toml(toml, &|entry| {
            loom_core::lock_ok!(imported).push(entry);
            Ok(())
        })
        .unwrap();

        assert_eq!(count, 1);
        let imported = loom_core::lock_ok!(imported);
        assert!(matches!(
            imported[0].transport.transport_type,
            McpTransportType::Http
        ));
        assert_eq!(
            imported[0].transport.url.as_deref(),
            Some("https://api.example.com/mcp")
        );
    }

    #[test]
    fn test_import_from_paths() {
        let dir = tempfile::tempdir().unwrap();
        let config_path = dir.path().join("my-mcp.toml");
        std::fs::write(
            &config_path,
            r#"
[servers]
[servers.ext-server]
name = "External"
transport_type = "stdio"
command = "python"
args = ["mcp-server.py"]
shared = false
"#,
        )
        .unwrap();

        let cfg = ToolMcpConfig {
            servers: HashMap::new(),
            mcp_config_paths: vec!["my-mcp.toml".to_string()],
        };

        let imported = Mutex::new(Vec::new());
        let count = import_from_paths(&cfg, dir.path(), &|entry| {
            loom_core::lock_ok!(imported).push(entry);
            Ok(())
        })
        .unwrap();

        assert_eq!(count, 1);
    }

    #[test]
    fn test_import_from_tools_dir() {
        let dir = tempfile::tempdir().unwrap();
        let tools_dir = dir.path().join("tools");
        std::fs::create_dir_all(&tools_dir).unwrap();

        let tool_dir = tools_dir.join("jcode");
        std::fs::create_dir_all(&tool_dir).unwrap();
        std::fs::write(
            tool_dir.join("mcp.toml"),
            r#"
[servers]
[servers.jcode-fs]
name = "Filesystem"
transport_type = "stdio"
command = "npx"
args = ["@modelcontextprotocol/server-filesystem", "/tmp"]
"#,
        )
        .unwrap();

        let imported = Mutex::new(Vec::new());
        let count = import_from_tools_dir(dir.path(), &|entry| {
            loom_core::lock_ok!(imported).push(entry);
            Ok(())
        })
        .unwrap();

        assert_eq!(count, 1);
        assert_eq!(loom_core::lock_ok!(imported)[0].id, "jcode-fs");
    }

    #[test]
    fn test_invalid_transport_type() {
        let toml = r#"
[servers]
[servers.bad]
transport_type = "invalid_protocol"
"#;

        let result = import_from_toml(toml, &|_| Ok(()));
        assert!(result.is_err());
    }
}
