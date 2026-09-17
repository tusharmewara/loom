use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::sync::mpsc;
use portable_pty::{native_pty_system, CommandBuilder, PtySize};

pub const DISPATCHER_COUNT: usize = 3;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DispatchResult {
    pub success: bool,
    pub message: String,
    /// Daemon-side run id for tracked ACP dispatches, when known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub run_id: Option<String>,
}

impl DispatchResult {
    fn ok(message: String, run_id: Option<String>) -> Self {
        Self { success: true, message, run_id }
    }

    fn err(message: String) -> Self {
        Self { success: false, message, run_id: None }
    }
}

/// Parse one daemon JSON-line response into a [`DispatchResult`].
/// Pure function so the protocol handling is unit-testable without a
/// live Unix socket.
pub fn parse_daemon_response(line: &str) -> DispatchResult {
    let line = line.trim();
    if line.is_empty() {
        return DispatchResult::err("Empty response from daemon".to_string());
    }
    let resp: Value = match serde_json::from_str(line) {
        Ok(v) => v,
        Err(_) => return DispatchResult::err("Invalid response from daemon".to_string()),
    };
    if resp.get("status").and_then(Value::as_str).unwrap_or("error") == "ok" {
        let data = resp.get("data");
        DispatchResult::ok(
            data
                .and_then(|d| d.get("message"))
                .and_then(Value::as_str)
                .unwrap_or("dispatched")
                .to_string(),
            data
                .and_then(|d| d.get("run_id"))
                .and_then(Value::as_str)
                .map(str::to_string),
        )
    } else {
        DispatchResult::err(format!(
            "Daemon error: {}",
            resp.get("error").and_then(Value::as_str).unwrap_or("unknown error")
        ))
    }
}

