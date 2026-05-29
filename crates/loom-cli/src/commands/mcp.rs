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
                        let config: toml::Value =
                            toml::from_str(&content).unwrap_or(toml::Value::Table(Default::default()));
                        servers.push(serde_json::json!({
                            "id": name.strip_suffix(".toml").unwrap_or(&name),
                            "name": config.get("name").and_then(|v| v.as_str()).unwrap_or(""),
                            "transport": config.get("transport").cloned(),
                        }));
                    }
                }
            }
            if *json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&serde_json::json!({
                        "schema_version": 1,
                        "command": "mcp.list",
                        "count": servers.len(),
                        "servers": servers
                    }))?
                );
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
                    println!(
                        "{}",
                        serde_json::to_string_pretty(&serde_json::json!({
                            "schema_version": 1,
                            "command": "mcp.show",
                            "id": id,
                            "config": config
                        }))?
                    );
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
            std::fs::write(
                &path,
                format!(
                    r#"id = "{name}"
name = "{name}"
[transport]
transport_type = "Stdio"
command = ""
"#
                ),
            )?;
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
        McpAction::Install { name, prewarm } => {
            let known = loom_mcp::known::find(name)
                .ok_or_else(|| anyhow::anyhow!("Unknown MCP server '{}'. Use `loom mcp list-known` to see available servers.", name))?;

            install_known_server(loom_dir, known, *prewarm)?;
        }
        McpAction::InstallRecommended { prewarm } => {
            let recommended = loom_mcp::known::recommended();
            if recommended.is_empty() {
                println!("No recommended MCP servers defined.");
                return Ok(());
            }
            for server in &recommended {
                if let Err(e) = install_known_server(loom_dir, server, *prewarm) {
                    eprintln!("Warning: failed to install '{}': {e}", server.id);
                }
            }
            println!(
                "\nInstalled {} recommended MCP server(s). They will be injected into any tool launched via `loom exec`.",
                recommended.len()
            );
        }
        McpAction::ListKnown { json } => {
            let all = loom_mcp::known::ALL;
            if *json {
                let entries: Vec<serde_json::Value> = all
                    .iter()
                    .map(|s| {
                        serde_json::json!({
                            "id": s.id,
                            "name": s.name,
                            "description": s.description,
                            "npm_package": s.npm_package,
                            "tags": s.tags,
                        })
                    })
                    .collect();
                println!(
                    "{}",
                    serde_json::to_string_pretty(&serde_json::json!({
                        "schema_version": 1,
                        "command": "mcp.list-known",
                        "count": entries.len(),
                        "servers": entries
                    }))?
                );
            } else {
                let recommended = loom_mcp::known::recommended();
                let other: Vec<&loom_mcp::known::KnownMcpServer> =
                    all.iter().filter(|s| !s.tags.contains(&"recommended")).collect();

                println!("Known MCP servers (use `loom mcp install <id>` to install):\n");

                if !recommended.is_empty() {
                    println!("  ★ Recommended:");
                    for s in &recommended {
                        println!("     {:<14}  {}", s.id, s.description);
                    }
                }

                if !other.is_empty() {
                    println!("\n  More:");
                    for s in &other {
                        println!("     {:<14}  {}", s.id, s.description);
                    }
                }

                println!(
                    "\n{} known server(s) ({} recommended)",
                    all.len(),
                    recommended.len()
                );
            }
        }
        McpAction::Serve => {
            // Start the Loom MCP server (stdio mode).
            // Tools that support MCP can register this server and spawn it.
            let loom_dir_str = loom_dir.to_string_lossy().to_string();
            let server = loom_mcp_server::LoomMcpServer::new(&loom_dir_str);
            println!("✓ Loom MCP server started (stdio transport). Waiting for requests...");
            eprintln!("loom-mcp-server (pid={}) ready", std::process::id());
            server.run()?;
        }
    }
    Ok(())
}

fn install_known_server(
    loom_dir: &Path,
    known: &loom_mcp::known::KnownMcpServer,
    prewarm: bool,
) -> anyhow::Result<()> {
    let loom_dir_str = loom_dir.to_string_lossy();
    let registry =
        loom_mcp::McpRegistry::new(&loom_dir_str).map_err(|e| anyhow::anyhow!("{e}"))?;

    // Check if already registered
    if registry
        .get(known.id)
        .ok()
        .flatten()
        .is_some()
    {
        println!("  MCP server '{}' is already registered (skipping)", known.id);
        return Ok(());
    }

    // Register in the SQLite registry as a shared server
    let entry = known.to_entry();
    registry
        .upsert(&entry)
        .map_err(|e| anyhow::anyhow!("Failed to register '{}': {e}", known.id))?;
    println!("  ✓ Registered '{}' ({})", known.id, known.name);

    // Also write a companion TOML in mcp/ for file-based listing
    let mcp_dir = loom_dir.join("mcp");
    std::fs::create_dir_all(&mcp_dir)?;
    let toml_path = mcp_dir.join(format!("{}.toml", known.id));
    if !toml_path.exists() {
        let toml_content = format!(
            r#"id = "{}"
name = "{}"
description = "{}"
npm_package = "{}"

[transport]
transport_type = "Stdio"
command = "npx"
args = [{}]
"#,
            known.id,
            known.name,
            known.description,
            known.npm_package,
            known
                .args
                .iter()
                .map(|a| format!("\"{}\"", a))
                .collect::<Vec<_>>()
                .join(", "),
        );
        std::fs::write(&toml_path, toml_content)?;
    }

    // Optionally pre-warm npm cache so first tool use is blazing fast
    if prewarm {
        println!("  ⏳ Pre-warming npm cache for '{}'...", known.id);
        let status = std::process::Command::new("npx")
            .args(known.args)
            .arg("--help")
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status();
        match status {
            Ok(s) if s.success() => println!("  ✓ npm cache warmed"),
            Ok(s) => eprintln!(
                "  ⚠ npm cache warm exited with code {} (still usable, just slower on first call)",
                s
            ),
            Err(e) => eprintln!(
                "  ⚠ npm cache warm failed: {e} (still usable, just slower on first call)"
            ),
        }
    }

    Ok(())
}
