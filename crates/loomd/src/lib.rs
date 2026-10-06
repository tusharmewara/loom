mod activity;
mod capture;
mod grpc_server;
mod ipc_handler;
mod run_manager;
use activity::{now_ms, ActivityHandler};
use loom_core::{constants::DAEMON_DEFAULT_IDLE_TIMEOUT_MS, HealthCheck, LoomEvent};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime};
use tokio::sync::{broadcast, RwLock};

pub struct Loomd {
    loom_dir: String,
    #[allow(dead_code)]
    event_bus: Arc<loom_event_bus::EventBus>,
    discovery: Arc<loom_discovery::DiscoveryEngine>,
    #[allow(clippy::arc_with_non_send_sync)]
    mcp: Arc<loom_mcp::McpManager>,
    skills: Arc<loom_skills::SkillsEngine>,
    sessions: Arc<loom_sessions::SessionManager>,
    auth: Arc<loom_auth::AuthManager>,
    hooks: Arc<loom_hooks::HooksEngine>,
    telemetry: Arc<loom_telemetry::TelemetryCollector>,
    memory_mgr: Arc<loom_mcp::AgentMemoryManager>,
    acp_server: Arc<RwLock<Option<Arc<loom_sessions::AcpDispatchServer>>>>,
    run_manager: Arc<Mutex<run_manager::RunManager>>,
    shutdown_tx: broadcast::Sender<()>,
}

impl Loomd {
    #[allow(clippy::arc_with_non_send_sync)]
    pub fn new(loom_dir: &str) -> anyhow::Result<Self> {
        // Create ~/.loom directory tree if it doesn't exist
        if !std::path::Path::new(loom_dir).exists() {
            loom_discovery::initializer::SnapshotBuilder::new(loom_dir)
                .create_agents_tree()
                .map_err(|e| anyhow::anyhow!("Failed to create loom directory: {}", e))?;
        }

        let event_bus = loom_event_bus::EventBus::with_defaults(loom_dir).as_arc();
        let telemetry = Arc::new(loom_telemetry::TelemetryCollector::new(loom_dir));
        telemetry.init_tracing();
        let (shutdown_tx, _) = broadcast::channel(1);

        let memory_mgr = Arc::new(loom_mcp::AgentMemoryManager::new(loom_dir));

        Ok(Self {
            loom_dir: loom_dir.to_string(),
            event_bus: event_bus.clone(),
            discovery: Arc::new(loom_discovery::DiscoveryEngine::new(loom_dir)),
            mcp: Arc::new(loom_mcp::McpManager::new(loom_dir)?),
            skills: Arc::new(loom_skills::SkillsEngine::new(loom_dir)?),
            sessions: Arc::new(loom_sessions::SessionManager::new(loom_dir)),
            auth: Arc::new(loom_auth::AuthManager::new(loom_dir)),
            hooks: Arc::new(loom_hooks::HooksEngine::new(loom_dir)),
            telemetry: telemetry.clone(),
            memory_mgr,
            acp_server: Arc::new(RwLock::new(None)),
            run_manager: Arc::new(Mutex::new(run_manager::RunManager::with_dir(
                std::path::Path::new(loom_dir).join("runs"),
            ))),
            shutdown_tx,
        })
    }

    pub fn loom_dir(&self) -> &str {
        &self.loom_dir
    }

    pub fn discovery(&self) -> &Arc<loom_discovery::DiscoveryEngine> {
        &self.discovery
    }

    pub fn mcp(&self) -> &Arc<loom_mcp::McpManager> {
        &self.mcp
    }

    pub fn skills(&self) -> &Arc<loom_skills::SkillsEngine> {
        &self.skills
    }

    pub fn sessions(&self) -> &Arc<loom_sessions::SessionManager> {
        &self.sessions
    }

    pub fn auth(&self) -> &Arc<loom_auth::AuthManager> {
        &self.auth
    }

    pub fn hooks(&self) -> &Arc<loom_hooks::HooksEngine> {
        &self.hooks
    }

    pub fn memory_mgr(&self) -> &Arc<loom_mcp::AgentMemoryManager> {
        &self.memory_mgr
    }

    pub async fn acp_server(&self) -> Option<Arc<loom_sessions::AcpDispatchServer>> {
        self.acp_server.read().await.clone()
    }

    /// Access to the inner RwLock for IPC handler construction.
    pub fn acp_server_inner(&self) -> &Arc<RwLock<Option<Arc<loom_sessions::AcpDispatchServer>>>> {
        &self.acp_server
    }

    pub fn run_manager(&self) -> &Arc<Mutex<run_manager::RunManager>> {
        &self.run_manager
    }

