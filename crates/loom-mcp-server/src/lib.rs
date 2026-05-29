//! Loom MCP Server
//!
//! Implements an MCP (Model Context Protocol) server that exposes Loom's
//! shared resources (skills, MCP servers, sessions, auth) as MCP tools.
//! Any MCP-compatible client (jcode, claude-code, opencode, etc.) can
//! connect and use these tools directly.
//!
//! # Protocol
//!
//! Uses JSON-RPC 2.0 over stdio transport:
//! - Request:  `{"jsonrpc":"2.0","id":1,"method":"tools/list","params":{}}`
//! - Response: `{"jsonrpc":"2.0","id":1,"result":{"tools":[...]}}`
//!
//! # Standard MCP Methods
//!
//! | Method | Description |
//! |--------|-------------|
//! | `initialize` | Server capabilities |
//! | `tools/list` | List available Loom tools |
//! | `tools/call` | Call a Loom tool |
//!
//! # Custom Loom Tools
//!
//! | Tool | Description |
//! |------|-------------|
//! | `loom_list_skills` | List shared skills |
//! | `loom_get_skill` | Get skill content by name |
//! | `loom_list_mcp_servers` | List shared MCP servers |
//! | `loom_get_mcp_config` | Get merged MCP config |
//! | `loom_save_session` | Export session |
//! | `loom_list_providers` | List auth providers |
//! | `loom_get_credential` | Get credential for a provider |

use anyhow::Result;
use serde_json::Value;
use std::io::{self, BufRead, Write};
use std::path::Path;

/// Loom MCP Server instance.
pub struct LoomMcpServer {
    loom_dir: String,
}

impl LoomMcpServer {
    /// Create a new Loom MCP server targeting the given ecosystem directory.
    pub fn new(loom_dir: &str) -> Self {
        Self {
            loom_dir: loom_dir.to_string(),
        }
    }

    /// Run the MCP server loop, reading JSON-RPC requests from stdin
    /// and writing responses to stdout.
    pub fn run(&self) -> Result<()> {
        let stdin = io::stdin();
        let stdout = io::stdout();

        for line in stdin.lock().lines() {
            let line = line?;
            if line.trim().is_empty() {
                continue;
            }

            let request: serde_json::Value = match serde_json::from_str(&line) {
                Ok(v) => v,
                Err(e) => {
                    let err = jsonrpc_error(None, -32700, "Parse error", &e.to_string());
                    writeln!(io::stdout(), "{}", serde_json::to_string(&err)?)?;
                    stdout.lock().flush()?;
                    continue;
                }
            };

            let id = request.get("id").cloned();
            let method = request
                .get("method")
                .and_then(|m| m.as_str())
                .unwrap_or("")
                .to_string();
            let params = request.get("params").cloned().unwrap_or(Value::Null);

            let response = match method.as_str() {
                "initialize" => self.handle_initialize(id),
                "tools/list" => self.handle_tools_list(id),
                "tools/call" => self.handle_tools_call(id, &params),
                _ => jsonrpc_error(
                    id,
                    -32601,
                    "Method not found",
                    &format!("Unknown method: {method}"),
                ),
            };

            let output = serde_json::to_string(&response)?;
            writeln!(io::stdout(), "{output}")?;
            stdout.lock().flush()?;
        }

        Ok(())
    }

    // ── Handler: initialize ──────────────────────────────────────────────

    fn handle_initialize(&self, id: Option<Value>) -> Value {
        jsonrpc_result(
            id,
            serde_json::json!({
                "protocolVersion": "2024-11-05",
                "capabilities": {
                    "tools": {}
                },
                "serverInfo": {
                    "name": "loom-mcp-server",
                    "version": env!("CARGO_PKG_VERSION")
                }
            }),
        )
    }

    // ── Handler: tools/list ──────────────────────────────────────────────

