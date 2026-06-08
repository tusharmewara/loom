//! LSP transport — JSON-RPC over stdio to a language server subprocess.

use anyhow::{Context, Result};
use std::io::{BufRead, Read, Write};
use std::process::{Child, Command as StdCommand, Stdio};

/// Wraps a language server subprocess with JSON-RPC message framing.
pub struct LspTransport {
    child: Child,
    request_id: u64,
}

impl LspTransport {
    pub fn spawn(command: &str, args: &[String]) -> Result<Self> {
        let child = StdCommand::new(command)
            .args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .with_context(|| format!("Failed to spawn language server: {command}"))?;
        Ok(Self { child, request_id: 0 })
    }

    pub async fn notify(&mut self, method: &str, params: serde_json::Value) -> Result<()> {
        let msg = serde_json::json!({
            "jsonrpc": "2.0",
            "method": method,
            "params": params,
        });
        self.write_message(&msg)
    }

    pub async fn request<T: serde::de::DeserializeOwned>(
        &mut self,
        method: &str,
        params: serde_json::Value,
    ) -> Result<T> {
        let id = self.request_id;
        self.request_id += 1;

        let msg = serde_json::json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": method,
            "params": params,
        });
        self.write_message(&msg)?;

        loop {
            let response = self.read_message()?;
            if response["id"].as_u64() == Some(id) {
                if let Some(error) = response.get("error") {
                    anyhow::bail!("LSP error: {:?}", error);
                }
                return Ok(serde_json::from_value(response["result"].clone())?);
            }
        }
    }

    fn write_message(&mut self, msg: &serde_json::Value) -> Result<()> {
        let body = serde_json::to_string(msg)?;
        let header = format!("Content-Length: {}\r\n\r\n", body.len());
        if let Some(ref mut stdin) = self.child.stdin {
            let mut writer = std::io::BufWriter::new(stdin);
            writer.write_all(header.as_bytes())?;
            writer.write_all(body.as_bytes())?;
            writer.flush()?;
        }
        Ok(())
    }

    fn read_message(&mut self) -> Result<serde_json::Value> {
        let stdout = self.child.stdout.as_mut()
            .context("stdout not captured")?;
        let mut reader = std::io::BufReader::new(stdout);

        let mut content_length: usize = 0;
        loop {
            let mut line = String::new();
            reader.read_line(&mut line)?;
            let trimmed = line.trim();
            if trimmed.is_empty() {
                break;
            }
            if let Some(len_str) = trimmed.strip_prefix("Content-Length: ") {
                content_length = len_str.parse().unwrap_or(0);
            }
        }

        let mut body = vec![0u8; content_length];
        reader.read_exact(&mut body)?;
        Ok(serde_json::from_slice(&body)
            .context("Failed to parse LSP response JSON")?)
    }
}

impl Drop for LspTransport {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
