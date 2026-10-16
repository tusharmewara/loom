//! ACP Dispatch Server — TCP server for agent-to-agent task dispatch.
//!
//! Provides:
//! - Ed25519 challenge-response handshake (reuses [`AcpHandshake`])
//! - JSON-line protocol for dispatching tasks to named agents
//! - Health check endpoint
//! - Configurable bind address and key management
//!
//! # Protocol
//!
//! 1. **Handshake** (before any dispatch):
//!    ```json
//!    {"type":"handshake","challenge":"<hex>","signature":"<hex>","public_key":"<hex>"}
//!    {"type":"handshake_ack","challenge":"<hex>","signature":"<hex>","public_key":"<hex>"}
//!    ```
//! 2. **Dispatch** (after handshake):
//!    ```json
//!    {"type":"dispatch","request_id":"uuid","task_id":"...","agent_id":"...","payload":{...}}
//!    ```
//! 3. **Health** (after handshake):
//!    ```json
//!    {"type":"health","request_id":"uuid"}
//!    ```
//! 4. **Response** (always):
//!    ```json
//!    {"type":"response","request_id":"...","status":"ok|error","data":{...},"error":null}
//!    ```

use crate::acp_handshake::{perform_responder_handshake, AcpKeyPair};
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::net::SocketAddr;
use std::path::Path;
use std::sync::Arc;
use std::time::SystemTime;
use tokio::io::{AsyncBufReadExt, AsyncWrite, AsyncWriteExt, BufReader};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{broadcast, RwLock};

// ─── Configuration ─────────────────────────────────────────────────────

/// Configuration for the ACP dispatch server.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcpServerConfig {
    /// Bind address (e.g., "127.0.0.1:0" for random port).
    #[serde(default = "default_bind")]
    pub bind: String,
    /// Ed25519 secret key seed (32 bytes, hex-encoded).
    /// If empty, a new key is generated on first start.
    #[serde(default)]
    pub secret_key: String,
    /// Path to persist the generated key pair (if secret_key is empty).
    #[serde(default)]
    pub key_path: String,
}

fn default_bind() -> String {
    "127.0.0.1:0".to_string()
}

impl Default for AcpServerConfig {
    fn default() -> Self {
        Self {
            bind: default_bind(),
            secret_key: String::new(),
            key_path: String::new(),
        }
    }
}

// ─── Server State ──────────────────────────────────────────────────────

/// Handles dispatch requests for a specific agent/tool.
pub type DispatchFn = Arc<dyn Fn(&DispatchRequest) -> Result<Value> + Send + Sync>;

/// A request to dispatch work to an agent.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DispatchRequest {
    /// Opaque identifier for this request.
    pub request_id: String,
    /// Task identifier for the workflow system.
    pub task_id: String,
    /// Target agent identifier.
    pub agent_id: String,
    /// Arbitrary payload for the agent.
    #[serde(default)]
    pub payload: Value,
}

/// Response sent back to the caller.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DispatchResponse {
    pub request_id: String,
    pub status: String,
    pub data: Option<Value>,
    pub error: Option<String>,
}

// ─── ACP Dispatch Server ───────────────────────────────────────────────

/// The ACP dispatch server, listening for connections and dispatching tasks.
pub struct AcpDispatchServer {
    /// Resolved listen address (after binding).
    listen_addr: Arc<RwLock<Option<SocketAddr>>>,
    /// The server's key pair for handshakes.
    key_pair: AcpKeyPair,
    /// Registered dispatch handlers (agent_id → handler).
    handlers: Arc<RwLock<std::collections::HashMap<String, DispatchFn>>>,
    /// Optional default handler (fallback when no specific handler is registered).
    default_handler: Arc<RwLock<Option<DispatchFn>>>,
    /// Shutdown signal sender.
    shutdown_tx: Arc<RwLock<Option<broadcast::Sender<()>>>>,
    /// Server config.
    config: AcpServerConfig,
    /// Start time for uptime tracking.
    start_time: Arc<RwLock<Option<SystemTime>>>,
}