    fn handle_tools_list(&self, id: Option<Value>) -> Value {
        let tools = vec![
            serde_json::json!({
                "name": "loom_list_skills",
                "description": "List all shared skills in the Loom ecosystem",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "tag": {
                            "type": "string",
                            "description": "Optional tag filter"
                        }
                    }
                }
            }),
            serde_json::json!({
                "name": "loom_get_skill",
                "description": "Get the content of a specific skill by name",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "name": {
                            "type": "string",
                            "description": "Skill name"
                        }
                    },
                    "required": ["name"]
                }
            }),
            serde_json::json!({
                "name": "loom_list_mcp_servers",
                "description": "List shared MCP servers registered in the ecosystem",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "tag": {
                            "type": "string",
                            "description": "Optional tag filter"
                        }
                    }
                }
            }),
            serde_json::json!({
                "name": "loom_get_mcp_config",
                "description": "Get the merged MCP server configuration for all shared servers",
                "inputSchema": {
                    "type": "object",
                    "properties": {}
                }
            }),
            serde_json::json!({
                "name": "loom_list_providers",
                "description": "List registered auth providers in the ecosystem",
                "inputSchema": {
                    "type": "object",
                    "properties": {}
                }
            }),
            serde_json::json!({
                "name": "loom_save_session",
                "description": "Export a session to the shared ecosystem store",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "session_id": {
                            "type": "string",
                            "description": "Session identifier"
                        },
                        "content": {
                            "type": "string",
                            "description": "Session content in JSONL format"
                        }
                    },
                    "required": ["session_id", "content"]
                }
            }),
            serde_json::json!({
                "name": "loom_get_credential",
                "description": "Get a stored credential for a provider (requires vault unlocked)",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "provider_id": {
                            "type": "string",
                            "description": "Provider identifier (e.g. anthropic, openai)"
                        }
                    },
                    "required": ["provider_id"]
                }
            }),
        ];

        jsonrpc_result(id, serde_json::json!({ "tools": tools }))
    }

    // ── Handler: tools/call ──────────────────────────────────────────────

    fn handle_tools_call(&self, id: Option<Value>, params: &Value) -> Value {
        let name = params
            .get("name")
            .and_then(|n| n.as_str())
            .unwrap_or("");
        let arguments = params
            .get("arguments")
            .cloned()
            .unwrap_or(Value::Null);

        match name {
            "loom_list_skills" => self.call_list_skills(id, &arguments),
            "loom_get_skill" => self.call_get_skill(id, &arguments),
            "loom_list_mcp_servers" => self.call_list_mcp_servers(id, &arguments),
            "loom_get_mcp_config" => self.call_get_mcp_config(id),
            "loom_list_providers" => self.call_list_providers(id),
            "loom_save_session" => self.call_save_session(id, &arguments),
            "loom_get_credential" => self.call_get_credential(id, &arguments),
            _ => jsonrpc_error(
                id,
                -32601,
                "Tool not found",
                &format!("Unknown tool: {name}"),
            ),
        }
    }

    // ── Tool Implementations ─────────────────────────────────────────────

    fn call_list_skills(&self, id: Option<Value>, args: &Value) -> Value {
        let tag_filter = args.get("tag").and_then(|t| t.as_str());

        match loom_skills::SkillsEngine::new(&self.loom_dir) {
            Ok(engine) => {
                let skills = engine.registry().list();
                let filtered: Vec<Value> = skills
                    .iter()
                    .filter(|s| {
                        tag_filter.is_none_or(|tag| s.tags.iter().any(|t| t == tag))
                    })
                    .map(|s| {
                        serde_json::json!({
                            "name": s.name,
                            "description": s.description,
                            "version": s.version,
                            "tags": s.tags,
                            "last_modified": s.last_modified,
                        })
                    })
                    .collect();

                jsonrpc_result(id, serde_json::json!({ "skills": filtered }))
            }
            Err(e) => jsonrpc_error(id, -32603, "Internal error", &e.to_string()),
        }
    }

    fn call_get_skill(&self, id: Option<Value>, args: &Value) -> Value {
        let name = match args.get("name").and_then(|n| n.as_str()) {
            Some(n) => n,
            None => return jsonrpc_error(id, -32602, "Invalid params", "Missing 'name'"),
        };

        match loom_skills::SkillsEngine::new(&self.loom_dir) {
            Ok(engine) => match engine.registry().get(name) {
                Some(skill) => {
                    jsonrpc_result(
                        id,
                        serde_json::json!({
                            "name": skill.name,
                            "description": skill.description,
                            "version": skill.version,
                            "tags": skill.tags,
                        }),
                    )
                }
                None => jsonrpc_error(id, -32602, "Skill not found", &format!("No skill named '{name}'")),
            },
            Err(e) => jsonrpc_error(id, -32603, "Internal error", &e.to_string()),
        }
    }

    fn call_list_mcp_servers(&self, id: Option<Value>, args: &Value) -> Value {
        let tag_filter = args.get("tag").and_then(|t| t.as_str());

        match loom_mcp::McpRegistry::new(&self.loom_dir) {
            Ok(registry) => {
                let servers = match tag_filter {
                    Some(tag) => registry.filter_by_tag(tag).unwrap_or_default(),
                    None => registry.filter_shared(true).unwrap_or_default(),
                };

                let result: Vec<Value> = servers
                    .iter()
                    .map(|s| {
                        serde_json::json!({
                            "id": s.id,
                            "name": s.name,
                            "transport_type": s.transport.transport_type,
                            "command": s.transport.command,
                            "args": s.transport.args,
                            "health_status": s.health_status,
                            "tags": s.tags,
                            "compatible_tools": s.compatible_tools,
                        })
                    })
                    .collect();

                jsonrpc_result(id, serde_json::json!({ "servers": result }))
            }
            Err(e) => jsonrpc_error(id, -32603, "Internal error", &e.to_string()),
        }
    }

    fn call_get_mcp_config(&self, id: Option<Value>) -> Value {
        match loom_mcp::McpRegistry::new(&self.loom_dir) {
            Ok(registry) => {
                let servers = match registry.filter_shared(true) {
                    Ok(s) => s,
                    Err(e) => {
                        return jsonrpc_error(id, -32603, "Internal error", &e.to_string());
                    }
                };

                let mut mcp_servers = serde_json::Map::new();
                for entry in &servers {
                    let server_obj = serde_json::json!({
                        "command": entry.transport.command,
                        "args": entry.transport.args,
                        "env": entry.transport.env,
                        "url": entry.transport.url,
                    });
                    mcp_servers.insert(entry.id.clone(), server_obj);
                }

                jsonrpc_result(
                    id,
                    serde_json::json!({
                        "mcpServers": mcp_servers,
                    }),
                )
            }
            Err(e) => jsonrpc_error(id, -32603, "Internal error", &e.to_string()),
        }
    }

    fn call_list_providers(&self, id: Option<Value>) -> Value {
        let auth_dir = Path::new(&self.loom_dir).join("auth");
        if !auth_dir.exists() {
            return jsonrpc_result(id, serde_json::json!({ "providers": [] }));
        }

        let auth = loom_auth::AuthManager::new(&self.loom_dir);
        let providers = auth.list_providers();
        let result: Vec<Value> = providers
            .iter()
            .map(|p| {
                serde_json::json!({
                    "provider_id": p.provider_id,
                    "name": p.name,
                    "auth_method": p.auth_method,
                    "status": p.status,
                    "supported_tools": p.supported_tools,
                })
            })
            .collect();

        jsonrpc_result(id, serde_json::json!({ "providers": result }))
    }

    fn call_save_session(&self, id: Option<Value>, args: &Value) -> Value {
        let session_id = match args.get("session_id").and_then(|s| s.as_str()) {
            Some(s) => s,
            None => {
                return jsonrpc_error(id, -32602, "Invalid params", "Missing 'session_id'");
            }
        };
        let content = match args.get("content").and_then(|c| c.as_str()) {
            Some(c) => c,
            None => {
                return jsonrpc_error(id, -32602, "Invalid params", "Missing 'content'");
            }
        };

        let sessions_dir = Path::new(&self.loom_dir).join("sessions");
        std::fs::create_dir_all(&sessions_dir).ok();

        let file_path = sessions_dir.join(format!("{session_id}.jsonl"));
        match std::fs::write(&file_path, content) {
            Ok(_) => jsonrpc_result(
                id,
                serde_json::json!({
                    "success": true,
                    "path": file_path.to_string_lossy(),
                }),
            ),
            Err(e) => jsonrpc_error(id, -32603, "Write failed", &e.to_string()),
        }
    }

    fn call_get_credential(&self, id: Option<Value>, args: &Value) -> Value {
        let provider_id = match args.get("provider_id").and_then(|p| p.as_str()) {
            Some(p) => p,
            None => {
                return jsonrpc_error(id, -32602, "Invalid params", "Missing 'provider_id'");
            }
        };

        let auth = loom_auth::AuthManager::new(&self.loom_dir);
        if auth.vault_is_locked() {
            return jsonrpc_error(
                id,
                -32000,
                "Vault locked",
                "Auth vault is locked. Run `loom auth unlock` first.",
            );
        }

        match auth.vault().get_credential(provider_id) {
            Ok(Some(plaintext)) => jsonrpc_result(
                id,
                serde_json::json!({
                    "provider_id": provider_id,
                    "credential": plaintext,
                }),
            ),
            Ok(None) => jsonrpc_error(
                id,
                -32602,
                "Credential not found",
                &format!("No credential for provider '{provider_id}'"),
            ),
            Err(e) => jsonrpc_error(id, -32603, "Internal error", &e.to_string()),
        }
    }
}