/// Extract a dispatch target from a task payload: required
/// `task_id`/`agent_id` plus optional `prior_sessions` (non-string
/// and blank entries dropped). Returns `None` when the required ids
/// are missing, so callers fail before touching the socket.
pub fn dispatch_target(task: &Value) -> Option<(String, String, Vec<String>)> {
    let task_id = task.pointer("/task_id").and_then(Value::as_str).unwrap_or("");
    let agent_id = task
        .pointer("/agent_id")
        .and_then(Value::as_str)
        .unwrap_or("");
    if task_id.is_empty() || agent_id.is_empty() {
        return None;
    }
    let prior_sessions = task
        .pointer("/prior_sessions")
        .and_then(Value::as_array)
        .map(|arr| {
            arr.iter()
                .filter_map(Value::as_str)
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default();
    Some((task_id.to_string(), agent_id.to_string(), prior_sessions))
}

pub trait Dispatcher: Send + Sync {
    fn name(&self) -> &'static str;
    fn shortcut(&self) -> char;
    fn available(&self) -> bool;
    fn dispatch(&self, task: &serde_json::Value) -> DispatchResult;
}

pub struct TerminalLaunch;

impl TerminalLaunch {
    fn find_agent_binary() -> Option<String> {
        let candidates = ["opencode", "claude-code", "jcode", "crush"];
        for bin in &candidates {
            if Self::command_exists(bin) {
                return Some(bin.to_string());
            }
        }
        None
    }

    fn command_exists(cmd: &str) -> bool {
        std::process::Command::new("which")
            .arg(cmd)
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
    }

    fn find_terminal_launcher() -> Option<(&'static str, &'static str)> {
        if cfg!(target_os = "macos") {
            if Self::command_exists("osascript") {
                Some(("osascript", "-e"))
            } else {
                None
            }
        } else {
            for cmd in &["x-terminal-emulator", "gnome-terminal", "xterm"] {
                if Self::command_exists(cmd) {
                    return Some((cmd, ""));
                }
            }
            None
        }
    }

    fn is_safe_task_id(s: &str) -> bool {
        !s.is_empty() && s.chars().all(|c| c.is_alphanumeric() || c == '-')
    }

    fn shell_escape(s: &str) -> String {
        s.replace('\'', "'\\'\'")
    }
}

impl Dispatcher for TerminalLaunch {
    fn name(&self) -> &'static str {
        "Terminal Launch"
    }

    fn shortcut(&self) -> char {
        't'
    }

    fn available(&self) -> bool {
        Self::find_agent_binary().is_some() && Self::find_terminal_launcher().is_some()
    }

    fn dispatch(&self, task: &serde_json::Value) -> DispatchResult {
        let agent = match Self::find_agent_binary() {
            Some(a) => a,
            None => {
                return DispatchResult::err(
                    "No agent binary found on PATH (opencode, claude-code, jcode, crush)".to_string(),
                );
            }
        };

        let (launcher, arg) = match Self::find_terminal_launcher() {
            Some(l) => l,
            None => {
                return DispatchResult::err("No terminal emulator found".to_string());
            }
        };

        let task_id = task.pointer("/task_id").and_then(Value::as_str).unwrap_or("");
        let _title = task.pointer("/title").and_then(Value::as_str).unwrap_or("");

        if !Self::is_safe_task_id(task_id) {
            return DispatchResult::err("Invalid task ID: contains unsafe characters".to_string());
        }

        let cwd = std::env::current_dir()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_default();

        let cwd_has_metachar = cwd.contains([';', '|', '&', '$', '`', '(', ')', '{', '}', '<', '>', '!', '\\']);
        if cwd_has_metachar {
            return DispatchResult::err("Working directory contains unsafe characters".to_string());
        }

        let command = format!("cd '{}' && loom workflow show '{}' && exec {}", Self::shell_escape(&cwd), Self::shell_escape(task_id), agent);

        let mut cmd = std::process::Command::new(launcher);
        if cfg!(target_os = "macos") {
            let script = format!(
                r#"tell application "Terminal" to do script "{}""#,
                command.replace('"', r#"\""#)
            );
            cmd.arg(arg).arg(&script);
        } else {
            cmd.arg(arg).arg(format!("bash -c '{}'", Self::shell_escape(&command)));
        }

        match cmd.spawn() {
            Ok(child) => DispatchResult::ok(
                format!("Opened {} in terminal (pid {})", agent, child.id()),
                None,
            ),
            Err(e) => DispatchResult::err(format!("Failed to launch terminal: {e}")),
        }
    }
}

pub struct ACPDispatch {
    daemon_reachable: bool,
    socket_path: Option<String>,
}

impl ACPDispatch {
    pub fn new(daemon_reachable: bool, socket_path: Option<String>) -> Self {
        Self { daemon_reachable, socket_path }
    }
}

impl Dispatcher for ACPDispatch {
    fn name(&self) -> &'static str {
        "ACP Dispatch"
    }

    fn shortcut(&self) -> char {
        'a'
    }

    fn available(&self) -> bool {
        self.daemon_reachable
    }

    fn dispatch(&self, task: &serde_json::Value) -> DispatchResult {
        let Some((task_id, agent_id, prior_sessions)) = dispatch_target(task) else {
            return DispatchResult::err("task_id and agent_id required".to_string());
        };

        // Try daemon IPC via Unix socket
        if let Some(socket_path) = &self.socket_path {
            use std::io::{Read, Write};
            use std::os::unix::net::UnixStream;

            let request = serde_json::json!({
                "command": "acp.dispatch",
                "request_id": format!("tui-{}", std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_nanos())
                    .unwrap_or(0)),
                "params": {
                    "task_id": task_id,
                    "agent_id": agent_id,
                    "prior_sessions": prior_sessions,
                },
            });

            match UnixStream::connect(socket_path) {
                Ok(mut stream) => {
                    let req_line = serde_json::to_string(&request).unwrap() + "\n";
                    if let Err(e) = stream.write_all(req_line.as_bytes()) {
                        return DispatchResult::err(format!("IPC write failed: {e}"));
                    }
                    // Read one JSON line from the daemon response (daemon keeps connection open)
                    // Use a simple line-by-line read from the stream
                    let mut line = String::new();
                    let mut buf = [0u8; 1];
                    loop {
                        match stream.read(&mut buf) {
                            Ok(0) => break, // EOF
                            Ok(_) => {
                                if buf[0] == b'\n' {
                                    break;
                                }
                                line.push(buf[0] as char);
                            }
                            Err(e) => {
                                return DispatchResult::err(format!("IPC read failed: {e}"));
                            }
                        }
                    }
                    return parse_daemon_response(&line);
                }
                Err(e) => {
                    return DispatchResult::err(format!("Failed to connect to daemon: {e}"));
                }
            }
        }

        DispatchResult::err("ACP dispatch requires daemon IPC — daemon not running".to_string())
    }
}

pub struct EmbeddedPane;

