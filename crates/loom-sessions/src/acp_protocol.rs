//! Canonical Agent Client Protocol (v1) client.
//!
//! Speaks JSON-RPC 2.0 over the agent's stdio (newline-delimited
//! messages): `initialize` → `session/new` → `session/prompt`,
//! collecting `session/update` notifications until the prompt response
//! arrives with a stop reason. Permission requests are answered with
//! the first `allow*` option (documented headless policy).
//!
//! NOTE (TDD RED step): tests only — `AcpProtocolClient` and friends do
//! not exist yet.

use serde_json::Value;
use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::process::{Command, Stdio};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

/// ACP major protocol version this client speaks.
pub const ACP_PROTOCOL_VERSION: u64 = 1;

/// Configuration for one headless agent invocation.
#[derive(Debug, Clone)]
pub struct AcpClientConfig {
    /// Agent program (absolute path or PATH name).
    pub program: String,
    /// Arguments for the program.
    pub args: Vec<String>,
    /// Session working directory (absolute). Defaults to current dir.
    pub cwd: Option<String>,
    /// Per-request timeout in seconds.
    pub timeout_secs: u64,
}

/// Collected outcome of one prompt turn.
#[derive(Debug, Clone, Default)]
pub struct AcpTranscript {
    /// Agent-side session id from `session/new`.
    pub session_id: String,
    /// Stop reason from the `session/prompt` response (`end_turn`, …).
    pub stop_reason: String,
    /// Concatenated `agent_message_chunk` text.
    pub agent_text: String,
    /// Tool summaries as `title [status]`.
    pub tool_calls: Vec<String>,
}

/// One newline-delimited JSON-RPC message from the agent.
#[derive(Debug)]
pub enum AcpIncoming {
    Response {
        id: Value,
        result: Option<Value>,
        error: Option<Value>,
    },
    Notification {
        method: String,
        params: Value,
    },
    Request {
        id: Value,
        method: String,
        params: Value,
    },
    Invalid,
}

/// Classify one raw stdio line. Ids stay as raw JSON values so
/// agent-chosen (string) ids round-trip exactly.
pub fn parse_message(line: &str) -> AcpIncoming {
    let Ok(value) = serde_json::from_str::<Value>(line) else {
        return AcpIncoming::Invalid;
    };
    if !value.is_object() {
        return AcpIncoming::Invalid;
    }
    let id = value.get("id").cloned();
    let method = value
        .get("method")
        .and_then(Value::as_str)
        .map(str::to_string);
    let params = value.get("params").cloned().unwrap_or(Value::Null);
    let id_is_scalar = matches!(&id, Some(Value::Number(_)) | Some(Value::String(_)));
    match (id, method) {
        (Some(id), Some(method)) if id_is_scalar => AcpIncoming::Request { id, method, params },
        (Some(id), None) if id_is_scalar => AcpIncoming::Response {
            id,
            result: value.get("result").cloned(),
            error: value.get("error").cloned(),
        },
        (None, Some(method)) => AcpIncoming::Notification { method, params },
        _ => AcpIncoming::Invalid,
    }
}

/// Build one JSON-RPC request line (without trailing newline).
pub fn build_request(id: u64, method: &str, params: Value) -> String {
    serde_json::json!({
        "jsonrpc": "2.0",
        "id": id,
        "method": method,
        "params": params,
    })
    .to_string()
}

fn id_key(id: &Value) -> String {
    serde_json::to_string(id).unwrap_or_default()
}

#[derive(Default)]
struct TranscriptAcc {
    text: String,
    tools: Vec<(String, String, String)>,
}

struct Shared {
    responses: Mutex<HashMap<String, (Option<Value>, Option<Value>)>>,
    arrived: Condvar,
    transcript: Mutex<TranscriptAcc>,
}