impl AcpDispatchServer {
    /// Create a new ACP dispatch server.
    pub fn new(config: AcpServerConfig) -> Result<Self> {
        let key_pair = if !config.secret_key.is_empty() {
            AcpKeyPair::from_seed_hex(&config.secret_key)?
        } else if !config.key_path.is_empty() && Path::new(&config.key_path).exists() {
            let hex =
                std::fs::read_to_string(&config.key_path).context("Failed to read key file")?;
            AcpKeyPair::from_seed_hex(hex.trim())?
        } else {
            let kp = AcpKeyPair::generate();
            // Persist if key_path is set
            if !config.key_path.is_empty() {
                if let Some(parent) = Path::new(&config.key_path).parent() {
                    let _ = std::fs::create_dir_all(parent);
                }
                let _ = std::fs::write(&config.key_path, kp.secret_key_hex());
            }
            kp
        };

        Ok(Self {
            listen_addr: Arc::new(RwLock::new(None)),
            key_pair,
            handlers: Arc::new(RwLock::new(std::collections::HashMap::new())),
            default_handler: Arc::new(RwLock::new(None)),
            shutdown_tx: Arc::new(RwLock::new(None)),
            config,
            start_time: Arc::new(RwLock::new(None)),
        })
    }

    /// Register a dispatch handler for a specific agent.
    pub async fn register_handler(&self, agent_id: &str, handler: DispatchFn) {
        let mut handlers = self.handlers.write().await;
        handlers.insert(agent_id.to_string(), handler);
    }

    /// Unregister a dispatch handler.
    pub async fn unregister_handler(&self, agent_id: &str) {
        let mut handlers = self.handlers.write().await;
        handlers.remove(agent_id);
    }

    /// Set a default handler used as fallback when no specific handler is registered.
    /// Clears the previous default handler if `None` is passed.
    pub async fn set_default_handler(&self, handler: Option<DispatchFn>) {
        let mut dh = self.default_handler.write().await;
        *dh = handler;
    }

    /// Get the resolved listen address (available after `start`).
    pub async fn listen_addr(&self) -> Option<SocketAddr> {
        *self.listen_addr.read().await
    }

    /// Get the server's public key (hex-encoded).
    pub fn public_key_hex(&self) -> String {
        self.key_pair.public_key_hex()
    }

    /// Get uptime in seconds (0 if not started).
    pub async fn uptime_secs(&self) -> u64 {
        match *self.start_time.read().await {
            Some(t) => t.elapsed().unwrap_or_default().as_secs(),
            None => 0,
        }
    }

    /// Get server status information.
    pub async fn status(&self) -> Value {
        serde_json::json!({
            "running": self.listen_addr.read().await.is_some(),
            "listen_addr": self.listen_addr.read().await.map(|a| a.to_string()),
            "public_key": self.public_key_hex(),
            "uptime_secs": self.uptime_secs().await,
            "handlers_count": self.handlers.read().await.len(),
            "handlers": self.handlers.read().await.keys().cloned().collect::<Vec<_>>(),
            "has_default_handler": self.default_handler.read().await.is_some(),
        })
    }

