use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DispatchResult {
    pub success: bool,
    pub message: String,
    pub detail: Option<String>,
}

#[allow(dead_code)]
pub trait Dispatcher: Send + Sync {
    fn name(&self) -> &'static str;
    fn description(&self) -> &'static str;
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
        s.replace('\'', "'\\''")
    }
}

impl Dispatcher for TerminalLaunch {
    fn name(&self) -> &'static str {
        "Terminal Launch"
    }

    fn description(&self) -> &'static str {
        "Open a new terminal window with the agent"
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
                    detail: None,
                };
            }
        };

        let (launcher, arg) = match Self::find_terminal_launcher() {
            Some(l) => l,
            None => {
                return DispatchResult {
                    success: false,
                    message: "No terminal emulator found".to_string(),
                    detail: None,
                };
            }
        };

        let task_id = task.pointer("/task_id").and_then(Value::as_str).unwrap_or("");
        let title = task.pointer("/title").and_then(Value::as_str).unwrap_or("");

        if !Self::is_safe_task_id(task_id) {
            return DispatchResult {
                success: false,
                message: "Invalid task ID: contains unsafe characters".to_string(),
                detail: None,
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
                detail: None,
            };
        }

        let command = format!("cd '{}' && loom-cli workflow show '{}' && exec {}", Self::shell_escape(&cwd), Self::shell_escape(task_id), agent);

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
                detail: Some(format!("Task: {}", title)),
            },
            Err(e) => DispatchResult {
                success: false,
                message: format!("Failed to launch terminal: {}", e),
                detail: None,
            },
        }
    }
}

#[allow(dead_code)]
pub struct ACPDispatch {
    daemon_reachable: bool,
}

#[allow(dead_code)]
impl ACPDispatch {
    pub fn new(daemon_reachable: bool) -> Self {
        Self { daemon_reachable }
    }
}

impl Dispatcher for ACPDispatch {
    fn name(&self) -> &'static str {
        "ACP Dispatch"
    }

    fn description(&self) -> &'static str {
        "Dispatch via daemon ACP protocol"
    }

    fn shortcut(&self) -> char {
        'a'
    }

    fn available(&self) -> bool {
        self.daemon_reachable
    }

    fn dispatch(&self, _task: &serde_json::Value) -> DispatchResult {
        DispatchResult {
            success: false,
            message: "ACP dispatch requires daemon IPC (not yet wired)".to_string(),
            detail: Some("Use Terminal Launch for now".to_string()),
        }
    }
}

pub struct EmbeddedPane;

impl Dispatcher for EmbeddedPane {
    fn name(&self) -> &'static str {
        "Embedded Pane"
    }

    fn description(&self) -> &'static str {
        "Run agent inline in TUI (future)"
    }

    fn shortcut(&self) -> char {
        'e'
    }

    fn available(&self) -> bool {
        false
    }

    fn dispatch(&self, _task: &serde_json::Value) -> DispatchResult {
        DispatchResult {
            success: false,
            message: "Embedded Pane is not yet implemented".to_string(),
            detail: None,
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
            detail: Some("detail".to_string()),
        };
        let json = serde_json::to_string(&result).unwrap();
        assert!(json.contains("success"));
        assert!(json.contains("message"));
    }

    #[test]
    fn test_acp_dispatch_not_available() {
        let dispatch = ACPDispatch::new(false);
        assert!(!dispatch.available());
        assert_eq!(dispatch.name(), "ACP Dispatch");
        assert_eq!(dispatch.shortcut(), 'a');
    }

    #[test]
    fn test_embedded_pane_not_available() {
        let dispatch = EmbeddedPane;
        assert!(!dispatch.available());
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
