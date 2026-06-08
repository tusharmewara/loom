//! `loom lsp` subcommands — Language Server Protocol integration.

use crate::cli::LspAction;
use loom_lsp::client::LspClient;
use loom_lsp::discover::discover_servers;

pub async fn run(action: &LspAction) -> anyhow::Result<()> {
    match action {
        LspAction::Servers { json } => {
            let servers = discover_servers();
            if *json {
                println!("{}", serde_json::to_string_pretty(&serde_json::json!({
                    "schema_version": 1,
                    "command": "lsp.servers",
                    "count": servers.len(),
                    "servers": servers,
                }))?);
            } else {
                if servers.is_empty() {
                    println!("No language servers found on PATH.");
                    println!("  Install one: rust-analyzer, pyright, typescript-language-server, gopls");
                } else {
                    println!("Available language servers:");
                    for s in &servers {
                        let exts = s.file_extensions.join(", ");
                        println!("  {}  ({})  — {}", s.command, exts, s.language);
                    }
                }
            }
        }
        LspAction::Rename { path, line, column, new_name, server } => {
            let server_cmd = resolve_server(path, server.as_deref())?;
            let uri = format!("file://{}", std::path::absolute(path)?.display());
            let mut client = LspClient::connect(&server_cmd.command, &server_cmd.args, &uri).await?;
            client.rename(&uri, *line, *column, new_name).await?;
            println!("✓ Renamed symbol at {}:{}:{} to '{}'", path, line, column, new_name);
            client.shutdown().await?;
        }
        LspAction::FindReferences { path, symbol, server } => {
            let server_cmd = resolve_server(path, server.as_deref())?;
            let uri = format!("file://{}", std::path::absolute(path)?.display());
            let mut client = LspClient::connect(&server_cmd.command, &server_cmd.args, &uri).await?;
            // Use line 0 as a fallback position
            let refs = client.find_references(&uri, 0, 0).await?;
            if refs.is_empty() {
                println!("No references found for '{}'", symbol);
            } else {
                println!("References for '{}' ({}):", symbol, refs.len());
                for r in &refs {
                    println!("  {}:{}:{}", r.uri.as_str(), r.range.start.line, r.range.start.character);
                }
            }
            client.shutdown().await?;
        }
    }
    Ok(())
}

struct ServerCmd {
    command: String,
    args: Vec<String>,
}

fn resolve_server(path: &str, server_override: Option<&str>) -> anyhow::Result<ServerCmd> {
    if let Some(s) = server_override {
        return Ok(ServerCmd {
            command: s.to_string(),
            args: vec![],
        });
    }
    let ext = std::path::Path::new(path)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("");
    let servers = discover_servers();
    for s in &servers {
        if s.file_extensions.iter().any(|e| e == ext) {
            return Ok(ServerCmd {
                command: s.command.clone(),
                args: s.args.clone(),
            });
        }
    }
    anyhow::bail!(
        "No language server found for '.{ext}' files. Use --server to specify one, or install one. ({})",
        servers.iter().map(|s| s.command.as_str()).collect::<Vec<_>>().join(", ")
    );
}
