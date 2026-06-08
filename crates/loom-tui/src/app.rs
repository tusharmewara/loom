use crate::adapter::{CommandResult, LoomCliAdapter};
use crate::theme::Theme;
use anyhow::Result;
use serde_json::Value;
use std::time::Instant;
use tokio::sync::mpsc;

const DAEMON_ACTIONS: &[&str] = &["start", "stop", "restart", "install-service", "uninstall-service"];

pub struct App {
    pub tabs: Vec<&'static str>,
    pub tab_index: usize,
    pub last_refresh: Instant,
    pub status: Value,
    pub scan: Value,
    pub doctor: Value,
    pub daemon: Value,
    pub mcp: Value,
    pub skills: Value,
    pub sessions: Value,
    pub workflow: Value,
    pub hooks: Value,
    pub knowledge: Value,
    pub pulls: Value,
    pub pull_selection_index: usize,
    pub last_error: Option<String>,
    pub action_palette_open: bool,
    pub action_palette_selected: usize,
    pub daemon_actions: Vec<&'static str>,
    pub last_action_result: Option<CommandResult>,
    pub action_in_progress: bool,
    pub help_overlay_open: bool,
    pub scroll_offsets: Vec<usize>,
    pub task_selection_index: usize,
    pub dispatch_menu_open: bool,
    pub dispatch_menu_selected: usize,
    pub command_palette_open: bool,
    pub command_palette_query: String,
    pub command_palette_selected: usize,
    pub theme: Theme,
    pub sidebar_visible: bool,
    pub embedded_task: Option<Value>,
    pub embedded_log: Vec<String>,
    pub embedded_log_scroll: usize,
    pub embedded_log_rx: Option<mpsc::Receiver<String>>,
    pub embedded_child: Option<tokio::process::Child>,
    pub embedded_stdin: Option<tokio::process::ChildStdin>,
    theme_manager: crate::theme::ThemeManager,
    pub adapter: LoomCliAdapter,
}

impl App {
    pub fn new(adapter: LoomCliAdapter) -> Self {
        let theme_manager = crate::theme::ThemeManager::new();
        let theme = theme_manager.current();
        Self {
            tabs: vec!["Dashboard", "Daemon", "Discovery", "MCP", "Skills", "Sessions", "Logs", "Knowledge", "PRs"],
            tab_index: 0,
            last_refresh: Instant::now(),
            status: Value::Null,
            scan: Value::Null,
            doctor: Value::Null,
            daemon: Value::Null,
            mcp: Value::Null,
            skills: Value::Null,
            sessions: Value::Null,
            workflow: Value::Null,
            hooks: Value::Null,
            knowledge: Value::Null,
            pulls: Value::Null,
            pull_selection_index: 0,
            last_error: None,
            action_palette_open: false,
            action_palette_selected: 0,
            daemon_actions: DAEMON_ACTIONS.to_vec(),
            last_action_result: None,
            action_in_progress: false,
            help_overlay_open: false,
            scroll_offsets: vec![0; 9],
            task_selection_index: 0,
            dispatch_menu_open: false,
            dispatch_menu_selected: 0,
            command_palette_open: false,
            command_palette_query: String::new(),
            command_palette_selected: 0,
            theme,
            sidebar_visible: true,
            embedded_task: None,
            embedded_log: Vec::new(),
            embedded_log_scroll: 0,
            embedded_log_rx: None,
            embedded_child: None,
            embedded_stdin: None,
            theme_manager,
            adapter,
        }
    }

    pub fn next_tab(&mut self) {
        self.tab_index = (self.tab_index + 1) % self.tabs.len();
    }

    pub fn prev_tab(&mut self) {
        if self.tab_index == 0 {
            self.tab_index = self.tabs.len() - 1;
        } else {
            self.tab_index -= 1;
        }
    }

    pub fn set_tab_from_digit(&mut self, digit: usize) {
        if digit < self.tabs.len() {
            self.tab_index = digit;
        }
    }

