//! `loom lsp` subcommands — Language Server Protocol integration.

use crate::cli::LspAction;
use loom_adapters::adapter_for;
use loom_lsp::client::LspClient;
use loom_lsp::discover::discover_servers;
use std::path::Path;

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
        LspAction::Exec { tool, operation, path, symbol, line, column } => {
            let adapter = adapter_for(tool)
                .ok_or_else(|| anyhow::anyhow!("Unknown tool adapter: '{}'. Available: {}", tool, loom_adapters::supported_tools().join(", ")))?;

            let home = loom_core::home_dir();
            let servers = adapter.lsp_servers(&home);
            if servers.is_empty() {
                anyhow::bail!("Tool '{tool}' has no LSP servers configured.");
            }

            let ext = Path::new(path)
                .extension()
                .and_then(|e| e.to_str())
                .unwrap_or("");
            let server_info = servers.into_iter()
                .find(|s| s.file_extensions.iter().any(|e| e == ext))
                .ok_or_else(|| anyhow::anyhow!("No LSP server for '.{ext}' files in tool '{tool}' config"))?;

            let abs_path = std::path::absolute(path)?.display().to_string();
            let uri = format!("file://{abs_path}");
            let mut client = LspClient::connect(&server_info.command, &server_info.args, &uri).await?;

            match operation.as_str() {
                "rename" | "rename-symbol" => {
                    let new_name = symbol.as_deref()
                        .ok_or_else(|| anyhow::anyhow!("--symbol is required for rename operation"))?;
                    let l = line.unwrap_or(0);
                    let c = column.unwrap_or(0);
                    client.rename(&uri, l, c, new_name).await?;
                    println!("✓ [{tool}] Renamed symbol at {}:{}:{} to '{}'", path, l, c, new_name);
                }
                "find-references" | "references" => {
                    let refs = client.find_references(&uri, line.unwrap_or(0), column.unwrap_or(0)).await?;
                    let label = symbol.as_deref().unwrap_or("symbol");
                    if refs.is_empty() {
                        println!("No references found for '{label}'");
                    } else {
                        println!("[{tool}] References for '{label}' ({}):", refs.len());
                        for r in &refs {
                            println!("  {}:{}:{}", r.uri.as_str(), r.range.start.line, r.range.start.character);
                        }
                    }
                }
                "goto-definition" | "gotodef" | "go-to-def" => {
                    let l = line.unwrap_or(0);
                    let c = column.unwrap_or(0);
                    match client.goto_definition(&uri, l, c).await? {
                        Some(loc) => println!("[{tool}] Definition at {}:{}:{}", loc.uri.as_str(), loc.range.start.line, loc.range.start.character),
                        None => println!("No definition found"),
                    }
                }
                "code-action" => {
                    let l = line.unwrap_or(0);
                    let c = column.unwrap_or(0);
                    let range = lsp_types::Range {
                        start: lsp_types::Position { line: l, character: c },
                        end: lsp_types::Position { line: l + 1, character: c + 1 },
                    };
                    let actions = client.code_action(&uri, range).await?;
                    for action in &actions {
                        match action {
                            lsp_types::CodeActionOrCommand::CodeAction(ca) => {
                                println!("  [{tool}] Code action: {} ({:?})", ca.title, ca.kind);
                            }
                            lsp_types::CodeActionOrCommand::Command(cmd) => {
                                println!("  [{tool}] Command: {} ({})", cmd.title, cmd.command);
                            }
                        }
                    }
                }
                "document-symbols" | "symbols" => {
                    let symbols = client.document_symbols(&uri).await?;
                    println!("[{tool}] Symbols in {} ({}):", path, symbols.len());
                    for s in &symbols {
                        println!("  {} ({:?}) — {}", s.name, s.kind, s.location.uri.as_str());
                    }
                }
                other => anyhow::bail!("Unknown LSP operation '{other}'. Supported: rename, find-references, goto-definition, code-action, document-symbols"),
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
