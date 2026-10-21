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
#[derive(Debug, Clone, Default)]
pub struct AcpClientConfig {
    /// Agent program (absolute path or PATH name).
    pub program: String,
    /// Arguments for the program.
    pub args: Vec<String>,
    /// Session working directory (absolute). Defaults to current dir.
    pub cwd: Option<String>,
    /// Per-request timeout in seconds.
    pub timeout_secs: u64,
    /// Extra environment variables for the agent process.
    pub env: Vec<(String, String)>,
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
    /// Token usage the agent reported, if any.
    ///
    /// ACP v1 carries usage in two places with different meanings:
    /// the STABLE `usage_update` session notification reports *context*
    /// usage (`used` tokens currently in context, `size` context-window
    /// size) plus an optional cumulative *session* cost; the UNSTABLE
    /// `session/prompt` response may carry per-turn `usage` with
    /// `inputTokens`/`outputTokens` (cumulative across the session's turns).
    ///
    /// Neither is a per-request cost, and a context-window size is not a
    /// turn count — so `context_tokens` and the token counts stay
    /// separate fields, and a run with no counts at all stays empty.
    pub usage: loom_core::UsageCounts,
    /// Which ACP usage source the counts came from, for the ledger.
    ///
    /// "acp:session-usage" — the unstable `usage` object on a
    /// `session/prompt` response. Upstream defines its `inputTokens` /
    /// `outputTokens` as CUMULATIVE across the whole session, so the
    /// label names the session, not a turn.
    /// "acp:session-context-cost" — the stable `usage_update`
    /// notification: a context-window size and the session's cumulative
    /// cost.
    /// Empty — the agent reported no usage.
    pub usage_method: String,
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
    usage: loom_core::UsageCounts,
    usage_method: String,
}

type PendingResponse = (Option<Value>, Option<Value>);

struct Shared {
    responses: Mutex<HashMap<String, PendingResponse>>,
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
                if let Some(entry) = transcript.tools.iter_mut().find(|(id, _, _)| id == call_id) {
                    entry.2 = status.to_string();
                }
            }
        }
        // Stable ACP usage notification: `used` tokens currently in context,
        // `size` context-window size, optional cumulative session `cost`.
        // A context size is NOT a turn count, so `used` lands in
        // `context_tokens` only and is never promoted to input/output.
        // `size` is the window limit, not a usage number, so it is not
        // recorded at all.
        "usage_update" => {
            let used = update.get("used").and_then(Value::as_u64);
            let cost = update
                .get("cost")
                .and_then(|c| c.get("amount"))
                .and_then(Value::as_f64);
            if used.is_some() {
                transcript.usage.context_tokens = used;
            }
            if cost.is_some() {
                transcript.usage.cost_usd = cost;
            }
            if (used.is_some() || cost.is_some()) && transcript.usage_method.is_empty() {
                transcript.usage_method = "acp:session-context-cost".to_string();
            }
        }
        _ => {}
    }
}

