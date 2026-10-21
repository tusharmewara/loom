use chrono;
use loom_runtime::RuntimeRegistry;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::env;

pub const DISPATCHER_COUNT: usize = 4;

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
        Self {
            success: true,
            message,
            run_id,
        }
    }

    fn err(message: String) -> Self {
        Self {
            success: false,
            message,
            run_id: None,
        }
    }
}

/// Supported terminal applications for launching new tabs/windows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TerminalApp {
    Terminal,  // macOS Terminal.app
    Iterm2,    // iTerm2
    Ghostty,   // Ghostty
    Wezterm,   // WezTerm
    Alacritty, // Alacritty
    Kitty,     // Kitty
    Unknown,
}

impl TerminalApp {
    /// Detect the current terminal application from environment variables.
    pub fn detect() -> Self {
        // Check TERM_PROGRAM first (most reliable on macOS)
        if let Ok(term_program) = env::var("TERM_PROGRAM") {
            match term_program.to_lowercase().as_str() {
                "iterm.app" | "iterm2" => return TerminalApp::Iterm2,
                "apple_terminal" => return TerminalApp::Terminal,
                "ghostty" => return TerminalApp::Ghostty,
                "wezterm" => return TerminalApp::Wezterm,
                "vscode" => return TerminalApp::Unknown, // VS Code integrated terminal - can't easily open new tab
                _ => {}
            }
        }

        // Check TERM for additional clues
        if let Ok(term) = env::var("TERM") {
            let term_lower = term.to_lowercase();
            if term_lower.contains("iterm") || term_lower.contains("xterm-256color") {
                // Could be iTerm2 or Terminal.app - check parent process
                if Self::is_parent_iterm2() {
                    return TerminalApp::Iterm2;
                }
            }
            if term_lower.contains("ghostty") {
                return TerminalApp::Ghostty;
            }
            if term_lower.contains("wezterm") {
                return TerminalApp::Wezterm;
            }
            if term_lower.contains("alacritty") {
                return TerminalApp::Alacritty;
            }
            if term_lower.contains("kitty") {
                return TerminalApp::Kitty;
            }
        }

        // Check parent process name as fallback
        if Self::is_parent_iterm2() {
            return TerminalApp::Iterm2;
        }
        if Self::is_parent_ghostty() {
            return TerminalApp::Ghostty;
        }
        if Self::is_parent_wezterm() {
            return TerminalApp::Wezterm;
        }

        TerminalApp::Terminal // Default fallback
    }

    fn is_parent_iterm2() -> bool {
        Self::parent_process_contains("iterm")
    }

    fn is_parent_ghostty() -> bool {
        Self::parent_process_contains("ghostty")
    }

    fn is_parent_wezterm() -> bool {
        Self::parent_process_contains("wezterm")
    }

    fn parent_process_contains(name: &str) -> bool {
        // Read /proc/<ppid>/comm on Linux or use ps on macOS
        #[cfg(target_os = "macos")]
        {
            if let Ok(output) = std::process::Command::new("ps")
                .args(["-p", &std::process::id().to_string(), "-o", "ppid="])
                .output()
            {
                if let Ok(ppid_str) = String::from_utf8(output.stdout) {
                    if let Ok(ppid) = ppid_str.trim().parse::<u32>() {
                        if let Ok(output) = std::process::Command::new("ps")
                            .args(["-p", &ppid.to_string(), "-o", "comm="])
                            .output()
                        {
                            if let Ok(comm) = String::from_utf8(output.stdout) {
                                return comm.to_lowercase().contains(name);
                            }
                        }
                    }
                }
            }
        }
        #[cfg(target_os = "linux")]
        {
            if let Ok(comm) =
                std::fs::read_to_string(format!("/proc/{}/comm", nix::unistd::getppid()))
            {
                return comm.to_lowercase().contains(name);
            }
        }
        false
    }