    pub async fn run(&self) -> anyhow::Result<()> {
        use loom_ipc::serve;
        use std::path::Path;

        tracing::info!("Loom daemon starting...");

        // Health check all subsystems
        for (name, health) in &[
            ("discovery", self.discovery.health()),
            ("mcp", self.mcp.health()),
            ("skills", self.skills.health()),
            ("sessions", self.sessions.health()),
            ("auth", self.auth.health()),
            ("hooks", self.hooks.health()),
            ("telemetry", self.telemetry.health()),
        ] {
            match health {
                Ok(status) => tracing::info!("  ✅ {} v{}", name, status.version),
                Err(e) => tracing::warn!("  ⚠️  {} health check failed: {}", name, e),
            }
        }

        // ── Auto-start agentmemory memory server ─────────────────────────
        // This gives every tool persistent memory via the agentmemory MCP
        // shim that is registered as a shared server in seed_defaults().
        // If it fails (npx not installed, etc.), loomd continues gracefully.
        match self.memory_mgr.start() {
            Ok(true) => tracing::info!("  ✅ agentmemory memory server started"),
            Ok(false) => tracing::info!("  ✓ agentmemory already running"),
            Err(e) => tracing::warn!(
                "  ⚠️  agentmemory not available: {e}\n       \
                 Install: npm install -g @agentmemory/agentmemory\n       \
                 Memory features will not be available until started manually."
            ),
        }

        // ── Warm cross-session memory store ──────────────────────────────
        // Index existing sessions into the FTS5 memory store on startup
        // so cross-session search is immediately available.
        let memory_loom_dir = self.loom_dir.clone();
        let sessions = self.sessions.clone();
        tokio::spawn(async move {
            match loom_sessions::memory::CrossSessionMemory::new(&memory_loom_dir) {
                Ok(memory) => {
                    let manifests = sessions.store().list();
                    let mut total = 0usize;
                    for manifest in &manifests {
                        if let Ok(messages) = sessions.store().load_messages(&manifest.session_id) {
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
                    if total > 0 {
                        tracing::info!("  ✅ Indexed {} messages from {} sessions into cross-session memory", total, manifests.len());
                    } else {
                        tracing::info!("  ✓ No sessions to index into cross-session memory");
                    }
                }
                Err(e) => tracing::warn!("  ⚠️  Failed to open cross-session memory store: {e}"),
            }
        });

        // ── ACP Dispatch Server ─────────────────────────────────────────
        // Start the TCP server for agent-to-agent dispatch.
        let acp_config = loom_sessions::AcpServerConfig {
            key_path: format!("{}/acp_server_key.txt", self.loom_dir),
            ..Default::default()
        };
        match loom_sessions::AcpDispatchServer::new(acp_config) {
            Ok(server) => {
                let server_arc = Arc::new(server);

                // Register a default dispatch handler using ACP protocol.
                // Runs initialize → session/new → session/prompt and captures
                // the transcript as a Loom session.
                let loom_dir = self.loom_dir.clone();
                let run_manager = self.run_manager.clone();
                let default_handler: loom_sessions::DispatchFn = Arc::new(move |req: &loom_sessions::DispatchRequest| {
                    let prompt = req.payload.get("prompt").and_then(|v| v.as_str()).unwrap_or("");
                    let timeout_secs = req.payload.get("timeout_secs").and_then(|v| v.as_u64()).unwrap_or(120);
                    crate::capture::dispatch_acp_protocol(
                        &loom_dir,
                        &run_manager,
                        &req.agent_id,
                        &req.task_id,
                        prompt,
                        timeout_secs,
                    )
                    .map_err(|e| anyhow::anyhow!("Dispatch failed: {e}"))
                });

                // Set the default handler on the server before it starts
                let server_arc_pre = server_arc.clone();
                server_arc_pre.set_default_handler(Some(default_handler)).await;

                let server_arc_clone = server_arc.clone();
                let loom_dir_clone = self.loom_dir.clone();
                let acp_server_lock = self.acp_server.clone();

                tokio::spawn(async move {
                    match server_arc_clone.start().await {
                        Ok(addr) => {
                            tracing::info!("  ✅ ACP dispatch server listening on {addr} (key: {})",
                                server_arc_clone.public_key_hex().chars().take(16).collect::<String>());
                            let info = serde_json::json!({
                                "listen_addr": addr.to_string(),
                                "public_key": server_arc_clone.public_key_hex(),
                                "started_at": SystemTime::now()
                                    .duration_since(SystemTime::UNIX_EPOCH)
                                    .map(|d| d.as_secs())
                                    .unwrap_or(0),
                            });
                            let _ = std::fs::write(
                                format!("{loom_dir_clone}/acp_server.json"),
                                serde_json::to_string_pretty(&info).unwrap_or_default(),
                            );
                        }
                        Err(e) => {
                            tracing::warn!("  ⚠️  ACP dispatch server failed to start: {e}");
                        }
                    }
                });

                // Store the server reference (synchronously, before IPC handler starts)
                let mut guard = acp_server_lock.write().await;
                *guard = Some(server_arc.clone());
            }
            Err(e) => {
                tracing::warn!("  ⚠️  ACP dispatch server creation failed: {e}");
            }
        }

        let socket_path = Path::new(&self.loom_dir).join("loomd.sock");
        let socket_path_str = socket_path.to_str().unwrap().to_string();

        let last_activity = Arc::new(AtomicU64::new(now_ms()));
        let inner_handler = ipc_handler::LoomdIpcHandler::new(self);
        let ipc_handler = ActivityHandler::new(inner_handler, last_activity.clone());

        let shutdown = self.shutdown_tx.subscribe();
        let idle_shutdown = self.shutdown_tx.clone();
        let idle_activity = last_activity.clone();

        tokio::spawn(async move {
            let interval = Duration::from_secs(30);
            loop {
                tokio::time::sleep(interval).await;
                let idle_ms = now_ms().saturating_sub(idle_activity.load(Ordering::Relaxed));
                if idle_ms >= DAEMON_DEFAULT_IDLE_TIMEOUT_MS {
                    tracing::info!(
                        idle_secs = idle_ms / 1000,
                        "Idle timeout reached; shutting down loomd"
                    );
                    let _ = idle_shutdown.send(());
                    break;
                }
            }
        }); // end idle shutdown spawn

        // ── Auto-sync background task ───────────────────────────────────
        // Subscribe to SkillsAdded / McpServerAdded events and push
        // Loom-managed resources into each tool's native filesystem.
        let mut event_rx = self.event_bus.subscribe();
        let sync_loom_dir = self.loom_dir.clone();
        let sync_home = std::env::var("HOME").unwrap_or_else(|_| "~".to_string());
        let sync_shutdown = self.shutdown_tx.subscribe();
        tokio::spawn(async move {
            use loom_adapters::{adapter_for, supported_tools};
            let mut sync_shutdown = sync_shutdown;
            loop {
                tokio::select! {
                    biased;
                    _ = sync_shutdown.recv() => {
                        tracing::debug!("Auto-sync task shutting down");
                        break;
                    }
                    event = event_rx.recv() => {
                        match event {
                            Ok(LoomEvent::SkillsAdded { .. } | LoomEvent::McpServerAdded { .. }) => {
                                tracing::info!("Auto-sync triggered by event");
                                for tool_name in supported_tools() {
                                    if let Some(adapter) = adapter_for(tool_name) {
                                        if let Err(e) = adapter.sync_to_native(&sync_loom_dir, &sync_home) {
                                            tracing::warn!("Auto-sync failed for {tool_name}: {e}");
                                        }
                                    }
                                }
                            }
                            Ok(_) => {} // ignore other events
                            Err(broadcast::error::RecvError::Closed) => {
                                tracing::debug!("Event bus closed, stopping auto-sync");
                                break;
                            }
                            Err(broadcast::error::RecvError::Lagged(n)) => {
                                tracing::warn!("Auto-sync lagged behind by {n} events");
                            }
                        }
                    }
                }
            }
        });

        tracing::info!(
            "Starting IPC server on {} (idle timeout {}s)",
            socket_path.display(),
            DAEMON_DEFAULT_IDLE_TIMEOUT_MS / 1000
        );
        let s = socket_path_str.clone();
        let serve_fut = async move { serve(&s, ipc_handler, shutdown).await.unwrap() };
        let serve_handle = tokio::spawn(serve_fut);

        // ── gRPC server ──────────────────────────────────────────────
        let grpc_config = grpc_server::GrpcConfig {
            listen_addr: std::env::var("LOOM_GRPC_ADDR").unwrap_or_else(|_| "127.0.0.1:50051".to_string()),
            enable_tls: std::env::var("LOOM_GRPC_TLS").is_ok(),
            cert_path: std::env::var("LOOM_GRPC_CERT").ok(),
            key_path: std::env::var("LOOM_GRPC_KEY").ok(),
        };
        let grpc_service = grpc_server::LoomGrpcService::new(
            &self.loom_dir,
            self.event_bus.clone(),
            self.shutdown_tx.clone(),
        );
        let grpc_handle = tokio::spawn(async move {
            if let Err(e) = grpc_server::start_grpc_server(grpc_config, grpc_service).await {
                tracing::error!("gRPC server error: {e}");
            }
        });

        tracing::info!("Loom daemon running on {}", self.loom_dir);

        let mut idle_shutdown_rx = self.shutdown_tx.subscribe();
        tokio::select! {
            _ = tokio::signal::ctrl_c() => {
                tracing::info!("Ctrl+C received");
            }
            _ = async move {
                let _ = idle_shutdown_rx.recv().await;
            } => {
                tracing::info!("Shutdown signal received");
            }
        }

        tracing::info!("Loom daemon shutting down.");
        self.shutdown_tx.send(()).ok();
        serve_handle.abort();
        grpc_handle.abort();
        let _ = std::fs::remove_file(&socket_path);
        Ok(())
    }
}

impl HealthCheck for Loomd {
    fn health(&self) -> anyhow::Result<loom_core::HealthStatus> {
        Ok(loom_core::HealthStatus {
            crate_name: "loomd",
            version: env!("CARGO_PKG_VERSION"),
            uptime_seconds: 0,
            last_error: None,
            performance_counters: std::collections::HashMap::new(),
        })
    }
}
