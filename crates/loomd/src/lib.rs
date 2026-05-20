mod activity;
mod ipc_handler;
use activity::{now_ms, ActivityHandler};
use loom_core::{constants::DAEMON_DEFAULT_IDLE_TIMEOUT_MS, HealthCheck};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;
use tokio::sync::broadcast;

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
        });

        tracing::info!(
            "Starting IPC server on {} (idle timeout {}s)",
            socket_path.display(),
            DAEMON_DEFAULT_IDLE_TIMEOUT_MS / 1000
        );
        let s = socket_path_str.clone();
        let serve_fut = async move { serve(&s, ipc_handler, shutdown).await.unwrap() };
        let serve_handle = tokio::spawn(serve_fut);

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
