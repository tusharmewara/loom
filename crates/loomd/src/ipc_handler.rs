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
}

impl LoomdIpcHandler {
    pub fn new(loomd: &Loomd) -> Self {
        Self {
            version: env!("CARGO_PKG_VERSION"),
            loom_dir: loomd.loom_dir().to_string(),
            discovery: loomd.discovery().clone(),
            skills: loomd.skills().clone(),
            sessions: loomd.sessions().clone(),
            auth: loomd.auth().clone(),
            hooks: loomd.hooks().clone(),
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
            "mcp.list" => match loom_mcp::McpRegistry::new(&self.loom_dir) {
                Ok(registry) => match registry.list() {
                    Ok(servers) => IpcResponse::ok(req.request_id, json!({ "servers": servers })),
                    Err(e) => IpcResponse::error(req.request_id, e.to_string()),
                },
                Err(e) => IpcResponse::error(req.request_id, e.to_string()),
            },
            "mcp.health" => match loom_mcp::McpRegistry::new(&self.loom_dir) {
                Ok(registry) => match registry.list() {
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
                },
                Err(e) => IpcResponse::error(req.request_id, e.to_string()),
            },
            "sessions.list" => {
                let sessions = self.sessions.store().list();
                IpcResponse::ok(req.request_id, json!({ "sessions": sessions }))
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
            _ => IpcResponse::error(req.request_id, format!("Unknown command: {}", req.command)),
        }
    }
}