    pub fn scroll_down(&mut self) {
        if self.tab_index < self.scroll_offsets.len() {
            self.scroll_offsets[self.tab_index] += 1;
        }
    }

    pub fn scroll_up(&mut self) {
        if self.tab_index < self.scroll_offsets.len() && self.scroll_offsets[self.tab_index] > 0 {
            self.scroll_offsets[self.tab_index] -= 1;
        }
    }

    pub fn toggle_action_palette(&mut self) {
        self.action_palette_open = !self.action_palette_open;
        if self.action_palette_open {
            self.action_palette_selected = 0;
        }
    }

    pub fn close_action_palette(&mut self) {
        self.action_palette_open = false;
    }

    pub fn action_palette_next(&mut self) {
        if self.action_palette_selected + 1 < self.daemon_actions.len() {
            self.action_palette_selected += 1;
        }
    }

    pub fn action_palette_prev(&mut self) {
        if self.action_palette_selected > 0 {
            self.action_palette_selected -= 1;
        }
    }

    pub fn toggle_help(&mut self) {
        self.help_overlay_open = !self.help_overlay_open;
    }

    pub fn close_help(&mut self) {
        self.help_overlay_open = false;
    }

    pub async fn refresh(&mut self) -> Result<()> {
        // Each fetch is independent — one failure must not skip remaining fields.
        // Assign each value immediately so partial refresh still works.
        if let Ok(v) = self.adapter.fetch_status().await {
            self.status = v;
        }
        if let Ok(v) = self.adapter.fetch_scan().await {
            self.scan = v;
        }
        if let Ok(v) = self.adapter.fetch_doctor().await {
            self.doctor = v;
        }
        if let Ok(v) = self.adapter.fetch_daemon_status().await {
            self.daemon = v;
        }
        if let Ok(v) = self.adapter.fetch_mcp_list().await {
            self.mcp = v;
        }
        if let Ok(v) = self.adapter.fetch_skills_list().await {
            self.skills = v;
        }
        if let Ok(v) = self.adapter.fetch_sessions_list().await {
            self.sessions = v;
        }
        if let Ok(v) = self.adapter.fetch_workflow_list().await {
            self.workflow = v;
        }
        if let Ok(v) = self.adapter.fetch_hooks_list().await {
            self.hooks = v;
        }
        if let Ok(v) = self.adapter.fetch_knowledge_stats().await {
            self.knowledge = v;
        } else if self.knowledge.is_null() {
            self.knowledge = Value::Object(Default::default());
        }
        if let Ok(v) = self.adapter.fetch_pulls_list().await {
            self.pulls = v;
        } else if self.pulls.is_null() {
            self.pulls = Value::Object(Default::default());
        }
        self.last_error = None;
        self.last_refresh = Instant::now();
        self.drain_embedded_log();
        Ok(())
    }

    pub async fn refresh_safe(&mut self) {
        if let Err(e) = self.refresh().await {
            self.last_error = Some(e.to_string());
            self.last_refresh = Instant::now();
        }
    }

    pub async fn execute_selected_action(&mut self) {
        if self.action_palette_selected >= self.daemon_actions.len() {
            return;
        }
        let action = self.daemon_actions[self.action_palette_selected];
        self.action_in_progress = true;
        let result = match action {
            "start" => self.adapter.daemon_start().await,
            "stop" => self.adapter.daemon_stop().await,
            "restart" => self.adapter.daemon_restart().await,
            "install-service" => self.adapter.daemon_install_service().await,
            "uninstall-service" => self.adapter.daemon_uninstall_service().await,
            _ => CommandResult {
                action: action.to_string(),
                success: false,
                message: "unknown action".to_string(),
            },
        };
        self.last_action_result = Some(result);
        self.action_in_progress = false;
        self.close_action_palette();
        self.refresh_safe().await;
    }

