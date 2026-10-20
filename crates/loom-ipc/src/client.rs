use crate::protocol::{IpcRequest, IpcResponse};
use anyhow::{Context, Result};
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixStream;
use tokio::time::timeout;

/// Default deadline for a whole IPC request/response round trip.
pub const DEFAULT_REQUEST_TIMEOUT: Duration = Duration::from_secs(30);

/// Connect to the loomd IPC socket and send a command, returning the response.
///
/// Uses [`DEFAULT_REQUEST_TIMEOUT`] as the overall deadline; a daemon that accepts
/// the connection but never replies can no longer hang the caller forever.
pub async fn send_command(socket_path: &str, req: IpcRequest) -> Result<IpcResponse> {
    send_command_with_timeout(socket_path, req, DEFAULT_REQUEST_TIMEOUT).await
}

/// Same as [`send_command`] but with an explicit overall deadline.
pub async fn send_command_with_timeout(
    socket_path: &str,
    req: IpcRequest,
    request_timeout: Duration,
) -> Result<IpcResponse> {
    timeout(request_timeout, send_command_inner(socket_path, req))
        .await
        .map_err(|_| {
            anyhow::anyhow!(
                "IPC request timed out after {:?}: daemon did not respond on socket {}",
                request_timeout,
                socket_path
            )
        })?
}

async fn send_command_inner(socket_path: &str, req: IpcRequest) -> Result<IpcResponse> {
    let stream = UnixStream::connect(socket_path)
        .await
        .with_context(|| format!("failed to connect to IPC socket {}", socket_path))?;
    let (reader, mut writer) = stream.into_split();

    // Send request
    let req_line = req.to_json_line();
    writer
        .write_all(req_line.as_bytes())
        .await
        .with_context(|| format!("failed to write IPC request to {}", socket_path))?;

    // Read response
    let mut buf_reader = BufReader::new(reader);
    let mut line = String::new();
    let n = buf_reader
        .read_line(&mut line)
        .await
        .with_context(|| format!("failed to read IPC response from {}", socket_path))?;
    if n == 0 {
        anyhow::bail!("IPC daemon closed the connection on {} without responding", socket_path);
    }

    let resp: IpcResponse = serde_json::from_str(line.trim())
        .with_context(|| format!("invalid IPC response from {}", socket_path))?;
    Ok(resp)
}