    /// Get the launcher command and args for this terminal app.
    pub fn launcher(&self) -> Option<(&'static str, Vec<&'static str>)> {
        match self {
            TerminalApp::Terminal => {
                if cfg!(target_os = "macos") && TerminalLaunch::command_exists("osascript") {
                    Some(("osascript", vec!["-e"]))
                } else {
                    None
                }
            }
            TerminalApp::Iterm2 => {
                if cfg!(target_os = "macos") && TerminalLaunch::command_exists("osascript") {
                    Some(("osascript", vec!["-e"]))
                } else {
                    None
                }
            }
            TerminalApp::Ghostty => {
                if TerminalLaunch::command_exists("ghostty") {
                    Some(("ghostty", vec!["--new-tab"]))
                } else {
                    None
                }
            }
            TerminalApp::Wezterm => {
                if TerminalLaunch::command_exists("wezterm") {
                    Some(("wezterm", vec!["cli", "spawn"]))
                } else {
                    None
                }
            }
            TerminalApp::Alacritty => {
                if TerminalLaunch::command_exists("alacritty") {
                    Some(("alacritty", vec!["--new-tab"])) // Requires alacritty msg command or specific version
                } else {
                    None
                }
            }
            TerminalApp::Kitty => {
                if TerminalLaunch::command_exists("kitty") {
                    Some(("kitty", vec!["@launch", "--type=tab"]))
                } else {
                    None
                }
            }
            TerminalApp::Unknown => None,
        }
    }