fn accumulate(transcript: &mut TranscriptAcc, params: &Value) {
    let update = params.get("update").unwrap_or(&Value::Null);
    let kind = update
        .get("sessionUpdate")
        .and_then(Value::as_str)
        .unwrap_or("");
    match kind {
        "agent_message_chunk" => {
            if let Some(text) = update
                .get("content")
                .and_then(|c| c.get("text"))
                .and_then(Value::as_str)
            {
                transcript.text.push_str(text);
            }
        }
        "tool_call" => {
            let call_id = update
                .get("toolCallId")
                .and_then(Value::as_str)
                .unwrap_or("?")
                .to_string();
            let title = update
                .get("title")
                .and_then(Value::as_str)
                .unwrap_or("?")
                .to_string();
            let status = update
                .get("status")
                .and_then(Value::as_str)
                .unwrap_or("pending")
                .to_string();
            transcript.tools.push((call_id, title, status));
        }
        "tool_call_update" => {
            let call_id = update.get("toolCallId").and_then(Value::as_str);
            let status = update.get("status").and_then(Value::as_str);
            if let (Some(call_id), Some(status)) = (call_id, status) {
                if let Some(entry) = transcript
                    .tools
                    .iter_mut()
                    .find(|(id, _, _)| id == call_id)
                {
                    entry.2 = status.to_string();
                }
            }
        }
        _ => {}
    }
}

/// Headless policy for `session/request_permission`: pick the first
/// `allow*` option, else the first option, else report cancelled.
fn permission_outcome(params: &Value) -> Value {
    let options = params
        .get("options")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let allow = options.iter().find(|o| {
        o.get("kind")
            .and_then(Value::as_str)
            .map(|k| k.starts_with("allow"))
            .unwrap_or(false)
    });
    match allow.or_else(|| options.first()) {
        Some(option) => {
            let option_id = option
                .get("optionId")
                .and_then(Value::as_str)
                .unwrap_or("");
            serde_json::json!({
                "outcome": "selected",
                "optionId": option_id,
            })
        }
        None => serde_json::json!({ "outcome": "cancelled" }),
    }
}

/// Canonical ACP v1 client: one-shot headless prompt turns.
pub struct AcpProtocolClient;

impl AcpProtocolClient {
    /// Spawn the agent, run initialize → session/new → session/prompt,
    /// and return the collected transcript. The child is always reaped.
    pub fn run_prompt(config: &AcpClientConfig, prompt: &str) -> anyhow::Result<AcpTranscript> {
        let timeout = Duration::from_secs(config.timeout_secs.max(1));
        let mut child = Command::new(&config.program)
            .args(&config.args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| anyhow::anyhow!("failed to spawn ACP agent '{}': {e}", config.program))?;

        let result = Self::drive(config, prompt, timeout, &mut child);
        let _ = child.kill();
        let _ = child.wait();
        result
    }

    fn drive(
        config: &AcpClientConfig,
        prompt: &str,
        timeout: Duration,
        child: &mut std::process::Child,
    ) -> anyhow::Result<AcpTranscript> {
        let mut stdin = child
            .stdin
            .take()
            .ok_or_else(|| anyhow::anyhow!("ACP agent has no stdin"))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| anyhow::anyhow!("ACP agent has no stdout"))?;
        let stderr = child
            .stderr
            .take();
        if let Some(stderr) = stderr {
            // Spawn a thread to read stderr
            let _stderr_thread = std::thread::spawn(move || {
                let reader = BufReader::new(stderr);
                for line in reader.lines() {
                    match line {
                        Ok(line) => eprintln!("[STDERR] {}", line),
                        Err(e) => eprintln!("[STDERR ERROR] {}", e),
                    }
                }
            });
        }
        let stdin = Arc::new(Mutex::new(stdin));
        let shared = Arc::new(Shared {
            responses: Mutex::new(HashMap::new()),
            arrived: Condvar::new(),
            transcript: Mutex::new(TranscriptAcc::default()),
        });

        let reader_shared = shared.clone();
        let reader_stdin = stdin.clone();
        std::thread::spawn(move || {
            let reader = BufReader::new(stdout);
            let mut line_count = 0;
            for line in reader.lines() {
                match line {
                    Ok(line) => {
                        line_count += 1;
                        if line.trim().is_empty() {
                            continue;
                        }
                        match parse_message(&line) {
                            AcpIncoming::Response { id, result, error } => {
                                reader_shared
                                    .responses
                                    .lock()
                                    .unwrap()
                                    .insert(id_key(&id), (result, error));
                                reader_shared.arrived.notify_all();
                            }
                            AcpIncoming::Notification { method, params } => {
                                if method == "session/update" {
                                    if let Ok(mut acc) = reader_shared.transcript.lock() {
                                        accumulate(&mut acc, &params);
                                    }
                                }
                            }
                            AcpIncoming::Request { id, method, params } => {
                                if method == "session/request_permission" {
                                    let response = serde_json::json!({
                                        "jsonrpc": "2.0",
                                        "id": id,
                                        "result": { "outcome": permission_outcome(&params) },
                                    });
                                    if let Ok(mut stdin) = reader_stdin.lock() {
                                        let _ = writeln!(stdin, "{response}");
                                        let _ = stdin.flush();
                                    }
                                }
                            }
                            AcpIncoming::Invalid => {}
                        }
                    }
                    Err(e) => {
                        break;
                    }
                }
            }
        });

