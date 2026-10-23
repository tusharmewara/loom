//! gRPC server for Loom daemon remote control.

use anyhow::{Context, Result};
use async_stream::stream;
use loom_core::{HealthCheck, LoomEvent};
use loom_event_bus::EventBus;
use loom_workflow::WorkflowEngine;
use std::path::Path;
use std::sync::Arc;
use tonic::{Request, Response, Status};

// ponytail: tonic-generated signatures must return Result<_, Status> (trait-mandated);
// clippy 1.99's result_large_err flags them — boxing would break the impl.
#[allow(clippy::result_large_err)]
pub mod loom_grpc {
    include!(concat!(env!("OUT_DIR"), "/loom.rs"));
}

use loom_grpc::{
    loom_service_server::{LoomService, LoomServiceServer},
    *,
};

use std::pin::Pin;
use tokio_stream::Stream;

type WatchEventsStream = Pin<Box<dyn Stream<Item = Result<Event, Status>> + Send>>;

/// Configuration for the gRPC server.
#[derive(Clone, Debug)]
pub struct GrpcConfig {
    pub listen_addr: String,
    pub enable_tls: bool,
    pub cert_path: Option<String>,
    pub key_path: Option<String>,
}

impl Default for GrpcConfig {
    fn default() -> Self {
        Self {
            listen_addr: "127.0.0.1:50051".to_string(),
            enable_tls: false,
            cert_path: None,
            key_path: None,
        }
    }
}

/// gRPC service implementation.
#[derive(Clone)]
pub struct LoomGrpcService {
    loom_dir: String,
    event_bus: Arc<EventBus>,
}

impl LoomGrpcService {
    pub fn new(loom_dir: &str, event_bus: Arc<EventBus>) -> Self {
        Self {
            loom_dir: loom_dir.to_string(),
            event_bus,
        }
    }

    fn workflow_engine(&self) -> WorkflowEngine {
        WorkflowEngine::new(&self.loom_dir)
    }
}

#[tonic::async_trait]
impl LoomService for LoomGrpcService {
    type WatchEventsStream = WatchEventsStream;

    // === Health ===
    async fn health_check(
        &self,
        _: Request<HealthCheckRequest>,
    ) -> Result<Response<HealthCheckResponse>, Status> {
        let engine = self.workflow_engine();
        let health = engine
            .health()
            .map_err(|e| Status::internal(e.to_string()))?;
        Ok(Response::new(HealthCheckResponse {
            crate_name: health.crate_name.to_string(),
            version: health.version.to_string(),
            uptime_seconds: health.uptime_seconds as i64,
            last_error: health.last_error.unwrap_or_default(),
            performance_counters: health
                .performance_counters
                .into_iter()
                .map(|(k, v)| (k, v as i64))
                .collect(),
        }))
    }

    // === Tasks ===
    async fn create_task(
        &self,
        req: Request<CreateTaskRequest>,
    ) -> Result<Response<CreateTaskResponse>, Status> {
        let req = req.into_inner();
        let engine = self.workflow_engine();
        let orch = engine.orchestrator();
        let task = orch
            .create_task(&req.title, &req.description, req.worktree, None)
            .map_err(|e| Status::internal(e.to_string()))?;

        let (complexity, routing) =
            if let (Some(c), Some(r)) = (&task.complexity_score, &task.routing_decision) {
                (
                    serde_json::to_string(c).unwrap_or_default(),
                    serde_json::to_string(r).unwrap_or_default(),
                )
            } else {
                (String::new(), String::new())
            };

        Ok(Response::new(CreateTaskResponse {
            task_id: task.task_id,
            title: task.title,
            description: task.description,
            phase: format!("{:?}", task.phase),
            status: format!("{:?}", task.status),
            created_at: task.created_at,
            updated_at: task.updated_at,
            worktree_path: task.worktree_path.unwrap_or_default(),
            complexity_score: complexity,
            routing_decision: routing,
        }))
    }

