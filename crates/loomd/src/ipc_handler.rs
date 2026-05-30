use crate::Loomd;
use loom_core::HealthCheck;
use loom_ipc::{IpcHandler, IpcRequest, IpcResponse};
use serde_json::json;

pub struct LoomdIpcHandler {
    version: &'static str,
    loom_dir: String,
    discovery: std::sync::Arc<loom_discovery::DiscoveryEngine>,
    skills: std::sync::Arc<loom_skills::SkillsEngine>,
    sessions: std::sync::Arc<loom_sessions::SessionManager>,
    auth: std::sync::Arc<loom_auth::AuthManager>,
    hooks: std::sync::Arc<loom_hooks::HooksEngine>,
    mcp_registry: loom_mcp::McpRegistry,
    memory_mgr: std::sync::Arc<loom_mcp::AgentMemoryManager>,
}

impl LoomdIpcHandler {
    pub fn new(loomd: &Loomd) -> Self {
        let mcp_registry = loom_mcp::McpRegistry::new(loomd.loom_dir()).unwrap();
        Self {
            version: env!("CARGO_PKG_VERSION"),
            loom_dir: loomd.loom_dir().to_string(),
            discovery: loomd.discovery().clone(),
            mcp_registry,
            skills: loomd.skills().clone(),
            sessions: loomd.sessions().clone(),
            auth: loomd.auth().clone(),
            hooks: loomd.hooks().clone(),
            memory_mgr: loomd.memory_mgr().clone(),
        }
    }
}

