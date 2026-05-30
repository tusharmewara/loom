use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const DISPATCHER_COUNT: usize = 3;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DispatchResult {
    pub success: bool,
    pub message: String,
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
                return DispatchResult {
                    success: false,
                    message: "No agent binary found on PATH (opencode, claude-code, jcode, crush)".to_string(),
                };
            }
        };

        let (launcher, arg) = match Self::find_terminal_launcher() {
            Some(l) => l,
            None => {
                return DispatchResult {
                    success: false,
                    message: "No terminal emulator found".to_string(),
                };
            }
        };

        let task_id = task.pointer("/task_id").and_then(Value::as_str).unwrap_or("");
        let _title = task.pointer("/title").and_then(Value::as_str).unwrap_or("");

        if !Self::is_safe_task_id(task_id) {
            return DispatchResult {
                success: false,
                message: "Invalid task ID: contains unsafe characters".to_string(),
            };
        }

        let cwd = std::env::current_dir()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_default();

        let cwd_has_metachar = cwd.contains([';', '|', '&', '$', '`', '(', ')', '{', '}', '<', '>', '!', '\\']);
        if cwd_has_metachar {
            return DispatchResult {
                success: false,
                message: "Working directory contains unsafe characters".to_string(),
            };
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
            Ok(child) => DispatchResult {
                success: true,
                message: format!("Opened {} in terminal (pid {})", agent, child.id()),
            },
            Err(e) => DispatchResult {
                success: false,
                message: format!("Failed to launch terminal: {}", e),
            },
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
        let task_id = task.pointer("/task_id").and_then(Value::as_str).unwrap_or("");
        let agent_id = task.pointer("/agent_id").and_then(Value::as_str).unwrap_or("");

        if task_id.is_empty() || agent_id.is_empty() {
            return DispatchResult {
                success: false,
                message: "task_id and agent_id required".to_string(),
            };
        }

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
                },
            });

            match UnixStream::connect(socket_path) {
                Ok(mut stream) => {
                    let req_line = serde_json::to_string(&request).unwrap() + "\n";
                    if let Err(e) = stream.write_all(req_line.as_bytes()) {
                        return DispatchResult {
                            success: false,
                            message: format!("IPC write failed: {e}"),
                        };
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
                                return DispatchResult {
                                    success: false,
                                    message: format!("IPC read failed: {e}"),
                                };
                            }
                        }
                    }
                    if !line.is_empty() {
                        if let Ok(resp) = serde_json::from_str::<serde_json::Value>(line.trim()) {
                            let status = resp.get("status").and_then(|v| v.as_str()).unwrap_or("error");
                            if status == "ok" {
                                let msg = resp.get("data")
                                    .and_then(|d| d.get("message"))
                                    .and_then(|v| v.as_str())
                                    .unwrap_or("dispatched");
                                return DispatchResult {
                                    success: true,
                                    message: msg.to_string(),
                                };
                            } else {
                                let err = resp.get("error")
                                    .and_then(|v| v.as_str())
                                    .unwrap_or("unknown error");
                                return DispatchResult {
                                    success: false,
                                    message: format!("Daemon error: {err}"),
                                };
                            }
                        }
                    }
                    return DispatchResult {
                        success: false,
                        message: "Empty response from daemon".to_string(),
                    };
                }
                Err(e) => {
                    return DispatchResult {
                        success: false,
                        message: format!("Failed to connect to daemon: {e}"),
                    };
                }
            }
        }

        DispatchResult {
            success: false,
            message: "ACP dispatch requires daemon IPC — daemon not running".to_string(),
        }
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
        if task_id.is_empty() {
            return DispatchResult {
                success: false,
                message: "task_id required".to_string(),
            };
        }
        DispatchResult {
            success: true,
            message: format!("Opened task {} in embedded pane", task_id),
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
        let result = DispatchResult {
            success: true,
            message: "ok".to_string(),
        };
        let json = serde_json::to_string(&result).unwrap();
        assert!(json.contains("success"));
        assert!(json.contains("message"));
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
}