    async fn get_task(
        &self,
        req: Request<GetTaskRequest>,
    ) -> Result<Response<GetTaskResponse>, Status> {
        let engine = self.workflow_engine();
        let orch = engine.orchestrator();
        let task = orch
            .get_task(&req.into_inner().task_id)
            .map_err(|e| Status::internal(e.to_string()))?
            .ok_or_else(|| Status::not_found("Task not found"))?;

        Ok(Response::new(GetTaskResponse {
            task_json: serde_json::to_string(&task).unwrap_or_default(),
        }))
    }

    async fn list_tasks(
        &self,
        _: Request<ListTasksRequest>,
    ) -> Result<Response<ListTasksResponse>, Status> {
        let engine = self.workflow_engine();
        let orch = engine.orchestrator();
        let tasks = orch
            .list_tasks()
            .map_err(|e| Status::internal(e.to_string()))?;

        let summaries = tasks
            .into_iter()
            .map(|t| TaskSummary {
                task_id: t.task_id,
                title: t.title,
                phase: format!("{:?}", t.phase),
                status: format!("{:?}", t.status),
                worktree_path: t.worktree_path.unwrap_or_default(),
            })
            .collect();

        Ok(Response::new(ListTasksResponse { tasks: summaries }))
    }

    async fn advance_phase(
        &self,
        req: Request<AdvancePhaseRequest>,
    ) -> Result<Response<AdvancePhaseResponse>, Status> {
        let engine = self.workflow_engine();
        let orch = engine.orchestrator();
        let task = orch
            .advance_phase(&req.into_inner().task_id)
            .map_err(|e| Status::internal(e.to_string()))?;

        Ok(Response::new(AdvancePhaseResponse {
            task_json: serde_json::to_string(&task).unwrap_or_default(),
        }))
    }

    async fn remove_task(
        &self,
        req: Request<RemoveTaskRequest>,
    ) -> Result<Response<RemoveTaskResponse>, Status> {
        let engine = self.workflow_engine();
        let orch = engine.orchestrator();
        orch.remove_task(&req.into_inner().task_id)
            .map_err(|e| Status::internal(e.to_string()))?;

        Ok(Response::new(RemoveTaskResponse { success: true }))
    }

    async fn assign_routing(
        &self,
        req: Request<AssignRoutingRequest>,
    ) -> Result<Response<AssignRoutingResponse>, Status> {
        let req = req.into_inner();
        let engine = self.workflow_engine();
        let orch = engine.orchestrator();
        let task = orch
            .assign_routing_with_overrides(
                &req.task_id,
                if req.provider.is_empty() {
                    None
                } else {
                    Some(&req.provider)
                },
                if req.model.is_empty() {
                    None
                } else {
                    Some(&req.model)
                },
            )
            .map_err(|e| Status::internal(e.to_string()))?;

        Ok(Response::new(AssignRoutingResponse {
            task_json: serde_json::to_string(&task).unwrap_or_default(),
        }))
    }

    // === Skills ===
    async fn list_skills(
        &self,
        _: Request<ListSkillsRequest>,
    ) -> Result<Response<ListSkillsResponse>, Status> {
        let skills_engine = loom_skills::SkillsEngine::new(&self.loom_dir)
            .map_err(|e| Status::internal(e.to_string()))?;
        let skills = skills_engine.registry().list();

        let summaries = skills
            .into_iter()
            .map(|s| SkillSummary {
                name: s.name,
                description: s.description,
                version: s.version,
                path: s.path,
            })
            .collect();

        Ok(Response::new(ListSkillsResponse { skills: summaries }))
    }

    async fn get_skill(
        &self,
        req: Request<GetSkillRequest>,
    ) -> Result<Response<GetSkillResponse>, Status> {
        let skills_engine = loom_skills::SkillsEngine::new(&self.loom_dir)
            .map_err(|e| Status::internal(e.to_string()))?;
        let skill = skills_engine
            .registry()
            .get(&req.into_inner().name)
            .ok_or_else(|| Status::not_found("Skill not found"))?;

        Ok(Response::new(GetSkillResponse {
            skill_json: serde_json::to_string(&skill).unwrap_or_default(),
        }))
    }