impl Dispatcher for EmbeddedPane {
    fn name(&self) -> &'static str {
        "Embedded Pane"
    }

    fn shortcut(&self) -> char {
        'e'
    }

    fn available(&self) -> bool {
        true
    }

    fn dispatch(&self, task: &serde_json::Value) -> DispatchResult {
        let task_id = task.pointer("/task_id").and_then(Value::as_str).unwrap_or("");
        let agent_id = task.pointer("/agent_id").and_then(Value::as_str).unwrap_or("");
        let _prior_sessions: Vec<String> = task
            .pointer("/prior_sessions")
            .and_then(Value::as_array)
            .map(|arr| {
                arr.iter()
                    .filter_map(Value::as_str)
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .map(str::to_string)
                    .collect()
            })
            .unwrap_or_default();

        if task_id.is_empty() || agent_id.is_empty() {
            return DispatchResult::err("task_id and agent_id required".to_string());
        }

        // This dispatcher returns success with the info needed for the TUI
        // to spawn the PTY session. The actual PTY spawn happens in the TUI app.
        DispatchResult::ok(
            format!("Ready to launch {agent_id} for task {task_id} in embedded pane (PTY)"),
            None,
        )
    }
}

/// PTY-based embedded session state for the TUI
pub struct EmbeddedSession {
    pub task_id: String,
    pub agent_id: String,
    pub pty_pair: Option<Box<dyn portable_pty::MasterPty + Send>>,
    pub child: Option<Box<dyn portable_pty::Child + Send + Sync>>,
    pub log_rx: Option<mpsc::Receiver<String>>,
    pub log_tx: Option<mpsc::Sender<String>>,
    pub stdin_tx: Option<mpsc::Sender<Vec<u8>>>,
}

impl EmbeddedSession {
    /// Spawn a new PTY session for the given agent and task
    pub async fn spawn(task_id: String, agent_id: String) -> anyhow::Result<Self> {
        let pty_system = native_pty_system();
        let pty_pair = pty_system.openpty(PtySize {
            rows: 24,
            cols: 80,
            pixel_width: 0,
            pixel_height: 0,
        })?;

        let mut cmd = CommandBuilder::new(&agent_id);
        
        // Set up environment variables for the task
        cmd.env("LOOM_TASK_ID", &task_id);
        cmd.env("LOOM_AGENTS_DIR", std::env::var("LOOM_AGENTS_DIR").unwrap_or_else(|_| ".loom".to_string()));
        
        // Set working directory
        if let Ok(cwd) = std::env::current_dir() {
            cmd.cwd(cwd);
        }

        let child = pty_pair.slave.spawn_command(cmd)?;

        let (log_tx, log_rx) = mpsc::channel::<String>(256);
        let (stdin_tx, stdin_rx) = mpsc::channel::<Vec<u8>>(256);

        // Spawn reader task for PTY output
        let mut reader = pty_pair.master.try_clone_reader()?;
        let log_tx_clone = log_tx.clone();
        tokio::spawn(async move {
            let mut buf = vec![0u8; 4096];
            loop {
                match reader.read(&mut buf) {
                    Ok(0) => break, // EOF
                    Ok(n) => {
                        let output = String::from_utf8_lossy(&buf[..n]).to_string();
                        if log_tx_clone.send(output).await.is_err() {
                            break;
                        }
                    }
                    Err(_) => break,
                }
            }
        });

        // Spawn writer task for stdin
        let mut writer = pty_pair.master.take_writer()?;
        tokio::spawn(async move {
            let mut stdin_rx = stdin_rx;
            while let Some(data) = stdin_rx.recv().await {
                if writer.write_all(&data).is_err() {
                    break;
                }
                if writer.flush().is_err() {
                    break;
                }
            }
        });

        Ok(Self {
            task_id,
            agent_id,
            pty_pair: Some(pty_pair.master),
            child: Some(child),
            log_rx: Some(log_rx),
            log_tx: Some(log_tx),
            stdin_tx: Some(stdin_tx),
        })
    }

    /// Send input to the PTY
    pub async fn write(&self, data: &[u8]) -> anyhow::Result<()> {
        if let Some(tx) = &self.stdin_tx {
            tx.send(data.to_vec()).await?;
        }
        Ok(())
    }

    /// Resize the PTY
    pub fn resize(&self, rows: u16, cols: u16) -> anyhow::Result<()> {
        if let Some(pty) = &self.pty_pair {
            pty.resize(PtySize {
                rows,
                cols,
                pixel_width: 0,
                pixel_height: 0,
            })?;
        }
        Ok(())
    }