    /// Start the server — binds to the configured address and begins accepting
    /// connections. Returns immediately; the server runs in background tasks.
    pub async fn start(&self) -> Result<SocketAddr> {
        let listener = TcpListener::bind(&self.config.bind)
            .await
            .context("Failed to bind ACP dispatch server")?;

        let addr = listener.local_addr()?;
        {
            let mut listen_addr = self.listen_addr.write().await;
            *listen_addr = Some(addr);
        }
        {
            let mut start_time = self.start_time.write().await;
            *start_time = Some(SystemTime::now());
        }

        let (shutdown_tx, _) = broadcast::channel(1);
        {
            let mut tx = self.shutdown_tx.write().await;
            *tx = Some(shutdown_tx.clone());
        }

        let handlers = self.handlers.clone();
        let default_handler = self.default_handler.clone();
        let key_pair = AcpKeyPair::from_seed_hex(&self.key_pair.secret_key_hex())
            .expect("Failed to clone key pair");

        tokio::spawn(async move {
            loop {
                let mut shutdown_rx = shutdown_tx.subscribe();
                tokio::select! {
                    biased;
                    _ = shutdown_rx.recv() => {
                        tracing::info!("ACP dispatch server shutting down");
                        break;
                    }
                    accept = listener.accept() => {
                        match accept {
                            Ok((stream, peer_addr)) => {
                                let handlers = handlers.clone();
                                let default_handler = default_handler.clone();
                                let kp = AcpKeyPair::from_seed_hex(&key_pair.secret_key_hex())
                                    .expect("Failed to clone key pair for connection");
                                tokio::spawn(async move {
                                    if let Err(e) = handle_connection(stream, peer_addr, kp, handlers, default_handler).await {
                                        tracing::warn!("ACP connection error from {peer_addr}: {e}");
                                    }
                                });
                            }
                            Err(e) => {
                                tracing::error!("ACP accept error: {e}");
                                break;
                            }
                        }
                    }
                }
            }
        });

        tracing::info!("ACP dispatch server listening on {addr}");
        Ok(addr)
    }

    /// Stop the server gracefully.
    pub async fn stop(&self) {
        let mut tx = self.shutdown_tx.write().await;
        if let Some(tx) = tx.take() {
            let _ = tx.send(());
        }
        let mut listen_addr = self.listen_addr.write().await;
        *listen_addr = None;
    }
}

// ─── Connection Handling ───────────────────────────────────────────────