    // === MCP ===
    async fn list_mcp(
        &self,
        _: Request<ListMcpRequest>,
    ) -> Result<Response<ListMcpResponse>, Status> {
        let mcp_mgr = loom_mcp::McpManager::new(&self.loom_dir)
            .map_err(|e| Status::internal(e.to_string()))?;
        let mcps = mcp_mgr
            .registry()
            .list()
            .map_err(|e| Status::internal(e.to_string()))?;

        let summaries = mcps
            .into_iter()
            .map(|m| McpSummary {
                id: m.id,
                name: m.name,
                command: m.transport.command.unwrap_or_default(),
                args: m.transport.args,
                status: format!("{:?}", m.health_status),
            })
            .collect();

        Ok(Response::new(ListMcpResponse { mcps: summaries }))
    }

    async fn mcp_health(
        &self,
        _: Request<McpHealthRequest>,
    ) -> Result<Response<McpHealthResponse>, Status> {
        let mcp_mgr = loom_mcp::McpManager::new(&self.loom_dir)
            .map_err(|e| Status::internal(e.to_string()))?;
        let snapshot = mcp_mgr.health_checker().status_snapshot().await;

        let entries = snapshot
            .into_iter()
            .map(|(id, _name, status)| McpHealthEntry {
                id,
                status: format!("{:?}", status),
                last_check: chrono::Utc::now().to_rfc3339(),
                error: String::new(),
            })
            .collect();

        Ok(Response::new(McpHealthResponse { health: entries }))
    }

    // === Sessions ===
    async fn list_sessions(
        &self,
        _: Request<ListSessionsRequest>,
    ) -> Result<Response<ListSessionsResponse>, Status> {
        let sessions = loom_sessions::SessionManager::new(&self.loom_dir)
            .store()
            .list();
        let summaries = sessions
            .into_iter()
            .map(|s| SessionSummary {
                id: s.session_id,
                title: s.topic,
                status: "Completed".to_string(),
                message_count: s.message_count as i64,
                created_at: s.exported_at,
            })
            .collect();

        Ok(Response::new(ListSessionsResponse {
            sessions: summaries,
        }))
    }

    async fn get_session(
        &self,
        req: Request<GetSessionRequest>,
    ) -> Result<Response<GetSessionResponse>, Status> {
        let sessions = loom_sessions::SessionManager::new(&self.loom_dir);
        let session = sessions
            .store()
            .get(&req.into_inner().id)
            .ok_or_else(|| Status::not_found("Session not found"))?;

        Ok(Response::new(GetSessionResponse {
            session_json: serde_json::to_string(&session).unwrap_or_default(),
        }))
    }

    async fn export_session(
        &self,
        req: Request<ExportSessionRequest>,
    ) -> Result<Response<ExportSessionResponse>, Status> {
        let id = req.into_inner().id;
        let store = loom_sessions::store::SessionStore::new(&self.loom_dir);
        let export = loom_sessions::export_session(&store, &id)
            .map_err(|e| Status::internal(e.to_string()))?;

        // Write to default exports directory
        let out_path = Path::new(&self.loom_dir)
            .join("sessions")
            .join("exports")
            .join(format!("{}.json", id));
        loom_sessions::write_export(&export, &out_path)
            .map_err(|e| Status::internal(e.to_string()))?;

        Ok(Response::new(ExportSessionResponse {
            export_path: out_path.to_string_lossy().to_string(),
        }))
    }

    // === Config ===
    async fn get_config(
        &self,
        _: Request<GetConfigRequest>,
    ) -> Result<Response<GetConfigResponse>, Status> {
        let config = loom_core::config::ConfigReader::new(&self.loom_dir)
            .load()
            .map_err(|e| Status::internal(e.to_string()))?;

        Ok(Response::new(GetConfigResponse {
            config_json: serde_json::to_string(&config).unwrap_or_default(),
        }))
    }