    /// Generate the command to open a new tab/window with the given command.
    pub fn build_command(&self, command: &str, cwd: &str) -> Option<String> {
        let cwd_escaped = TerminalLaunch::shell_escape(cwd);
        let cmd_escaped = TerminalLaunch::shell_escape(command);
        let full_cmd = format!("cd '{}' && {}", cwd_escaped, cmd_escaped);

        match self {
            TerminalApp::Terminal => Some(format!(
                r#"tell application "Terminal" to do script "{}""#,
                full_cmd.replace('"', r#"\""#)
            )),
            TerminalApp::Iterm2 => {
                // iTerm2: create a new tab in the current window
                Some(format!(
                    r#"tell application "iTerm2" to tell current window to create tab with default profile command "{}""#,
                    full_cmd.replace('"', r#"\""#)
                ))
            }
            TerminalApp::Ghostty => {
                // Ghostty: --new-tab with command
                Some(format!("bash -c '{}'", full_cmd))
            }
            TerminalApp::Wezterm => {
                // WezTerm: wezterm cli spawn --cwd <dir> <cmd>
                Some(format!("bash -c '{}'", full_cmd))
            }
            TerminalApp::Alacritty => {
                // Alacritty: requires alacritty msg or --new-tab (newer versions)
                Some(format!("bash -c '{}'", full_cmd))
            }
            TerminalApp::Kitty => {
                // Kitty: kitty @ launch --type=tab --cwd <dir> <cmd>
                Some(full_cmd)
            }
            TerminalApp::Unknown => None,
        }
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
    if resp
        .get("status")
        .and_then(Value::as_str)
        .unwrap_or("error")
        == "ok"
    {
        let data = resp.get("data");
        DispatchResult::ok(
            data.and_then(|d| d.get("message"))
                .and_then(Value::as_str)
                .unwrap_or("dispatched")
                .to_string(),
            data.and_then(|d| d.get("run_id"))
                .and_then(Value::as_str)
                .map(str::to_string),
        )
    } else {
        DispatchResult::err(format!(
            "Daemon error: {}",
            resp.get("error")
                .and_then(Value::as_str)
                .unwrap_or("unknown error")
        ))
    }
}

/// Extract a dispatch target from a task payload: required
/// `task_id`/`agent_id` plus optional `prior_sessions` (non-string
/// and blank entries dropped). Returns `None` when the required ids
/// are missing, so callers fail before touching the socket.
pub fn dispatch_target(task: &Value) -> Option<(String, String, Vec<String>)> {
    let task_id = task
        .pointer("/task_id")
        .and_then(Value::as_str)
        .unwrap_or("");
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
        loom_core::find_binary(cmd).is_ok()
    }

    fn find_terminal_launcher() -> Option<TerminalApp> {
        let detected = TerminalApp::detect();
        if detected.launcher().is_some() {
            Some(detected)
        } else {
            // Fallback to generic terminals
            for cmd in &["x-terminal-emulator", "gnome-terminal", "xterm"] {
                if Self::command_exists(cmd) {
                    return Some(TerminalApp::Unknown);
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
                    "No agent binary found on PATH (opencode, claude-code, jcode, crush)"
                        .to_string(),
                );
            }
        };

        let terminal_app = match Self::find_terminal_launcher() {
            Some(app) => app,
            None => {
                return DispatchResult::err("No supported terminal emulator found".to_string());
            }
        };

        let task_id = task
            .pointer("/task_id")
            .and_then(Value::as_str)
            .unwrap_or("");
        let _title = task.pointer("/title").and_then(Value::as_str).unwrap_or("");

        if !Self::is_safe_task_id(task_id) {
            return DispatchResult::err("Invalid task ID: contains unsafe characters".to_string());
        }

        let cwd = env::current_dir()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_default();

        let cwd_has_metachar = cwd.contains([
            ';', '|', '&', '$', '`', '(', ')', '{', '}', '<', '>', '!', '\\',
        ]);
        if cwd_has_metachar {
            return DispatchResult::err("Working directory contains unsafe characters".to_string());
        }

        let command = format!(
            "cd '{}' && loom workflow show '{}' && exec {}",
            Self::shell_escape(&cwd),
            Self::shell_escape(task_id),
            agent
        );

        let script = match terminal_app.build_command(&command, &cwd) {
            Some(s) => s,
            None => {
                return DispatchResult::err(format!(
                    "Unsupported terminal app: {:?}",
                    terminal_app
                ));
            }
        };

        let (launcher, args) = match terminal_app.launcher() {
            Some((l, a)) => (l, a),
            None => {
                return DispatchResult::err("Terminal launcher not available".to_string());
            }
        };

        let mut cmd = std::process::Command::new(launcher);
        for arg in args {
            cmd.arg(arg);
        }
        cmd.arg(&script);

        match cmd.spawn() {
            Ok(child) => DispatchResult::ok(
                format!(
                    "Opened {} in {} tab (pid {})",
                    agent,
                    terminal_app_name(&terminal_app),
                    child.id()
                ),
                None,
            ),
            Err(e) => DispatchResult::err(format!("Failed to launch terminal: {e}")),
        }
    }
}

fn terminal_app_name(app: &TerminalApp) -> &'static str {
    match app {
        TerminalApp::Terminal => "Terminal.app",
        TerminalApp::Iterm2 => "iTerm2",
        TerminalApp::Ghostty => "Ghostty",
        TerminalApp::Wezterm => "WezTerm",
        TerminalApp::Alacritty => "Alacritty",
        TerminalApp::Kitty => "Kitty",
        TerminalApp::Unknown => "terminal",
    }
}

pub struct ACPDispatch {
    daemon_reachable: bool,
    socket_path: Option<String>,
}

impl ACPDispatch {
    pub fn new(daemon_reachable: bool, socket_path: Option<String>) -> Self {
        Self {
            daemon_reachable,
            socket_path,
        }
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
        let task_id = task
            .pointer("/task_id")
            .and_then(Value::as_str)
            .unwrap_or("");
        if task_id.is_empty() {
            return DispatchResult::err("task_id required".to_string());
        }
        DispatchResult::ok(format!("Opened task {task_id} in embedded pane"), None)
    }
}

/// Runtime-aware dispatcher that uses the runtime registry.
pub struct RuntimeDispatch {
    pub runtime_registry: RuntimeRegistry,
    pub default_runtime: String,
    pub loom_dir: String,
}

impl RuntimeDispatch {
    pub fn new(
        runtime_registry: RuntimeRegistry,
        default_runtime: String,
        loom_dir: String,
    ) -> Self {
        Self {
            runtime_registry,
            default_runtime,
            loom_dir,
        }
    }

    /// Get the runtime to use, falling back to default.
    fn get_runtime(&self, task: &serde_json::Value) -> Option<&dyn loom_runtime::Runtime> {
        // Check if task has a runtime specified
        if let Some(runtime_name) = task.pointer("/runtime").and_then(|v| v.as_str()) {
            self.runtime_registry.get(runtime_name)
        } else {
            self.runtime_registry.get(&self.default_runtime)
        }
    }