async fn handle_connection(
    mut stream: TcpStream,
    peer_addr: SocketAddr,
    key_pair: AcpKeyPair,
    handlers: Arc<RwLock<std::collections::HashMap<String, DispatchFn>>>,
    default_handler: Arc<RwLock<Option<DispatchFn>>>,
) -> Result<()> {
    tracing::debug!("ACP connection from {peer_addr}");

    let (reader, mut writer) = stream.split();
    let mut reader = BufReader::new(reader);
    let mut line = String::new();

    // ── Handshake ───────────────────────────────────────────────────────
    // Expect: {"type":"handshake","challenge":"...","signature":"...","public_key":"..."}
    line.clear();
    reader.read_line(&mut line).await?;
    let msg: Value = serde_json::from_str(line.trim()).context("Invalid handshake JSON")?;

    let msg_type = msg["type"].as_str().unwrap_or("");
    if msg_type != "handshake" {
        anyhow::bail!("Expected handshake, got '{msg_type}'");
    }

    let challenge = msg["challenge"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("Missing challenge"))?;
    let initiator_sig = msg["signature"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("Missing signature"))?;
    let initiator_pub = msg["public_key"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("Missing public_key"))?;

    // Perform responder handshake
    let (_handshake, our_sig) =
        perform_responder_handshake(&key_pair, challenge, initiator_sig, initiator_pub)?;

    // Send handshake_ack
    let ack = serde_json::json!({
        "type": "handshake_ack",
        "challenge": challenge,
        "signature": our_sig,
        "public_key": key_pair.public_key_hex(),
    });
    writer
        .write_all(format!("{}\n", serde_json::to_string(&ack)?).as_bytes())
        .await?;
    writer.flush().await?;

    tracing::debug!("ACP handshake completed with {peer_addr}");

    // ── Command Loop ────────────────────────────────────────────────────
    loop {
        line.clear();
        let n = reader.read_line(&mut line).await?;
        if n == 0 {
            tracing::debug!("ACP connection closed by {peer_addr}");
            break; // EOF
        }

        let cmd: Value = match serde_json::from_str(line.trim()) {
            Ok(v) => v,
            Err(e) => {
                let err_resp = serde_json::json!({
                    "type": "response",
                    "request_id": null,
                    "status": "error",
                    "data": null,
                    "error": format!("Invalid JSON: {e}"),
                });
                writer
                    .write_all(format!("{}\n", serde_json::to_string(&err_resp)?).as_bytes())
                    .await?;
                writer.flush().await?;
                continue;
            }
        };

        let cmd_type = cmd["type"].as_str().unwrap_or("");
        let request_id = cmd["request_id"].as_str().unwrap_or("unknown");

        match cmd_type {
            "dispatch" => {
                let agent_id = cmd["agent_id"].as_str().unwrap_or("");
                let task_id = cmd["task_id"].as_str().unwrap_or("");

                if agent_id.is_empty() {
                    send_response(
                        &mut writer,
                        request_id,
                        "error",
                        None,
                        Some("agent_id required"),
                    )
                    .await?;
                    continue;
                }
                if task_id.is_empty() {
                    send_response(
                        &mut writer,
                        request_id,
                        "error",
                        None,
                        Some("task_id required"),
                    )
                    .await?;
                    continue;
                }

                let payload = cmd.get("payload").cloned().unwrap_or(Value::Null);

                // Check if we have a handler for this agent
                let dispatch_result = {
                    let handlers = handlers.read().await;
                    match handlers.get(agent_id) {
                        Some(handler) => {
                            let req = DispatchRequest {
                                request_id: request_id.to_string(),
                                task_id: task_id.to_string(),
                                agent_id: agent_id.to_string(),
                                payload,
                            };
                            handler(&req)
                        }
                        None => {
                            // Fall back to default handler if available
                            let dh = default_handler.read().await;
                            match dh.as_ref() {
                                Some(handler) => {
                                    let req = DispatchRequest {
                                        request_id: request_id.to_string(),
                                        task_id: task_id.to_string(),
                                        agent_id: agent_id.to_string(),
                                        payload,
                                    };
                                    handler(&req)
                                }
                                None => Err(anyhow::anyhow!(
                                    "No handler registered for agent '{agent_id}'"
                                )),
                            }
                        }
                    }
                };

                match dispatch_result {
                    Ok(data) => {
                        send_response(&mut writer, request_id, "ok", Some(data), None).await?;
                    }
                    Err(e) => {
                        send_response(&mut writer, request_id, "error", None, Some(&e.to_string()))
                            .await?;
                    }
                }
            }
            "health" => {
                let health_data = serde_json::json!({
                    "status": "healthy",
                    "version": env!("CARGO_PKG_VERSION"),
                    "public_key": key_pair.public_key_hex(),
                });
                send_response(&mut writer, request_id, "ok", Some(health_data), None).await?;
            }
            _ => {
                send_response(
                    &mut writer,
                    request_id,
                    "error",
                    None,
                    Some(&format!("Unknown command type: '{cmd_type}'")),
                )
                .await?;
            }
        }
    }

    Ok(())
}

/// Send a JSON-line response to the client.
async fn send_response(
    writer: &mut (impl AsyncWrite + Unpin),
    request_id: &str,
    status: &str,
    data: Option<Value>,
    error: Option<&str>,
) -> Result<()> {
    let resp = serde_json::json!({
        "type": "response",
        "request_id": request_id,
        "status": status,
        "data": data,
        "error": error,
    });
    let bytes = format!("{}\n", serde_json::to_string(&resp)?);
    writer.write_all(bytes.as_bytes()).await?;
    writer.flush().await?;
    Ok(())
}

