use crate::protocol::{IpcRequest, IpcResponse};
use anyhow::Result;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixListener;
use tokio::sync::broadcast;

/// IPC command handler — implement this to handle requests
pub trait IpcHandler: Send + Sync + 'static {
    fn handle(&self, req: IpcRequest) -> IpcResponse;
}

/// Runs a Unix domain socket server, dispatching requests to the handler.
pub async fn serve(socket_path: &str, handler: impl IpcHandler, shutdown: broadcast::Receiver<()>) -> Result<()> {
    // Remove stale socket
    let _ = std::fs::remove_file(socket_path);

    let listener = UnixListener::bind(socket_path)?;
    tracing::info!("IPC server listening on {}", socket_path);

    let handler = std::sync::Arc::new(handler);
    let mut shutdown = shutdown;

    loop {
        tokio::select! {
            _ = shutdown.recv() => {
                tracing::info!("IPC server shutting down");
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
                                if let Ok(req) = serde_json::from_str::<IpcRequest>(line.trim()) {
                                    let resp = handler.handle(req);
                                    let resp_line = resp.to_json_line();
                                    let _ = writer.write_all(resp_line.as_bytes()).await;
                                }
                            }
                            Err(e) => {
                                tracing::warn!("IPC read error: {}", e);
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