    /// Wait for the child process to exit
    pub async fn wait(&mut self) -> anyhow::Result<i32> {
        if let Some(mut child) = self.child.take() {
            let exit_status = child.wait()?;
            Ok(exit_status.exit_code() as i32)
        } else {
            Ok(-1)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_terminal_launch_name() {
        let dispatch = TerminalLaunch;
        assert_eq!(dispatch.name(), "Terminal Launch");
        assert_eq!(dispatch.shortcut(), 't');
    }

    #[test]
    fn test_command_exists_missing() {
        assert!(!TerminalLaunch::command_exists("definitely-not-a-real-binary-xyz"));
    }

    #[test]
    fn test_dispatch_result_serialization() {
        let result = DispatchResult::ok("ok".to_string(), Some("run-1".to_string()));
        let json = serde_json::to_string(&result).unwrap();
        assert!(json.contains("success"));
        assert!(json.contains("message"));
        assert!(json.contains("run-1"));
    }

    #[test]
    fn test_acp_dispatch_not_available() {
        let dispatch = ACPDispatch::new(false, None);
        assert!(!dispatch.available());
        assert_eq!(dispatch.name(), "ACP Dispatch");
        assert_eq!(dispatch.shortcut(), 'a');
    }

    #[test]
    fn test_embedded_pane_available() {
        let dispatch = EmbeddedPane;
        assert!(dispatch.available());
        assert_eq!(dispatch.name(), "Embedded Pane");
        assert_eq!(dispatch.shortcut(), 'e');
    }

    #[test]
    fn test_is_safe_task_id_valid() {
        assert!(TerminalLaunch::is_safe_task_id("abc-123-def"));
        assert!(TerminalLaunch::is_safe_task_id("550e8400-e29b"));
        assert!(TerminalLaunch::is_safe_task_id("simple"));
    }

    #[test]
    fn test_is_safe_task_id_invalid() {
        assert!(!TerminalLaunch::is_safe_task_id(""));
        assert!(!TerminalLaunch::is_safe_task_id("id; rm -rf /"));
        assert!(!TerminalLaunch::is_safe_task_id("id$(whoami)"));
        assert!(!TerminalLaunch::is_safe_task_id("id`whoami`"));
        assert!(!TerminalLaunch::is_safe_task_id("id | cat"));
    }

    #[test]
    fn test_shell_escape_no_quotes() {
        assert_eq!(TerminalLaunch::shell_escape("/safe/path"), "/safe/path");
    }

    #[test]
    fn test_shell_escape_single_quote() {
        assert_eq!(TerminalLaunch::shell_escape("it's"), "it'\\''s");
    }

    #[test]
    fn test_parse_daemon_response_ok_with_run_id() {
        let line = r#"{"request_id":"tui-1","status":"ok","data":{"dispatched":true,"message":"Dispatched task t to a (run r1)","run_id":"r1"}}"#;
        let result = parse_daemon_response(line);
        assert!(result.success);
        assert_eq!(result.run_id.as_deref(), Some("r1"));
        assert!(result.message.contains("run r1"));
    }

    #[test]
    fn test_parse_daemon_response_ok_without_run_id() {
        let line = r#"{"request_id":"tui-1","status":"ok","data":{"dispatched":true,"message":"legacy"}}"#;
        let result = parse_daemon_response(line);
        assert!(result.success);
        assert_eq!(result.run_id, None);
        assert_eq!(result.message, "legacy");
    }

    #[test]
    fn test_parse_daemon_response_error() {
        let line = r#"{"request_id":"tui-1","status":"error","error":"boom"}"#;
        let result = parse_daemon_response(line);
        assert!(!result.success);
        assert_eq!(result.run_id, None);
        assert!(result.message.contains("boom"));
    }

    #[test]
    fn test_parse_daemon_response_empty_and_garbage() {
        let empty = parse_daemon_response("   \n");
        assert!(!empty.success);
        assert_eq!(empty.run_id, None);
        let garbage = parse_daemon_response("not json at all");
        assert!(!garbage.success);
        assert_eq!(garbage.run_id, None);
    }

    #[test]
    fn test_dispatch_target_full() {
        let task = serde_json::json!({
            "task_id": "t1",
            "agent_id": "a1",
            "prior_sessions": ["s1", 7, "s2", ""],
        });
        let (task_id, agent_id, priors) = dispatch_target(&task).unwrap();
        assert_eq!(task_id, "t1");
        assert_eq!(agent_id, "a1");
        assert_eq!(priors, vec!["s1".to_string(), "s2".to_string()]);
    }

    #[test]
    fn test_dispatch_target_rejects_missing_ids() {
        assert!(dispatch_target(&serde_json::json!({"task_id": "t"})).is_none());
        assert!(dispatch_target(&serde_json::json!({"agent_id": "a"})).is_none());
        assert!(dispatch_target(&serde_json::json!({})).is_none());
    }

    #[test]
    fn test_dispatch_target_allows_no_priors() {
        let task = serde_json::json!({"task_id": "t1", "agent_id": "a1"});
        let (_, _, priors) = dispatch_target(&task).unwrap();
        assert!(priors.is_empty());
    }
}