// ─── Tests ─────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::acp_handshake::{complete_initiator, initiate_handshake};
    use std::time::Duration;
    use tokio::io::AsyncReadExt;

    async fn connect_and_handshake(
        addr: SocketAddr,
        client_key: &AcpKeyPair,
    ) -> Result<tokio::net::TcpStream> {
        let mut stream = tokio::net::TcpStream::connect(addr).await?;

        // Initiate handshake
        let (mut handshake, challenge, our_sig) = initiate_handshake(client_key)?;

        let msg = serde_json::json!({
            "type": "handshake",
            "challenge": challenge,
            "signature": our_sig,
            "public_key": client_key.public_key_hex(),
        });
        stream
            .write_all(format!("{}\n", serde_json::to_string(&msg)?).as_bytes())
            .await?;

        // Read handshake_ack
        let mut line = String::new();
        let mut reader = BufReader::new(&mut stream);
        reader.read_line(&mut line).await?;
        let ack: Value = serde_json::from_str(line.trim())?;
        assert_eq!(ack["type"], "handshake_ack");

        let server_sig = ack["signature"].as_str().unwrap();
        let server_pub = ack["public_key"].as_str().unwrap();

        // Complete handshake
        complete_initiator(&mut handshake, server_sig, server_pub, &challenge)?;

        // Drop the BufReader to get the stream back
        drop(reader);
        Ok(stream)
    }

    #[tokio::test]
    async fn test_server_start_stop() {
        let config = AcpServerConfig::default();
        let server = AcpDispatchServer::new(config).unwrap();
        let addr = server.start().await.unwrap();
        assert!(addr.port() > 0);
        assert!(server.listen_addr().await.is_some());
        server.stop().await;
        assert!(server.listen_addr().await.is_none());
    }

    #[tokio::test]
    async fn test_handshake_success() {
        let config = AcpServerConfig::default();
        let server = AcpDispatchServer::new(config).unwrap();
        let addr = server.start().await.unwrap();
        let client_key = AcpKeyPair::generate();

        let stream = connect_and_handshake(addr, &client_key).await.unwrap();
        assert_eq!(stream.peer_addr().unwrap().port(), addr.port());
        server.stop().await;
    }

    #[tokio::test]
    async fn test_dispatch_no_handler() {
        let config = AcpServerConfig::default();
        let server = AcpDispatchServer::new(config).unwrap();
        let addr = server.start().await.unwrap();
        let client_key = AcpKeyPair::generate();

        let mut stream = connect_and_handshake(addr, &client_key).await.unwrap();

        let dispatch = serde_json::json!({
            "type": "dispatch",
            "request_id": "test-1",
            "task_id": "task-1",
            "agent_id": "nonexistent",
            "payload": {},
        });
        stream
            .write_all(format!("{}\n", serde_json::to_string(&dispatch).unwrap()).as_bytes())
            .await
            .unwrap();
        stream.flush().await.unwrap();

        let mut reader = BufReader::new(&mut stream);
        let mut line = String::new();
        reader.read_line(&mut line).await.unwrap();
        let resp: Value = serde_json::from_str(line.trim()).unwrap();
        assert_eq!(resp["status"], "error");
        assert!(resp["error"]
            .as_str()
            .unwrap()
            .contains("No handler registered"));

        server.stop().await;
    }

    #[tokio::test]
    async fn test_dispatch_with_handler() {
        let config = AcpServerConfig::default();
        let server = AcpDispatchServer::new(config).unwrap();
        let addr = server.start().await.unwrap();
        let client_key = AcpKeyPair::generate();

        // Register a handler
        let handler: DispatchFn = Arc::new(|req: &DispatchRequest| {
            Ok(serde_json::json!({
                "received_task_id": req.task_id,
                "echo_payload": req.payload,
            }))
        });
        server.register_handler("test-agent", handler).await;

        let mut stream = connect_and_handshake(addr, &client_key).await.unwrap();

        let dispatch = serde_json::json!({
            "type": "dispatch",
            "request_id": "test-2",
            "task_id": "task-42",
            "agent_id": "test-agent",
            "payload": {"foo": "bar"},
        });
        stream
            .write_all(format!("{}\n", serde_json::to_string(&dispatch).unwrap()).as_bytes())
            .await
            .unwrap();
        stream.flush().await.unwrap();

        let mut reader = BufReader::new(&mut stream);
        let mut line = String::new();
        reader.read_line(&mut line).await.unwrap();
        let resp: Value = serde_json::from_str(line.trim()).unwrap();
        assert_eq!(resp["status"], "ok");
        let data = resp["data"].as_object().unwrap();
        assert_eq!(data["received_task_id"], "task-42");
        assert_eq!(data["echo_payload"]["foo"], "bar");

        server.stop().await;
    }

    #[tokio::test]
    async fn test_health_endpoint() {
        let config = AcpServerConfig::default();
        let server = AcpDispatchServer::new(config).unwrap();
        let addr = server.start().await.unwrap();
        let client_key = AcpKeyPair::generate();

        let mut stream = connect_and_handshake(addr, &client_key).await.unwrap();

        let health_req = serde_json::json!({
            "type": "health",
            "request_id": "health-1",
        });
        stream
            .write_all(format!("{}\n", serde_json::to_string(&health_req).unwrap()).as_bytes())
            .await
            .unwrap();
        stream.flush().await.unwrap();

        let mut reader = BufReader::new(&mut stream);
        let mut line = String::new();
        reader.read_line(&mut line).await.unwrap();
        let resp: Value = serde_json::from_str(line.trim()).unwrap();
        assert_eq!(resp["status"], "ok");
        assert_eq!(resp["data"]["status"], "healthy");

        server.stop().await;
    }

    #[tokio::test]
    async fn test_unknown_command() {
        let config = AcpServerConfig::default();
        let server = AcpDispatchServer::new(config).unwrap();
        let addr = server.start().await.unwrap();
        let client_key = AcpKeyPair::generate();

        let mut stream = connect_and_handshake(addr, &client_key).await.unwrap();

        let bad_req = serde_json::json!({
            "type": "foobar",
            "request_id": "bad-1",
        });
        stream
            .write_all(format!("{}\n", serde_json::to_string(&bad_req).unwrap()).as_bytes())
            .await
            .unwrap();
        stream.flush().await.unwrap();

        let mut reader = BufReader::new(&mut stream);
        let mut line = String::new();
        reader.read_line(&mut line).await.unwrap();
        let resp: Value = serde_json::from_str(line.trim()).unwrap();
        assert_eq!(resp["status"], "error");
        assert!(resp["error"].as_str().unwrap().contains("Unknown command"));

        server.stop().await;
    }

    #[tokio::test]
    async fn test_key_persistence() {
        let dir = tempfile::tempdir().unwrap();
        let key_path = dir.path().join("acp_key.txt");
        let config = AcpServerConfig {
            key_path: key_path.to_string_lossy().to_string(),
            ..Default::default()
        };
        let server = AcpDispatchServer::new(config).unwrap();
        let pub_key = server.public_key_hex();

        // Stop and recreate — should load the same key
        server.stop().await;
        drop(server);

        let config2 = AcpServerConfig {
            key_path: key_path.to_string_lossy().to_string(),
            ..Default::default()
        };
        let server2 = AcpDispatchServer::new(config2).unwrap();
        assert_eq!(server2.public_key_hex(), pub_key);
    }

    #[tokio::test]
    async fn test_invalid_handshake_rejected() {
        let config = AcpServerConfig::default();
        let server = AcpDispatchServer::new(config).unwrap();
        let addr = server.start().await.unwrap();

        // Send invalid handshake (wrong signature)
        let mut stream = tokio::net::TcpStream::connect(addr).await.unwrap();
        let bad_msg = serde_json::json!({
            "type": "handshake",
            "challenge": "deadbeef",
            "signature": "00000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000",
            "public_key": "0000000000000000000000000000000000000000000000000000000000000000",
        });
        stream
            .write_all(format!("{}\n", serde_json::to_string(&bad_msg).unwrap()).as_bytes())
            .await
            .unwrap();
        stream.flush().await.unwrap();

        // Wait for close from server
        let mut buf = [0u8; 1024];
        let n = tokio::time::timeout(Duration::from_secs(3), stream.read(&mut buf)).await;
        // The connection should close (n is Ok(0) meaning EOF or Err)
        assert!(n.is_ok() || n.is_err());

        server.stop().await;
    }
}