        let send = |id: u64, method: &str, params: Value| -> anyhow::Result<()> {
            let line = build_request(id, method, params);
            let mut stdin_guard = stdin
                .lock()
                .map_err(|_| anyhow::anyhow!("ACP stdin lock poisoned"))?;
            writeln!(stdin_guard, "{line}")
                .map_err(|e| anyhow::anyhow!("failed to write ACP request: {e}"))?;
            stdin_guard
                .flush()
                .map_err(|e| anyhow::anyhow!("failed to flush ACP request: {e}"))?;
            Ok(())
        };
        let wait_response =
            |id: u64, method: &str, timeout: Duration| -> anyhow::Result<Value> {
                let key = id.to_string();
                let deadline = Instant::now() + timeout;
                let mut guard = shared.responses.lock().unwrap();
                loop {
                    if let Some((result, error)) = guard.remove(&key) {
                        if let Some(error) = error {
                            let message = error
                                .get("message")
                                .and_then(Value::as_str)
                                .unwrap_or("unknown ACP error");
                            anyhow::bail!("ACP {method} failed: {message}");
                        }
                        return Ok(result.unwrap_or(Value::Null));
                    }
                    let remaining = deadline.saturating_duration_since(Instant::now());
                    if remaining.is_zero() {
                        anyhow::bail!(
                            "ACP {method} timed out after {}s",
                            timeout.as_secs()
                        );
                    }
                    let (guard_out, wait) =
                        shared.arrived.wait_timeout(guard, remaining).unwrap();
                    guard = guard_out;
                    if wait.timed_out() && !guard.contains_key(&key) {
                        anyhow::bail!(
                            "ACP {method} timed out after {}s",
                            timeout.as_secs()
                        );
                    }
                }
            };

        // 1. initialize + version negotiation.
        send(
            0,
            "initialize",
            serde_json::json!({
                "protocolVersion": ACP_PROTOCOL_VERSION,
                "clientCapabilities": {
                    "fs": {"readTextFile": false, "writeTextFile": false},
                    "terminal": false,
                },
                "clientInfo": {
                    "name": "loom",
                    "title": "Loom",
                    "version": env!("CARGO_PKG_VERSION"),
                },
            }),
        )?;
        let init = wait_response(0, "initialize", timeout)?;
        let version = init.get("protocolVersion").and_then(Value::as_u64);
        if version != Some(ACP_PROTOCOL_VERSION) {
            anyhow::bail!(
                "agent speaks unsupported protocol version {} (client speaks ACP v{})",
                version
                    .map(|v| v.to_string())
                    .unwrap_or_else(|| "unknown".to_string()),
                ACP_PROTOCOL_VERSION
            );
        }

        // 2. session/new.
        let cwd = match &config.cwd {
            Some(dir) => dir.clone(),
            None => std::env::current_dir()
                .ok()
                .and_then(|p| p.to_str().map(str::to_string))
                .unwrap_or_default(),
        };
        send(
            1,
            "session/new",
            serde_json::json!({ "cwd": cwd, "mcpServers": [] }),
        )?;
        let new_session = wait_response(1, "session/new", timeout)?;
        let session_id = new_session
            .get("sessionId")
            .and_then(Value::as_str)
            .ok_or_else(|| anyhow::anyhow!("session/new response missing sessionId"))?
            .to_string();

        // 3. session/prompt; on timeout, cancel first so a well-behaved
        // agent can still report the `cancelled` stop reason.
        send(
            2,
            "session/prompt",
            serde_json::json!({
                "sessionId": session_id,
                "prompt": [{ "type": "text", "text": prompt }],
            }),
        )?;
        let prompt_result = match wait_response(2, "session/prompt", timeout) {
            Ok(result) => result,
            Err(_) => {
                let _ = send(
                    3,
                    "session/cancel",
                    serde_json::json!({ "sessionId": session_id }),
                );
                // session/cancel is a notification (no response); give the
                // agent a short grace period, then report the timeout.
                match wait_response(2, "session/prompt", Duration::from_secs(2)) {
                    Ok(result) => result,
                    Err(_) => anyhow::bail!("ACP session/prompt timed out"),
                }
            }
        };
        let stop_reason = prompt_result
            .get("stopReason")
            .and_then(Value::as_str)
            .ok_or_else(|| anyhow::anyhow!("prompt response missing stopReason"))?
            .to_string();