    /// Get launch config from the tool's GenericAdapter.
    fn get_launch_config(&self, agent_id: &str) -> Option<loom_runtime::LaunchConfig> {
        loom_generic_adapter::GenericAdapter::new(&self.loom_dir, agent_id)
            .ok()
            .and_then(|adapter| adapter.launch_config())
    }
}

/// Create a default runtime registry with all available runtimes.
pub fn default_runtime_registry() -> loom_runtime::RuntimeRegistry {
    let mut registry = loom_runtime::RuntimeRegistry::new();
    let _ = registry.register(Box::new(loom_runtime::ProcessRuntime::new()));
    #[cfg(feature = "docker")]
    {
        let _ = registry.register(Box::new(loom_runtime::ContainerRuntime::docker()));
    }
    #[cfg(feature = "podman")]
    {
        let _ = registry.register(Box::new(loom_runtime::ContainerRuntime::podman()));
    }
    #[cfg(feature = "kubernetes")]
    {
        let _ = registry.register(Box::new(loom_runtime::ContainerRuntime::kubernetes()));
    }
    registry
}

impl Dispatcher for RuntimeDispatch {
    fn name(&self) -> &'static str {
        "Runtime Dispatch"
    }

    fn shortcut(&self) -> char {
        'r'
    }

    fn available(&self) -> bool {
        true
    }

    fn dispatch(&self, task: &serde_json::Value) -> DispatchResult {
        let Some((task_id, agent_id, _prior_sessions)) = dispatch_target(task) else {
            return DispatchResult::err("task_id and agent_id required".to_string());
        };

        let Some(runtime) = self.get_runtime(task) else {
            return DispatchResult::err("No runtime available".to_string());
        };

        // Get launch config from tool's TOML
        let launch_config =
            self.get_launch_config(&agent_id)
                .unwrap_or_else(|| loom_runtime::LaunchConfig {
                    runtime: runtime.name().to_string(),
                    command: agent_id.to_string(),
                    args: vec![],
                    env: std::collections::HashMap::new(),
                    working_dir: None,
                    image: None,
                    network: None,
                    auto_remove: true,
                });

        // Create a minimal task for runtime execution
        let runtime_task = loom_core::Task {
            task_id: task_id.clone(),
            title: task
                .pointer("/title")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string(),
            description: "".to_string(),
            phase: loom_core::TaskPhase::Plan,
            phase_history: vec![],
            prd_path: None,
            context_files: vec![],
            routed_provider: None,
            files_modified: vec![],
            complexity_score: None,
            routing_decision: None,
            last_agent: None,
            linked_issue: None,
            sub_agents: None,
            phase_results: vec![],
            tags: vec![],
            worktree_path: task
                .pointer("/worktree_path")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string()),
            runtime: None,
            created_at: chrono::Utc::now().to_rfc3339(),
            updated_at: chrono::Utc::now().to_rfc3339(),
            status: loom_core::TaskStatus::Pending,
        };

        // Execute via runtime
        let rt = tokio::runtime::Runtime::new().unwrap();
        match rt.block_on(runtime.run(&launch_config, &runtime_task)) {
            Ok(result) => {
                // The runtime keeps the container alive so it can read the
                // logs (the engine's auto-remove is off), so the caller has to
                // release it. Without this the container outlives the
                // dispatch. Cleanup is best-effort: a container that could
                // not be removed is a leak, not a failed dispatch.
                let cleanup = rt.block_on(runtime.cleanup(&result));
                let message = match cleanup {
                    Ok(()) => format!("Dispatched task {} via {} runtime", task_id, runtime.name()),
                    Err(e) => format!(
                        "Dispatched task {} via {} runtime (cleanup failed: {e})",
                        task_id,
                        runtime.name()
                    ),
                };
                DispatchResult::ok(message, None)
            }
            Err(e) => DispatchResult::err(format!("Runtime dispatch failed: {e}")),
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
        assert!(!TerminalLaunch::command_exists(
            "definitely-not-a-real-binary-xyz"
        ));
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
        let line =
            r#"{"request_id":"tui-1","status":"ok","data":{"dispatched":true,"message":"legacy"}}"#;
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