    // === Events ===
    async fn watch_events(
        &self,
        _: Request<WatchEventsRequest>,
    ) -> Result<Response<WatchEventsStream>, Status> {
        let mut rx = self.event_bus.subscribe();
        let stream = stream! {
            while let Ok(event) = rx.recv().await {
                let (event_type, payload_json) = match &event {
                    LoomEvent::SkillsAdded { count } => (
                        "SkillsAdded".to_string(),
                        serde_json::to_string(&serde_json::json!({ "count": count })).unwrap_or_default(),
                    ),
                    LoomEvent::SkillsRemoved { name } => (
                        "SkillsRemoved".to_string(),
                        serde_json::to_string(&serde_json::json!({ "name": name })).unwrap_or_default(),
                    ),
                    LoomEvent::McpHealthChanged { id, status } => (
                        "McpHealthChanged".to_string(),
                        serde_json::to_string(&serde_json::json!({ "id": id, "status": format!("{:?}", status) })).unwrap_or_default(),
                    ),
                    LoomEvent::McpServerAdded { id, name } => (
                        "McpServerAdded".to_string(),
                        serde_json::to_string(&serde_json::json!({ "id": id, "name": name })).unwrap_or_default(),
                    ),
                    LoomEvent::SessionExported { session_id } => (
                        "SessionExported".to_string(),
                        serde_json::to_string(&serde_json::json!({ "session_id": session_id })).unwrap_or_default(),
                    ),
                    LoomEvent::RoutingUpdated => (
                        "RoutingUpdated".to_string(),
                        String::new(),
                    ),
                    LoomEvent::AuthProviderAdded { provider_id } => (
                        "AuthProviderAdded".to_string(),
                        serde_json::to_string(&serde_json::json!({ "provider_id": provider_id })).unwrap_or_default(),
                    ),
                    LoomEvent::HooksRegistered { hook_id } => (
                        "HooksRegistered".to_string(),
                        serde_json::to_string(&serde_json::json!({ "hook_id": hook_id })).unwrap_or_default(),
                    ),
                    LoomEvent::TaskPhaseChanged { task_id, phase } => (
                        "TaskPhaseChanged".to_string(),
                        serde_json::to_string(&serde_json::json!({ "task_id": task_id, "phase": format!("{:?}", phase) })).unwrap_or_default(),
                    ),
                    LoomEvent::TaskCreated { task_id } => (
                        "TaskCreated".to_string(),
                        serde_json::to_string(&serde_json::json!({ "task_id": task_id })).unwrap_or_default(),
                    ),
                    LoomEvent::TaskRunCompleted { task_id, run_id, status, session_id } => (
                        "TaskRunCompleted".to_string(),
                        serde_json::to_string(&serde_json::json!({ "task_id": task_id, "run_id": run_id, "status": status, "session_id": session_id })).unwrap_or_default(),
                    ),
                };
                yield Ok(Event {
                    event_type,
                    payload_json,
                    timestamp: std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .map(|d| d.as_secs())
                        .unwrap_or(0) as i64,
                });
            }
        };
        Ok(Response::new(Box::pin(stream)))
    }
}

/// Start the gRPC server.
pub async fn start_grpc_server(config: GrpcConfig, service: LoomGrpcService) -> Result<()> {
    let addr = config
        .listen_addr
        .parse()
        .context("Invalid gRPC listen address")?;

    let svc = LoomServiceServer::new(service);

    let mut server = tonic::transport::Server::builder();

    if config.enable_tls {
        let cert = std::fs::read_to_string(config.cert_path.as_ref().unwrap())
            .context("Failed to read TLS cert")?;
        let key = std::fs::read_to_string(config.key_path.as_ref().unwrap())
            .context("Failed to read TLS key")?;
        let identity = tonic::transport::Identity::from_pem(cert, key);
        server = server
            .tls_config(tonic::transport::ServerTlsConfig::new().identity(identity))
            .context("Invalid TLS config")?;
    }

    let router = server.add_service(svc);

    tracing::info!("Starting gRPC server on {}", addr);
    router.serve(addr).await.context("gRPC server failed")?;
    Ok(())
}