    pub async fn run_scan_command(&mut self) {
        self.action_in_progress = true;
        let result = self.adapter.run_scan().await;
        self.last_action_result = Some(result);
        self.action_in_progress = false;
        self.refresh_safe().await;
    }

    pub async fn run_doctor_command(&mut self) {
        self.action_in_progress = true;
        let result = self.adapter.run_doctor().await;
        self.last_action_result = Some(result);
        self.action_in_progress = false;
        self.refresh_safe().await;
    }

    pub fn toggle_sidebar(&mut self) {
        self.sidebar_visible = !self.sidebar_visible;
    }

    pub fn next_theme(&mut self) {
        self.theme = self.theme_manager.cycle_next();
    }

    pub fn prev_theme(&mut self) {
        self.theme = self.theme_manager.cycle_prev();
    }

    pub fn theme_name(&self) -> &'static str {
        self.theme_manager.current_name()
    }

    pub fn task_count(&self) -> usize {
        self.workflow
            .pointer("/tasks")
            .and_then(Value::as_array)
            .map(|a| a.len())
            .unwrap_or(0)
    }

    pub fn selected_task(&self) -> Option<&Value> {
        let tasks = self.workflow.pointer("/tasks").and_then(Value::as_array)?;
        if self.task_selection_index < tasks.len() {
            Some(&tasks[self.task_selection_index])
        } else {
            None
        }
    }

    pub fn selected_pull(&self) -> Option<&Value> {
        let pulls = self.pulls.pointer("/issues").and_then(Value::as_array)?;
        if self.pull_selection_index < pulls.len() {
            Some(&pulls[self.pull_selection_index])
        } else {
            None
        }
    }

    pub fn next_pull(&mut self) {
        if let Some(pulls) = self.pulls.pointer("/issues").and_then(Value::as_array) {
            if !pulls.is_empty() && self.pull_selection_index + 1 < pulls.len() {
                self.pull_selection_index += 1;
            }
        }
    }

    pub fn prev_pull(&mut self) {
        if self.pull_selection_index > 0 {
            self.pull_selection_index -= 1;
        }
    }

    pub fn select_next_task(&mut self) {
        let count = self.task_count();
        if count > 0 {
            self.task_selection_index = (self.task_selection_index + 1) % count;
        }
    }

    pub fn select_prev_task(&mut self) {
        let count = self.task_count();
        if count > 0 {
            if self.task_selection_index == 0 {
                self.task_selection_index = count - 1;
            } else {
                self.task_selection_index -= 1;
            }
        }
    }

    pub fn open_dispatch_menu(&mut self) {
        if self.task_count() > 0 {
            self.dispatch_menu_open = true;
            self.dispatch_menu_selected = 0;
        }
    }

    pub fn close_dispatch_menu(&mut self) {
        self.dispatch_menu_open = false;
    }

    pub fn close_embedded_pane(&mut self) {
        self.embedded_task = None;
        self.embedded_log.clear();
        self.embedded_log_scroll = 0;
        self.embedded_log_rx = None;
        // Kill the child process if still running
        if let Some(mut child) = self.embedded_child.take() {
            let _ = child.start_kill();
        }
        self.embedded_stdin = None;
    }

    /// Drain any pending log lines from the embedded process receiver
    /// into the `embedded_log` buffer (bounded to 5000 lines).
    pub fn drain_embedded_log(&mut self) {
        if let Some(ref mut rx) = self.embedded_log_rx {
            loop {
                match rx.try_recv() {
                    Ok(line) => {
                        self.embedded_log.push(line);
                        if self.embedded_log.len() > 5000 {
                            self.embedded_log.remove(0);
                        }
                    }
                    Err(mpsc::error::TryRecvError::Empty) => break,
                    Err(mpsc::error::TryRecvError::Disconnected) => {
                        self.embedded_log_rx = None;
                        break;
                    }
                }
            }
        }
    }

    pub fn embedded_log_scroll_down(&mut self) {
        let max = self.embedded_log.len().saturating_sub(1);
        if self.embedded_log_scroll < max {
            self.embedded_log_scroll += 1;
        }
    }

    pub fn embedded_log_scroll_up(&mut self) {
        if self.embedded_log_scroll > 0 {
            self.embedded_log_scroll -= 1;
        }
    }

    pub fn dispatch_menu_next(&mut self) {
        if self.dispatch_menu_selected + 1 < crate::dispatch::DISPATCHER_COUNT {
            self.dispatch_menu_selected += 1;
        }
    }

    pub fn dispatch_menu_prev(&mut self) {
        if self.dispatch_menu_selected > 0 {
            self.dispatch_menu_selected -= 1;
        }
    }

    pub fn toggle_command_palette(&mut self) {
        self.command_palette_open = !self.command_palette_open;
        if self.command_palette_open {
            self.command_palette_query.clear();
            self.command_palette_selected = 0;
        }
    }

    pub fn close_command_palette(&mut self) {
        self.command_palette_open = false;
        self.command_palette_query.clear();
        self.command_palette_selected = 0;
    }

    pub fn command_palette_next(&mut self) {
        self.command_palette_selected = self.command_palette_selected.saturating_add(1);
    }

    pub fn command_palette_prev(&mut self) {
        self.command_palette_selected = self.command_palette_selected.saturating_sub(1);
    }

    pub fn command_palette_append(&mut self, c: char) {
        self.command_palette_query.push(c);
        self.command_palette_selected = 0;
    }

    pub fn command_palette_backspace(&mut self) {
        self.command_palette_query.pop();
        self.command_palette_selected = 0;
    }

    pub fn command_palette_count(&self) -> usize {
        use crate::ui::overlays::all_commands;
        all_commands()
            .iter()
            .filter(|cmd| crate::ui::overlays::fuzzy_match(&self.command_palette_query, cmd.name))
            .count()
    }

    pub fn command_palette_selected_name(&self) -> Option<String> {
        use crate::ui::overlays::{all_commands, fuzzy_match};
        let commands = all_commands();
        let filtered: Vec<_> = commands
            .iter()
            .filter(|cmd| fuzzy_match(&self.command_palette_query, cmd.name))
            .collect();
        filtered
            .get(self.command_palette_selected)
            .map(|cmd| cmd.name.to_string())
    }

    pub fn is_task_tab(&self) -> bool {
        self.tab_index == 5 || self.tab_index == 6
    }

    pub async fn execute_named_command(&mut self, cmd_name: &str) -> bool {
        match cmd_name {
            "daemon start" => {
                self.last_action_result = Some(self.adapter.daemon_start().await);
            }
            "daemon stop" => {
                self.last_action_result = Some(self.adapter.daemon_stop().await);
            }
            "daemon restart" => {
                self.last_action_result = Some(self.adapter.daemon_restart().await);
            }
            "daemon install-service" => {
                self.last_action_result = Some(self.adapter.daemon_install_service().await);
            }
            "daemon uninstall-service" => {
                self.last_action_result = Some(self.adapter.daemon_uninstall_service().await);
            }
            "theme next" => self.next_theme(),
            "theme prev" => self.prev_theme(),
            "sidebar toggle" => self.toggle_sidebar(),
            "tab 1" => self.set_tab_from_digit(0),
            "tab 2" => self.set_tab_from_digit(1),
            "tab 3" => self.set_tab_from_digit(2),
            "tab 4" => self.set_tab_from_digit(3),
            "tab 5" => self.set_tab_from_digit(4),
            "tab 6" => self.set_tab_from_digit(5),
            "tab 7" => self.set_tab_from_digit(6),
            "refresh" => self.refresh_safe().await,
            "scan" => {
                self.last_action_result = Some(self.adapter.run_scan().await);
            }
            "doctor" => {
                self.last_action_result = Some(self.adapter.run_doctor().await);
            }
            "help" => self.toggle_help(),
            "quit" => {
                self.close_command_palette();
                return false;
            }
            _ => {}
        }
        true
    }
}
