use crate::protocol::{IpcRequest, IpcResponse};
use anyhow::Result;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixStream;

/// Connect to the loomd IPC socket and send a command, returning the response.
pub async fn send_command(socket_path: &str, req: IpcRequest) -> Result<IpcResponse> {
    let stream = UnixStream::connect(socket_path).await?;
    let (reader, mut writer) = stream.into_split();

    // Send request
    let req_line = req.to_json_line();
    writer.write_all(req_line.as_bytes()).await?;

    // Read response
    let mut buf_reader = BufReader::new(reader);
    let mut line = String::new();
    buf_reader.read_line(&mut line).await?;

    let resp: IpcResponse = serde_json::from_str(line.trim())?;
    Ok(resp)
}