impl IpcHandler for LoomdIpcHandler {
    fn handle(&self, req: IpcRequest) -> IpcResponse {
        match req.command.as_str() {
            "health.check" => {
                let health = json!({
                    "status": "healthy",
                    "version": self.version,
                    "subsystems": {
                        "discovery": self.discovery.health().is_ok(),
                        "skills": self.skills.health().is_ok(),
                        "sessions": self.sessions.health().is_ok(),
                        "auth": self.auth.health().is_ok(),
                        "hooks": self.hooks.health().is_ok(),
                    }
                });
                IpcResponse::ok(req.request_id, health)
            }
            "discovery.scan" => match self.discovery.run() {
                Ok(result) => IpcResponse::ok(req.request_id, json!(result)),
                Err(e) => IpcResponse::error(req.request_id, e.to_string()),
            },
            "skills.list" => {
                let skills = self.skills.registry().list();
                IpcResponse::ok(req.request_id, json!({ "skills": skills }))
            }
            "skills.show" => {
                let name = req
                    .params
                    .get("name")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                match self.skills.registry().get(name) {
                    Some(skill) => {
                        IpcResponse::ok(req.request_id, json!({ "skill": skill, "found": true }))
                    }
                    None => {
                        IpcResponse::ok(req.request_id, json!({ "skill": null, "found": false }))
                    }
                }
            }
            "mcp.list" => {
                match self.mcp_registry.list() {
                    Ok(servers) => IpcResponse::ok(req.request_id, json!({ "servers": servers })),
                    Err(e) => IpcResponse::error(req.request_id, e.to_string()),
                }
            }
            "mcp.health" => {
                match self.mcp_registry.list() {
                    Ok(servers) => {
                        let all_healthy = servers.iter().all(|s| {
                            matches!(s.health_status, loom_core::HealthStatusStr::Healthy)
                        });
                        IpcResponse::ok(
                            req.request_id,
                            json!({ "servers": servers, "all_healthy": all_healthy }),
                        )
                    }
                    Err(e) => IpcResponse::error(req.request_id, e.to_string()),
                }
            }
            "sessions.list" => {
                let sessions = self.sessions.store().list();
                IpcResponse::ok(req.request_id, json!({ "sessions": sessions }))
            }
            "sessions.search" => {
                let query = req.params.get("query").and_then(|v| v.as_str()).unwrap_or("");
                let limit = req.params.get("limit").and_then(|v| v.as_u64()).unwrap_or(10) as usize;
                if query.is_empty() {
                    return IpcResponse::error(req.request_id, "query parameter required");
                }
                match loom_sessions::memory::CrossSessionMemory::new(&self.loom_dir) {
                    Ok(memory) => match memory.search(query, limit) {
                        Ok(results) => IpcResponse::ok(req.request_id, json!({ "results": results, "count": results.len(), "query": query })),
                        Err(e) => IpcResponse::error(req.request_id, e.to_string()),
                    },
                    Err(e) => IpcResponse::error(req.request_id, e.to_string()),
                }
            }
            "sessions.index" => {
                match loom_sessions::memory::CrossSessionMemory::new(&self.loom_dir) {
                    Ok(memory) => {
                        let sessions = self.sessions.store().list();
                        let mut total = 0usize;
                        for manifest in &sessions {
                            if let Ok(messages) = self.sessions.store().load_messages(&manifest.session_id) {
                                if !messages.is_empty() {
                                    if let Err(e) = memory.index_session(
                                        &manifest.session_id,
                                        &messages,
                                        &manifest.topic,
                                    ) {
                                        tracing::warn!("Failed to index session '{}': {e}", manifest.session_id);
                                    } else {
                                        total += messages.len();
                                    }
                                }
                            }
                        }
                        IpcResponse::ok(req.request_id, json!({ "indexed_messages": total, "sessions_count": sessions.len() }))
                    }
                    Err(e) => IpcResponse::error(req.request_id, e.to_string()),
                }
            }
            "auth.list" => {
                let providers = self.auth.list_providers();
                IpcResponse::ok(req.request_id, json!({ "providers": providers }))
            }
            "hooks.list" => {
                let runtime = tokio::runtime::Runtime::new().unwrap();
                let hooks = runtime.block_on(self.hooks.registry().list());
                IpcResponse::ok(req.request_id, json!({ "hooks": hooks }))
            }
            "version" => IpcResponse::ok(req.request_id, json!({ "version": self.version })),
            "config.show" => {
                let reader = loom_core::config::ConfigReader::new(&self.loom_dir);
                match reader.load() {
                    Ok(config) => IpcResponse::ok(req.request_id, json!(config)),
                    Err(e) => IpcResponse::error(req.request_id, e.to_string()),
                }
            }
            "register_tool" => {
                let name = req
                    .params
                    .get("name")
                    .and_then(|v| v.as_str())
                    .unwrap_or("unknown");
                let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".into());
                let mut features = vec![];
                if let Some(adapter) = loom_adapters::adapter_for(name) {
                    let _ = loom_adapters::write_tool_registration(
                        adapter.as_ref(),
                        &self.loom_dir,
                        &home,
                    );
                    let _ = adapter.sync_from_native(&self.loom_dir, &home);
                    if let Ok(ga) =
                        loom_generic_adapter::GenericAdapter::new(&self.loom_dir, name)
                    {
                        features = ga
                            .build()
                            .present_features()
                            .into_iter()
                            .map(str::to_string)
                            .collect();
                    }
                }
                IpcResponse::ok(
                    req.request_id,
                    json!({ "tool": name, "features": features }),
                )
            }
            "touch" => IpcResponse::ok(req.request_id, json!({ "ok": true })),
            "memory.status" => {
                let running = self.memory_mgr.is_running();
                IpcResponse::ok(
                    req.request_id,
                    json!({
                        "running": running,
                        "port": loom_mcp::AGENTMEMORY_PORT,
                        "viewer_port": loom_mcp::AGENTMEMORY_VIEWER_PORT
                    }),
                )
            }
            "memory.start" => {
                match self.memory_mgr.start() {
                    Ok(true) => IpcResponse::ok(req.request_id, json!({ "started": true })),
                    Ok(false) => IpcResponse::ok(req.request_id, json!({ "started": false, "reason": "already running" })),
                    Err(e) => IpcResponse::error(req.request_id, format!("failed to start: {e}")),
                }
            }
            "memory.stop" => {
                match self.memory_mgr.stop() {
                    Ok(true) => IpcResponse::ok(req.request_id, json!({ "stopped": true })),
                    Ok(false) => IpcResponse::ok(req.request_id, json!({ "stopped": false, "reason": "not running" })),
                    Err(e) => IpcResponse::error(req.request_id, format!("failed to stop: {e}")),
                }
            }
            "acp.dispatch" => {
                let task_id = req.params.get("task_id").and_then(|v| v.as_str()).unwrap_or("");
                let agent_id = req.params.get("agent_id").and_then(|v| v.as_str()).unwrap_or("");
                if task_id.is_empty() || agent_id.is_empty() {
                    return IpcResponse::error(req.request_id, "task_id and agent_id required");
                }
                IpcResponse::error(req.request_id, "acp.dispatch requires daemon IPC - not yet wired")
            }
            _ => IpcResponse::error(req.request_id, format!("Unknown command: {}", req.command)),
        }
    }
}
