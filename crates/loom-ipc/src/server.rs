use crate::protocol::{IpcRequest, IpcResponse};
use anyhow::Result;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixListener;
use tokio::sync::broadcast;
use tracing::{debug, info, warn};

/// IPC command handler — implement this to handle requests
pub trait IpcHandler: Send + Sync + 'static {
    fn handle(&self, req: IpcRequest) -> IpcResponse;
}

/// Runs a Unix domain socket server, dispatching requests to the handler.
pub async fn serve(
    socket_path: &str,
    handler: impl IpcHandler,
    shutdown: broadcast::Receiver<()>,
) -> Result<()> {
    // Remove stale socket
    let _ = std::fs::remove_file(socket_path);

    let listener = UnixListener::bind(socket_path)?;
    info!("IPC server listening on {}", socket_path);

    let handler = std::sync::Arc::new(handler);
    let mut shutdown = shutdown;

    loop {
        tokio::select! {
            _ = shutdown.recv() => {
                info!("IPC server shutting down");
                let _ = std::fs::remove_file(socket_path);
                break;
            }
            result = listener.accept() => {
                let (stream, _addr) = result?;
                let handler = handler.clone();
                tokio::spawn(async move {
                    let (reader, mut writer) = stream.into_split();
                    let mut buf_reader = BufReader::new(reader);
                    let mut line = String::new();

                    loop {
                        line.clear();
                        match buf_reader.read_line(&mut line).await {
                            Ok(0) => break, // EOF
                            Ok(_) => {
                                debug!("IPC received request: {}", line.trim());
                                if let Ok(req) = serde_json::from_str::<IpcRequest>(line.trim()) {
                                    // Handlers are sync and drive their own tokio runtimes
                                    // (Handle::block_on / Runtime::block_on), which panics
                                    // if called with an ambient runtime context entered.
                                    // Run on a bare thread so that context is absent.
                                    // ponytail: one thread per request; IPC is a low-volume
                                    // control channel — switch to spawn_blocking + channel
                                    // if request volume ever matters.
                                    let req_id = req.request_id.clone();
                                    let handler = handler.clone();
                                    let resp = std::thread::scope(|s| {
                                        s.spawn(move || handler.handle(req)).join()
                                            .unwrap_or_else(|_| IpcResponse::error(req_id, "Internal server error: handler panicked"))
                                    });
                                    let resp_line = resp.to_json_line();
                                    debug!("IPC sending response: {}", resp_line.trim());
                                    let _ = writer.write_all(resp_line.as_bytes()).await;
                                }
                            }
                            Err(e) => {
                                warn!("IPC read error: {}", e);
                                break;
                            }
                        }
                    }
                });
            }
        }
    }

    Ok(())
}