// ── JSON-RPC Helpers ──────────────────────────────────────────────────

fn jsonrpc_result(id: Option<Value>, result: Value) -> Value {
    serde_json::json!({
        "jsonrpc": "2.0",
        "id": id,
        "result": result,
    })
}

fn jsonrpc_error(id: Option<Value>, code: i32, message: &str, data: &str) -> Value {
    serde_json::json!({
        "jsonrpc": "2.0",
        "id": id,
        "error": {
            "code": code,
            "message": message,
            "data": data,
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_initialize_response() {
        let server = LoomMcpServer::new("/tmp/.loom");
        let resp = server.handle_initialize(Some(serde_json::json!(1)));
        assert_eq!(resp["jsonrpc"], "2.0");
        assert_eq!(resp["id"], 1);
        assert!(resp["result"]["capabilities"]["tools"].is_object());
        assert_eq!(resp["result"]["serverInfo"]["name"], "loom-mcp-server");
    }

    #[test]
    fn test_tools_list_contains_core_tools() {
        let server = LoomMcpServer::new("/tmp/.loom");
        let resp = server.handle_tools_list(Some(serde_json::json!(2)));
        let tools = resp["result"]["tools"].as_array().unwrap();
        let names: Vec<&str> = tools.iter().map(|t| t["name"].as_str().unwrap()).collect();
        assert!(names.contains(&"loom_list_skills"));
        assert!(names.contains(&"loom_get_skill"));
        assert!(names.contains(&"loom_list_mcp_servers"));
        assert!(names.contains(&"loom_get_mcp_config"));
        assert!(names.contains(&"loom_save_session"));
        assert!(names.contains(&"loom_list_providers"));
        assert!(names.contains(&"loom_get_credential"));
    }

    #[test]
    fn test_unknown_method() {
        let server = LoomMcpServer::new("/tmp/.loom");
        let resp = server.handle_initialize(Some(serde_json::json!(1)));
        // Using initialize to test the framework, then test unknown method path
        let params = serde_json::json!({"name": "nonexistent_tool"});
        let result = server.handle_tools_call(Some(serde_json::json!(3)), &params);
        assert!(result.get("error").is_some());
        assert_eq!(result["error"]["code"], -32601);
    }

    #[test]
    fn test_missing_params_return_error() {
        let server = LoomMcpServer::new("/tmp/.loom");
        let params = serde_json::json!({"name": "loom_get_skill", "arguments": {}});
        let result = server.handle_tools_call(Some(serde_json::json!(4)), &params);
        assert!(result.get("error").is_some());
        assert_eq!(result["error"]["code"], -32602);
    }

    #[test]
    fn test_save_session_missing_content() {
        let server = LoomMcpServer::new("/tmp/.loom");
        let params = serde_json::json!({
            "name": "loom_save_session",
            "arguments": {"session_id": "test-123"}
        });
        let result = server.handle_tools_call(Some(serde_json::json!(5)), &params);
        assert!(result.get("error").is_some());
    }

    #[test]
    fn test_jsonrpc_error_format() {
        let err = jsonrpc_error(Some(serde_json::json!(1)), -32700, "Parse error", "bad json");
        assert_eq!(err["jsonrpc"], "2.0");
        assert_eq!(err["error"]["code"], -32700);
        assert_eq!(err["error"]["message"], "Parse error");
    }

    #[test]
    fn test_jsonrpc_result_format() {
        let res = jsonrpc_result(
            Some(serde_json::json!(1)),
            serde_json::json!({"status": "ok"}),
        );
        assert_eq!(res["jsonrpc"], "2.0");
        assert_eq!(res["result"]["status"], "ok");
    }
}
