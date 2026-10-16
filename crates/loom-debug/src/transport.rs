//! DAP transport — JSON-RPC over stdio to a debug adapter subprocess.
//!
//! Uses the same Content-Length framing as LSP (both are Microsoft
//! language server protocol family standards).

use anyhow::{Context, Result};
use std::io::{BufRead, Read, Write};
use std::process::{Child, Command as StdCommand, Stdio};

/// Wraps a debug adapter subprocess with JSON-RPC message framing.
pub struct DapTransport {
    child: Child,
    request_id: u64,
}

impl DapTransport {
    /// Spawn a debug adapter process.
    pub fn spawn(command: &str, args: &[String]) -> Result<Self> {
        let child = StdCommand::new(command)
            .args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .with_context(|| format!("Failed to spawn debug adapter: {command}"))?;
        Ok(Self {
            child,
            request_id: 0,
        })
    }

    /// Send a JSON-RPC notification (no response expected).
    pub fn notify(&mut self, method: &str, params: serde_json::Value) -> Result<()> {
        let msg = serde_json::json!({
            "type": "request",
            "seq": self.request_id,
            "command": method,
            "arguments": params,
        });
        self.request_id += 1;
        self.write_message(&msg)
    }

    /// Send a JSON-RPC request and wait for the matching response.
    pub fn request<T: serde::de::DeserializeOwned>(
        &mut self,
        command: &str,
        params: serde_json::Value,
    ) -> Result<T> {
        let seq = self.request_id;
        self.request_id += 1;

        let msg = serde_json::json!({
            "type": "request",
            "seq": seq,
            "command": command,
            "arguments": params,
        });
        self.write_message(&msg)?;

        loop {
            let response = self.read_message()?;
            if response["type"] == "event" {
                // Events don't have seq matching — skip them
                continue;
            }
            if response.get("request_seq").and_then(|v| v.as_u64()) == Some(seq)
                || response.get("seq").and_then(|v| v.as_u64()) == Some(seq)
            {
                if let Some(err) = response.get("success") {
                    if !err.as_bool().unwrap_or(true) {
                        let msg = response["message"].as_str().unwrap_or("unknown error");
                        anyhow::bail!("DAP error: {msg}");
                    }
                }
                let body = response
                    .get("body")
                    .cloned()
                    .unwrap_or(serde_json::Value::Null);
                return Ok(serde_json::from_value(body)?);
            }
        }
    }

    /// Read raw events without a matching request.
    pub fn read_event(&mut self) -> Result<serde_json::Value> {
        loop {
            let msg = self.read_message()?;
            if msg["type"] == "event" {
                return Ok(msg);
            }
            // Ignore responses waiting for a request
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
        let stdout = self.child.stdout.as_mut().context("stdout not captured")?;
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
        serde_json::from_slice(&body).context("Failed to parse DAP response JSON")
    }
}

impl Drop for DapTransport {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