/// Read the UNSTABLE `usage` object from a `session/prompt` response:
/// `{ totalTokens, inputTokens, outputTokens, ... }`.
///
/// Upstream ACP defines these as CUMULATIVE across the session ("Total
/// input tokens across all turns"), so the counts describe the session,
/// not the turn — hence the `acp:session-usage` label. One Loom run is
/// one turn today, which makes the two coincide by accident.
///
/// Absent or malformed means "no count" — the caller records the run as
/// unavailable rather than guessing.
fn prompt_turn_usage(prompt_result: &Value) -> Option<loom_core::UsageCounts> {
    let usage = prompt_result.get("usage")?;
    let input = usage.get("inputTokens").and_then(Value::as_u64);
    let output = usage.get("outputTokens").and_then(Value::as_u64);
    if input.is_none() && output.is_none() {
        return None;
    }
    Some(loom_core::UsageCounts {
        input_tokens: input,
        output_tokens: output,
        context_tokens: None,
        cost_usd: None,
    })
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
            let option_id = option.get("optionId").and_then(Value::as_str).unwrap_or("");
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
        let mut command = Command::new(&config.program);
        command
            .args(&config.args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        for (key, value) in &config.env {
            command.env(key, value);
        }
        let mut child = command
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
        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| anyhow::anyhow!("ACP agent has no stdin"))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| anyhow::anyhow!("ACP agent has no stdout"))?;
        let stderr = child.stderr.take();
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
            for line in reader.lines() {
                match line {
                    Ok(line) => {
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
                    Err(_e) => {
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
        let wait_response = |id: u64, method: &str, timeout: Duration| -> anyhow::Result<Value> {
            let key = id.to_string();
            let deadline = Instant::now() + timeout;
            let mut guard = loom_core::lock_ok!(shared.responses);
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
                    anyhow::bail!("ACP {method} timed out after {}s", timeout.as_secs());
                }
                let (guard_out, wait) = shared.arrived.wait_timeout(guard, remaining).unwrap();
                guard = guard_out;
                if wait.timed_out() && !guard.contains_key(&key) {
                    anyhow::bail!("ACP {method} timed out after {}s", timeout.as_secs());
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
            Err(e) => {
                let _ = send(
                    3,
                    "session/cancel",
                    serde_json::json!({ "sessionId": session_id }),
                );
                // session/cancel is a notification (no response); give the
                // agent a short grace period, then report the original
                // failure — a protocol error must not be reported as a
                // timeout.
                match wait_response(2, "session/prompt", Duration::from_secs(2)) {
                    Ok(result) => result,
                    Err(_) => return Err(e),
                }
            }
        };
        let stop_reason = prompt_result
            .get("stopReason")
            .and_then(Value::as_str)
            .ok_or_else(|| anyhow::anyhow!("prompt response missing stopReason"))?
            .to_string();

        let acc = loom_core::lock_ok!(shared.transcript);
        // The unstable per-turn `usage` wins on the input/output legs: it
        // is the only ACP shape that reports turn token counts. The stable
        // notification still contributes the context size and cost.
        let mut usage = acc.usage;
        let mut usage_method = acc.usage_method.clone();
        if let Some(counts) = prompt_turn_usage(&prompt_result) {
            usage.input_tokens = counts.input_tokens;
            usage.output_tokens = counts.output_tokens;
            usage_method = "acp:session-usage".to_string();
        }
        Ok(AcpTranscript {
            session_id,
            stop_reason,
            agent_text: acc.text.clone(),
            tool_calls: acc
                .tools
                .iter()
                .map(|(_, title, status)| format!("{title} [{status}]"))
                .collect(),
            usage,
            usage_method,
        })
    }
}

/// Shape an ACP transcript into a Loom session (manifest + System/User/
/// Assistant messages). Pure; shared by the CLI `acp run` path and the
/// daemon's `acp.dispatch`. `agent` is recorded verbatim in the System
/// message; its basename becomes the tag/source_tool/model_provider name.
/// `topic_override` replaces the default topic (prompt, ≤80 chars).
pub fn build_acp_session(
    transcript: &AcpTranscript,
    prompt: &str,
    agent: &str,
    topic_override: Option<String>,
) -> (loom_core::SessionManifest, Vec<loom_core::Message>) {
    let prompt = prompt.trim();
    let agent_name = std::path::Path::new(agent)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or(agent)
        .to_string();
    let session_id = format!("acp-{}", &uuid::Uuid::new_v4().to_string()[..8]);
    let topic = topic_override.unwrap_or_else(|| prompt.chars().take(80).collect());
    let now = loom_core::rfc3339_now();

    let mut assistant = if transcript.agent_text.trim().is_empty() {
        "(agent produced no text output)".to_string()
    } else {
        transcript.agent_text.trim().to_string()
    };
    if !transcript.tool_calls.is_empty() {
        assistant.push_str("\n\nTools:\n");
        for tool in &transcript.tool_calls {
            assistant.push_str(&format!("- {tool}\n"));
        }
    }
    assistant.push_str(&format!("\n[stop reason: {}]", transcript.stop_reason));

    let messages = vec![
        loom_core::Message {
            id: uuid::Uuid::new_v4().to_string(),
            role: loom_core::MessageRole::System,
            content: format!(
                "ACP protocol run\n  agent: {agent}\n  agent_session: {}\n  stop_reason: {}",
                transcript.session_id, transcript.stop_reason,
            ),
            tool_calls: None,
            timestamp: now.clone(),
            metadata: Default::default(),
        },
        loom_core::Message {
            id: uuid::Uuid::new_v4().to_string(),
            role: loom_core::MessageRole::User,
            content: prompt.to_string(),
            tool_calls: None,
            timestamp: now.clone(),
            metadata: Default::default(),
        },
        loom_core::Message {
            id: uuid::Uuid::new_v4().to_string(),
            role: loom_core::MessageRole::Assistant,
            content: assistant,
            tool_calls: None,
            timestamp: now.clone(),
            metadata: Default::default(),
        },
    ];

    let manifest = loom_core::SessionManifest {
        session_id,
        source_tool: format!("loom-acp/{agent_name}"),
        source_version: env!("CARGO_PKG_VERSION").to_string(),
        exported_at: now,
        topic,
        message_count: messages.len() as u32,
        active_skills: vec![],
        active_mcp_servers: vec![],
        working_directory: std::env::current_dir()
            .ok()
            .and_then(|p| p.to_str().map(String::from))
            .unwrap_or_default(),
        model_provider: agent_name.clone(),
        files_modified: vec![],
        compatibility_note: Some(format!(
            "Canonical ACP run against '{agent}' (stop={})",
            transcript.stop_reason
        )),
        tags: vec![
            "acp".to_string(),
            "acp-protocol".to_string(),
            format!("agent:{agent_name}"),
        ],
        summarized_at: None,
        summarized_message_count: None,
    };
    (manifest, messages)
}

#[cfg(test)]
mod tests {
    use super::{
        build_acp_session, build_request, parse_message, AcpClientConfig, AcpIncoming,
        AcpProtocolClient, AcpTranscript,
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
        match parse_message(
            r#"{"jsonrpc":"2.0","id":5,"method":"session/request_permission","params":{}}"#,
        ) {
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
                env: Vec::new(),
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
        assert!(transcript
            .tool_calls
            .iter()
            .any(|t| t.contains("Reading file")));
    }

    #[test]
    fn run_prompt_reports_no_usage_when_the_agent_reports_none() {
        let (cfg, _) = config("happy");
        let transcript = AcpProtocolClient::run_prompt(&cfg, "hello agent").unwrap();
        // No usage in the response and no usage_update notification: the
        // counts stay empty rather than defaulting to zero.
        assert_eq!(transcript.usage.input_tokens, None);
        assert_eq!(transcript.usage.output_tokens, None);
        assert_eq!(transcript.usage.context_tokens, None);
        assert!(transcript.usage_method.is_empty());
    }

    #[test]
    fn run_prompt_reads_the_unstable_session_usage_and_the_stable_context_usage() {
        let (cfg, _) = config("usage");
        let transcript = AcpProtocolClient::run_prompt(&cfg, "hello agent").unwrap();
        assert_eq!(transcript.usage.input_tokens, Some(1200));
        assert_eq!(transcript.usage.output_tokens, Some(300));
        // `used` (tokens currently in context) is context usage, not a turn
        // count; `size` (the window limit) is a model property, not usage.
        assert_eq!(transcript.usage.context_tokens, Some(4321));
        assert_eq!(transcript.usage.cost_usd, Some(0.0123));
        assert_eq!(transcript.usage_method, "acp:session-usage");
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

    #[test]
    fn run_prompt_reports_the_agents_error_not_a_timeout() {
        let (cfg, _) = config("error");
        let err = AcpProtocolClient::run_prompt(&cfg, "hi").unwrap_err();
        let message = err.to_string();
        assert!(
            message.contains("Authentication required"),
            "expected the agent's error, got: {message}"
        );
        assert!(
            !message.contains("timed out"),
            "a protocol error must not be reported as a timeout: {message}"
        );
    }

    fn transcript() -> AcpTranscript {
        AcpTranscript {
            session_id: "sess-abc".to_string(),
            stop_reason: "end_turn".to_string(),
            agent_text: "Hello back".to_string(),
            tool_calls: vec!["Reading file [completed]".to_string()],
            usage: loom_core::UsageCounts::default(),
            usage_method: String::new(),
        }
    }

    #[test]
    fn build_acp_session_shapes_messages_manifest_and_tags() {
        let (manifest, messages) = build_acp_session(
            &transcript(),
            "  hello agent, please help  ",
            "/usr/bin/fake-agent",
            None,
        );
        assert_eq!(messages.len(), 3);
        let roles: Vec<String> = messages.iter().map(|m| format!("{:?}", m.role)).collect();
        assert_eq!(roles, vec!["System", "User", "Assistant"]);
        assert_eq!(messages[1].content, "hello agent, please help");
        assert!(messages[0].content.contains("agent: /usr/bin/fake-agent"));
        assert!(messages[2].content.contains("Hello back"));
        assert!(messages[2].content.contains("Reading file [completed]"));
        assert_eq!(manifest.topic, "hello agent, please help");
        assert_eq!(manifest.source_tool, "loom-acp/fake-agent");
        assert!(manifest.tags.contains(&"acp-protocol".to_string()));
        assert!(manifest.tags.contains(&"agent:fake-agent".to_string()));
        assert!(manifest.session_id.starts_with("acp-"));
    }

    #[test]
    fn build_acp_session_truncates_long_topics_and_handles_empty_text() {
        let mut t = transcript();
        t.agent_text = String::new();
        t.tool_calls = vec![];
        let long_prompt = "p".repeat(200);
        let (manifest, messages) = build_acp_session(&t, &long_prompt, "agent", None);
        assert!(manifest.topic.chars().count() <= 80);
        assert!(messages[2].content.contains("no text output"));
    }

    #[test]
    fn build_acp_session_honors_topic_override() {
        let (manifest, _) = build_acp_session(
            &transcript(),
            "a long prompt that would otherwise be the topic",
            "claude-code",
            Some("Custom topic".to_string()),
        );
        assert_eq!(manifest.topic, "Custom topic");
        assert!(manifest.tags.contains(&"agent:claude-code".to_string()));
    }
}