        let acc = shared.transcript.lock().unwrap();
        Ok(AcpTranscript {
            session_id,
            stop_reason,
            agent_text: acc.text.clone(),
            tool_calls: acc
                .tools
                .iter()
                .map(|(_, title, status)| format!("{title} [{status}]"))
                .collect(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::{
        build_request, parse_message, AcpClientConfig, AcpIncoming, AcpProtocolClient,
    };

    #[test]
    fn parse_classifies_responses_notifications_and_requests() {
        match parse_message(r#"{"jsonrpc":"2.0","id":1,"result":{"stopReason":"end_turn"}}"#) {
            AcpIncoming::Response { id, .. } => assert_eq!(id, serde_json::json!(1)),
            other => panic!("expected Response, got {other:?}"),
        }
        match parse_message(r#"{"jsonrpc":"2.0","method":"session/update","params":{}}"#) {
            AcpIncoming::Notification { method, .. } => {
                assert_eq!(method, "session/update")
            }
            other => panic!("expected Notification, got {other:?}"),
        }
        match parse_message(r#"{"jsonrpc":"2.0","id":5,"method":"session/request_permission","params":{}}"#) {
            AcpIncoming::Request { id, method, .. } => {
                assert_eq!(id, serde_json::json!(5));
                assert_eq!(method, "session/request_permission");
            }
            other => panic!("expected Request, got {other:?}"),
        }
        match parse_message("not json") {
            AcpIncoming::Invalid => {}
            other => panic!("expected Invalid, got {other:?}"),
        }
    }

    #[test]
    fn build_request_shapes_the_jsonrpc_envelope() {
        let line = build_request(2, "session/prompt", serde_json::json!({"a": 1}));
        let value: serde_json::Value = serde_json::from_str(&line).unwrap();
        assert_eq!(value["jsonrpc"], "2.0");
        assert_eq!(value["id"], 2);
        assert_eq!(value["method"], "session/prompt");
        assert_eq!(value["params"]["a"], 1);
    }

    fn fake_server() -> Option<std::path::PathBuf> {
        let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests")
            .join("fixtures")
            .join("fake_acp_server.py");
        if !path.exists() {
            return None;
        }
        if std::process::Command::new("python3")
            .arg("--version")
            .output()
            .is_err()
        {
            return None;
        }
        Some(path)
    }

    fn config(mode: &str) -> (AcpClientConfig, String) {
        let server = fake_server().expect("fake ACP server fixture + python3 required");
        (
            AcpClientConfig {
                program: "python3".to_string(),
                args: vec![
                    server.to_string_lossy().to_string(),
                    format!("--mode={mode}"),
                ],
                cwd: None,
                timeout_secs: 10,
            },
            server.to_string_lossy().to_string(),
        )
    }

    #[test]
    fn run_prompt_collects_transcript_against_fake_server() {
        let (cfg, _) = config("happy");
        let transcript = AcpProtocolClient::run_prompt(&cfg, "hello agent").unwrap();
        assert_eq!(transcript.session_id, "sess-test-1");
        assert_eq!(transcript.stop_reason, "end_turn");
        assert!(transcript.agent_text.contains("Hello back"));
        assert!(transcript.tool_calls.iter().any(|t| t.contains("Reading file")));
    }

    #[test]
    fn run_prompt_auto_allows_permission_requests() {
        let (cfg, _) = config("permission");
        let transcript = AcpProtocolClient::run_prompt(&cfg, "do the thing").unwrap();
        assert_eq!(transcript.stop_reason, "end_turn");
        assert!(transcript.agent_text.contains("did the thing"));
    }

    #[test]
    fn run_prompt_rejects_protocol_version_mismatch() {
        let (cfg, _) = config("bad-version");
        let err = AcpProtocolClient::run_prompt(&cfg, "hi").unwrap_err();
        assert!(err.to_string().contains("protocol version"));
    }

    #[test]
    fn run_prompt_times_out_on_hung_server() {
        let (mut cfg, _) = config("hang");
        cfg.timeout_secs = 2;
        let err = AcpProtocolClient::run_prompt(&cfg, "hi").unwrap_err();
        assert!(err.to_string().contains("timed out") || err.to_string().contains("timeout"));
    }
}
