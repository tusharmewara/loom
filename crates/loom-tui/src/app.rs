use crate::adapter::{CommandResult, LoomCliAdapter};
use crate::dispatch::Dispatcher;
use crate::theme::Theme;
use anyhow::Result;
use serde_json::Value;
use std::time::Instant;
use tokio::sync::mpsc;

const DAEMON_ACTIONS: &[&str] = &[
    "start",
    "stop",
    "restart",
    "install-service",
    "uninstall-service",
];

/// Minimum interval between GitHub API polls (PR list / CI status / diff).
/// The TUI refresh loop ticks every second, so GitHub-backed data must not be
/// re-fetched on every tick or it would hammer the GitHub API rate limit.
const PR_POLL_INTERVAL: std::time::Duration = std::time::Duration::from_secs(30);

/// Minimum interval between ACP run-status polls. Runs are long-lived
/// compared to the 1s refresh tick, so polling every tick would spawn a
/// `loom` subprocess per second for no benefit.
const RUN_POLL_INTERVAL: std::time::Duration = std::time::Duration::from_secs(2);

/// A tracked ACP dispatch started from the Orchestrator tab, plus the
/// latest `run.status` payload fetched from the daemon.
#[derive(Clone, Debug, Default)]
pub struct OrchestratorRun {
    pub run_id: String,
    pub task_id: String,
    pub agent: String,
    pub status: Value,
}

/// Terminal run states need no further polling.
fn run_is_terminal(status: &str) -> bool {
    matches!(status, "succeeded" | "failed")
}

/// One-line summary of a tracked run for the Orchestrator status area.
/// Tolerates missing fields (e.g. a freshly dispatched run with no
/// status payload yet).
pub fn format_run_line(run: &OrchestratorRun) -> String {
    let short: String = run.run_id.chars().take(8).collect();
    let status = run
        .status
        .get("status")
        .and_then(Value::as_str)
        .unwrap_or("dispatched");
    let mut line = format!("run {short} · {status}");
    if let Some(code) = run.status.get("exit_code").and_then(Value::as_i64) {
        line.push_str(&format!(" · exit {code}"));
    }
    match run.status.get("session_id").and_then(Value::as_str) {
        Some(session) => {
            let preview: String = session.chars().take(21).collect();
            line.push_str(&format!(" · session {preview}"));
            if session.chars().count() > 21 {
                line.push('…');
            }
        }
        None => line.push_str(" · no session yet"),
    }
    line
}

/// Workflow phases shown as Kanban columns, in board order. Kept in
/// sync with the phase values the daemon writes (`loom-core` Task).
pub(crate) const PHASES: &[&str] = &["Plan", "Implement", "Verify", "Finish"];

/// Task ids for the Review tab, in workflow-list order.
pub fn review_task_ids(workflow: &Value) -> Vec<String> {
    workflow
        .pointer("/tasks")
        .and_then(Value::as_array)
        .map(|tasks| {
            tasks
                .iter()
                .filter_map(|t| t.get("task_id"))
                .filter_map(Value::as_str)
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}

/// Session ids captured for a task's phases (`{task_id}.{phase}`),
/// in sessions-list order. The bare task id itself is not a phase
/// session and is excluded.
pub fn task_session_ids(sessions: &Value, task_id: &str) -> Vec<String> {
    let prefix = format!("{task_id}.");
    sessions
        .pointer("/sessions")
        .and_then(Value::as_array)
        .map(|list| {
            list.iter()
                .filter_map(|s| s.get("session_id"))
                .filter_map(Value::as_str)
                .filter(|id| id.starts_with(&prefix))
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}

/// Agent to dispatch a reviewed task with: the task's recorded
/// `last_agent`, falling back to the Orchestrator-selected agent.
pub fn review_dispatch_agent(task_detail: &Value, fallback: Option<&str>) -> Option<String> {
    task_detail
        .pointer("/task/last_agent")
        .and_then(Value::as_str)
        .map(str::to_string)
        .or_else(|| fallback.map(str::to_string))
}

/// One-line task summary for the Review list.
pub fn format_review_task_line(task: &Value) -> String {
    let phase = task.get("phase").and_then(Value::as_str).unwrap_or("?");
    let status = task.get("status").and_then(Value::as_str).unwrap_or("?");
    let title = task
        .get("title")
        .and_then(Value::as_str)
        .unwrap_or("(untitled)");
    let id: String = task
        .get("task_id")
        .and_then(Value::as_str)
        .map(|s| s.chars().take(8).collect())
        .unwrap_or_else(|| "?".to_string());
    format!("[{phase}] [{status}] {title} ({id})")
}

/// Draft task titles from `workflow draft-plan --json` output, in order.
/// Entries without a title are skipped.
pub fn draft_task_titles(draft: &Value) -> Vec<String> {
    draft
        .pointer("/tasks")
        .and_then(Value::as_array)
        .map(|tasks| {
            tasks
                .iter()
                .filter_map(|t| t.get("title"))
                .filter_map(Value::as_str)
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}
/// Next agent in the list after `current`, wrapping around. Unknown
/// or missing current resolves to the first agent; empty list yields
/// `None`. Pure so the reassignment cycling is unit-testable.
pub fn next_agent_after(current: Option<&str>, agents: &[String]) -> Option<String> {
    if agents.is_empty() {
        return None;
    }
    let next = current
        .and_then(|c| agents.iter().position(|a| a == c))
        .map(|i| (i + 1) % agents.len())
        .unwrap_or(0);
    agents.get(next).cloned()
}

/// Merge the launchable-agent sources from `loom acp agents` output:
/// daemon-registered `/handlers` first, then `/agents_toml` entries,
/// trimmed, de-duplicated, order-preserving.
fn orchestrator_agent_list(acp: &Value) -> Vec<String> {
    let mut agents: Vec<String> = Vec::new();
    let mut push_unique = |name: &str| {
        let name = name.trim();
        if !name.is_empty() && !agents.iter().any(|a| a == name) {
            agents.push(name.to_string());
        }
    };
    if let Some(handlers) = acp.pointer("/handlers").and_then(Value::as_array) {
        for h in handlers.iter().filter_map(Value::as_str) {
            push_unique(h);
        }
    }
    if let Some(entries) = acp.pointer("/agents_toml").and_then(Value::as_array) {
        for e in entries.iter().filter_map(Value::as_str) {
            push_unique(e);
        }
    }
    agents
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OrchestratorField {
    Title,
    Details,
    Agent,
}

impl OrchestratorField {
    fn next(self) -> Self {
        match self {
            Self::Title => Self::Details,
            Self::Details => Self::Agent,
            Self::Agent => Self::Title,
        }
    }

    fn previous(self) -> Self {
        match self {
            Self::Title => Self::Agent,
            Self::Details => Self::Title,
            Self::Agent => Self::Details,
        }
    }
}

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
    pub acp: Value,
    pub hooks: Value,
    pub knowledge: Value,
    pub pulls: Value,
    pub pull_selection_index: usize,
    pub pr_ci_status: Value,
    pub pr_diff: Option<String>,
    pub pr_diff_scroll: usize,
    pub show_diff_view: bool,
    pub configured_repo: Option<String>,
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
    pub orchestrator_title: String,
    pub orchestrator_details: String,
    pub orchestrator_field: OrchestratorField,
    pub orchestrator_agents: Vec<String>,
    pub orchestrator_agent_index: usize,
    pub orchestrator_busy: bool,
    pub orchestrator_status: Option<CommandResult>,
    pub orchestrator_last_run: Option<OrchestratorRun>,
    pub orchestrator_draft: Option<Value>,
    pub review_selection_index: usize,
    pub review_detail: Option<Value>,
    pub review_detail_task_id: Option<String>,
    pub kanban_col: usize,
    pub kanban_row: usize,
    theme_manager: crate::theme::ThemeManager,
    pub adapter: LoomCliAdapter,
    /// Time of the last GitHub-backed poll, used to throttle PR/CI/diff fetches.
    last_pr_poll: Option<Instant>,
    /// Time of the last ACP run-status poll (throttled separately).
    last_run_poll: Option<Instant>,
}

impl App {
    pub fn new(adapter: LoomCliAdapter) -> Self {
        let theme_manager = crate::theme::ThemeManager::new();
        let theme = theme_manager.current();
        Self {
            tabs: vec![
                "Dashboard",
                "Daemon",
                "Discovery",
                "MCP",
                "Skills",
                "Sessions",
                "Logs",
                "Knowledge",
                "PRs",
                "Runtime",
                "Kanban",
                "Graph",
                "Cost",
                "Orchestrator",
                "Review",
            ],
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
            acp: Value::Null,
            hooks: Value::Null,
            knowledge: Value::Null,
            pulls: Value::Null,
            pull_selection_index: 0,
            pr_ci_status: Value::Null,
            pr_diff: None,
            pr_diff_scroll: 0,
            show_diff_view: false,
            configured_repo: None,
            last_error: None,
            action_palette_open: false,
            action_palette_selected: 0,
            daemon_actions: DAEMON_ACTIONS.to_vec(),
            last_action_result: None,
            action_in_progress: false,
            help_overlay_open: false,
            scroll_offsets: vec![0; 15],
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
            orchestrator_title: String::new(),
            orchestrator_details: String::new(),
            orchestrator_field: OrchestratorField::Title,
            orchestrator_agents: Vec::new(),
            orchestrator_agent_index: 0,
            orchestrator_busy: false,
            orchestrator_status: None,
            orchestrator_last_run: None,
            orchestrator_draft: None,
            review_selection_index: 0,
            review_detail: None,
            review_detail_task_id: None,
            kanban_col: 0,
            kanban_row: 0,
            theme_manager,
            adapter,
            last_pr_poll: None,
            last_run_poll: None,
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
        if let Ok(v) = self.adapter.fetch_acp_agents().await {
            self.acp = v;
            self.sync_orchestrator_agents();
        }
        self.poll_orchestrator_run().await;
        self.refresh_review_detail().await;
        if let Ok(v) = self.adapter.fetch_hooks_list().await {
            self.hooks = v;
        }
        if let Ok(v) = self.adapter.fetch_knowledge_stats().await {
            self.knowledge = v;
        } else if self.knowledge.is_null() {
            self.knowledge = Value::Object(Default::default());
        }
        // The Pull Requests tab is populated by `poll_github()` (throttled)
        // when a repo is configured. Only fall back to the generic issues list
        // when no repo is configured, so the 1s refresh cannot clobber the real
        // PR data with the issues-listing call.
        if self.configured_repo.is_none() {
            if let Ok(v) = self.adapter.fetch_pulls_list().await {
                self.pulls = v;
            } else if self.pulls.is_null() {
                self.pulls = Value::Object(Default::default());
            }
        }
        // Fetch real PR data from the configured repo, throttled so the 1s
        // refresh loop does not hammer the GitHub API on every tick.
        let due = self
            .last_pr_poll
            .map(|t| t.elapsed() >= PR_POLL_INTERVAL)
            .unwrap_or(true);
        if self.configured_repo.is_some() && due {
            self.poll_github().await;
            self.last_pr_poll = Some(Instant::now());
        }
        self.last_error = None;
        self.last_refresh = Instant::now();
        self.drain_embedded_log();
        Ok(())
    }

    /// Fetch PR list, CI status, and (optionally) the selected diff from the
    /// configured GitHub repo.
    async fn poll_github(&mut self) {
        let Some(ref repo) = self.configured_repo else {
            return;
        };
        if let Ok(v) = self.adapter.fetch_pull_requests(repo, "open").await {
            self.pulls = v;
        }
        if let Some((r, pr_number)) = self.selected_repo_and_number() {
            if let Ok(v) = self.adapter.fetch_pr_ci_status(&r, pr_number).await {
                self.pr_ci_status = v;
            }
            if self.show_diff_view {
                if let Ok(diff) = self.adapter.fetch_pr_diff(&r, pr_number).await {
                    self.pr_diff = Some(diff);
                }
            }
        }
    }

    /// Force an immediate GitHub poll (used on PR selection change and diff
    /// toggle) instead of waiting for the next throttled tick.
    pub async fn force_pr_poll(&mut self) {
        if self.configured_repo.is_some() {
            self.poll_github().await;
            self.last_pr_poll = Some(Instant::now());
        }
    }

    /// Extract `(repo, pr_number)` for the currently selected pull request.
    fn selected_repo_and_number(&self) -> Option<(String, u64)> {
        self.selected_pull().and_then(|pr| {
            let repo = pr.pointer("/metadata/repo").and_then(Value::as_str)?;
            let pr_number = pr
                .pointer("/id")
                .and_then(Value::as_str)
                .and_then(|s| s.split('#').next_back())
                .and_then(|s| s.parse::<u64>().ok())?;
            if repo.is_empty() || pr_number == 0 {
                None
            } else {
                Some((repo.to_string(), pr_number))
            }
        })
    }

    /// Set the git repository targeted by the PR/CI/diff view.
    pub fn set_configured_repo(&mut self, repo: String) {
        self.configured_repo = Some(repo);
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

    pub fn toggle_diff_view(&mut self) {
        self.show_diff_view = !self.show_diff_view;
        self.pr_diff_scroll = 0;
    }

    pub fn diff_scroll_down(&mut self) {
        if let Some(ref diff) = self.pr_diff {
            let lines = diff.lines().count();
            if self.pr_diff_scroll + 1 < lines {
                self.pr_diff_scroll += 1;
            }
        }
    }

    pub fn diff_scroll_up(&mut self) {
        if self.pr_diff_scroll > 0 {
            self.pr_diff_scroll -= 1;
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

    /// Dispatch a task into the embedded pane.
    ///
    /// This is the single owner of the embedded-pane path: it spawns
    /// `loom workflow dispatch <task> --agent <agent>` for the task's
    /// recorded agent and installs the child's output stream into the
    /// pane. There is no separate embedded-pane dispatcher.
    pub fn dispatch_embedded_pane(&mut self) -> CommandResult {
        let Some(task) = self.selected_task().cloned() else {
            return CommandResult {
                action: "Embedded Pane".to_string(),
                success: false,
                message: "No task selected".to_string(),
            };
        };
        let task_id = task
            .pointer("/task_id")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        let Some(agent) = task
            .pointer("/last_agent")
            .and_then(Value::as_str)
            .map(String::from)
        else {
            return CommandResult {
                action: "Embedded Pane".to_string(),
                success: false,
                message: format!("Task {task_id} has no recorded agent to dispatch"),
            };
        };

        let (child, log_rx, stdin) = self.adapter.stream_background(&agent, &task_id);
        let success = child.is_some();
        self.embedded_task = Some(task);
        self.embedded_log_rx = Some(log_rx);
        self.embedded_child = child;
        self.embedded_stdin = stdin;
        CommandResult {
            action: "Embedded Pane".to_string(),
            success,
            message: if success {
                format!("Dispatching task {task_id} to {agent}")
            } else {
                format!("Failed to spawn {agent} for task {task_id}")
            },
        }
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

    /// Dynamic index of the Orchestrator tab (never hardcoded, so future
    /// tabs cannot silently shift it).
    pub fn orchestrator_tab_index(&self) -> Option<usize> {
        self.tabs.iter().position(|&t| t == "Orchestrator")
    }

    pub fn is_orchestrator_tab(&self) -> bool {
        self.orchestrator_tab_index() == Some(self.tab_index)
    }

    /// Dynamic index of the Review tab.
    pub fn review_tab_index(&self) -> Option<usize> {
        self.tabs.iter().position(|&t| t == "Review")
    }

    pub fn is_review_tab(&self) -> bool {
        self.review_tab_index() == Some(self.tab_index)
    }

    /// Task id under the Review cursor, if any.
    pub fn review_selected_task_id(&self) -> Option<String> {
        review_task_ids(&self.workflow)
            .get(self.review_selection_index)
            .cloned()
    }

    pub fn review_select_next(&mut self) {
        let count = review_task_ids(&self.workflow).len();
        if count > 0 {
            self.review_selection_index = (self.review_selection_index + 1) % count;
        }
    }

    pub fn review_select_prev(&mut self) {
        let count = review_task_ids(&self.workflow).len();
        if count > 0 {
            if self.review_selection_index == 0 {
                self.review_selection_index = count - 1;
            } else {
                self.review_selection_index -= 1;
            }
        }
    }

    /// Keep the cursor inside the task list after refresh shrinks it.
    pub fn review_clamp_selection(&mut self) {
        let count = review_task_ids(&self.workflow).len();
        if count == 0 {
            self.review_selection_index = 0;
            self.review_detail = None;
            self.review_detail_task_id = None;
        } else if self.review_selection_index >= count {
            self.review_selection_index = count - 1;
        }
    }

    /// Dynamic index of the Kanban tab.
    pub fn kanban_tab_index(&self) -> Option<usize> {
        self.tabs.iter().position(|&t| t == "Kanban")
    }

    pub fn is_kanban_tab(&self) -> bool {
        self.kanban_tab_index() == Some(self.tab_index)
    }

    /// Task ids in the selected Kanban column, in workflow-list order.
    pub fn kanban_column_task_ids(&self) -> Vec<String> {
        let phase = PHASES.get(self.kanban_col).copied().unwrap_or("");
        self.workflow
            .pointer("/tasks")
            .and_then(Value::as_array)
            .map(|tasks| {
                tasks
                    .iter()
                    .filter(|t| t.get("phase").and_then(Value::as_str) == Some(phase))
                    .filter_map(|t| t.get("task_id"))
                    .filter_map(Value::as_str)
                    .map(str::to_string)
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Task id under the Kanban cursor, if the column has any task.
    /// Self-clamps, so a refresh that shrinks the board can never
    /// leave the cursor dangling past the end.
    pub fn kanban_selected_task_id(&self) -> Option<String> {
        let ids = self.kanban_column_task_ids();
        if ids.is_empty() {
            None
        } else {
            ids.get(self.kanban_row.min(ids.len() - 1)).cloned()
        }
    }

    fn kanban_clamp_row(&mut self) {
        let count = self.kanban_column_task_ids().len();
        self.kanban_row = if count == 0 {
            0
        } else {
            self.kanban_row.min(count - 1)
        };
    }

    pub fn kanban_select_next_col(&mut self) {
        self.kanban_col = (self.kanban_col + 1) % PHASES.len();
        self.kanban_clamp_row();
    }

    pub fn kanban_select_prev_col(&mut self) {
        if self.kanban_col == 0 {
            self.kanban_col = PHASES.len() - 1;
        } else {
            self.kanban_col -= 1;
        }
        self.kanban_clamp_row();
    }

    pub fn kanban_select_next_row(&mut self) {
        self.kanban_row += 1;
        self.kanban_clamp_row();
    }

    pub fn kanban_select_prev_row(&mut self) {
        self.kanban_row = self.kanban_row.saturating_sub(1);
    }

    /// Point the flat task cursor (used by the dispatch menu) at the
    /// given task id. Returns false if the id is not on the board.
    pub fn select_task_by_id(&mut self, id: &str) -> bool {
        let found = self
            .workflow
            .pointer("/tasks")
            .and_then(Value::as_array)
            .and_then(|tasks| {
                tasks
                    .iter()
                    .position(|t| t.get("task_id").and_then(Value::as_str) == Some(id))
            });
        match found {
            Some(i) => {
                self.task_selection_index = i;
                true
            }
            None => false,
        }
    }

    /// Advance the selected Kanban task to its next phase
    /// (`workflow advance`), the board's "move card right" gesture.
    pub async fn advance_kanban_task(&mut self) {
        let Some(task_id) = self.kanban_selected_task_id() else {
            self.last_action_result = Some(CommandResult {
                action: "kanban advance".to_string(),
                success: false,
                message: "No task selected.".to_string(),
            });
            return;
        };
        match self.adapter.advance_task_phase(&task_id).await {
            Ok(()) => {
                self.last_action_result = Some(CommandResult {
                    action: "kanban advance".to_string(),
                    success: true,
                    message: format!("Advanced task {task_id} to the next phase."),
                });
            }
            Err(e) => {
                self.last_action_result = Some(CommandResult {
                    action: "kanban advance".to_string(),
                    success: false,
                    message: format!("Could not advance task {task_id}: {e}"),
                });
            }
        }
        self.refresh_safe().await;
    }

    /// Remove the selected Kanban task and its per-phase sidecars,
    /// mirroring the Review tab's `x` action.
    pub async fn remove_kanban_task(&mut self) {
        let Some(task_id) = self.kanban_selected_task_id() else {
            self.last_action_result = Some(CommandResult {
                action: "kanban remove".to_string(),
                success: false,
                message: "No task selected.".to_string(),
            });
            return;
        };
        match self.adapter.remove_task(&task_id).await {
            Ok(()) => {
                self.last_action_result = Some(CommandResult {
                    action: "kanban remove".to_string(),
                    success: true,
                    message: format!("Removed task {task_id}."),
                });
            }
            Err(e) => {
                self.last_action_result = Some(CommandResult {
                    action: "kanban remove".to_string(),
                    success: false,
                    message: format!("Could not remove task {task_id}: {e}"),
                });
            }
        }
        self.refresh_safe().await;
    }

    /// Fetch full detail for the selected Review task when the cached
    /// detail belongs to another task (or is missing). Runs inside the
    /// 1s refresh so selection changes resolve on the next tick without
    /// spawning a subprocess per keypress. Failures keep the previous
    /// detail; the next tick retries, matching every other refresh fetch.
    async fn refresh_review_detail(&mut self) {
        if !self.is_review_tab() {
            return;
        }
        self.review_clamp_selection();
        let selected = self.review_selected_task_id();
        if self.review_detail_task_id == selected {
            return;
        }
        let Some(task_id) = selected else {
            self.review_detail = None;
            return;
        };
        if let Ok(detail) = self.adapter.fetch_workflow_show(&task_id).await {
            self.review_detail = Some(detail);
            self.review_detail_task_id = Some(task_id);
        }
    }

    /// Cycle the selected Review task's assigned agent through the
    /// known ACP agents and persist it (`workflow assign --agent`).
    /// The cached detail is invalidated so the next refresh refetches.
    pub async fn cycle_review_task_agent(&mut self) {
        let Some(task_id) = self.review_selected_task_id() else {
            self.last_action_result = Some(CommandResult {
                action: "review reassign".to_string(),
                success: false,
                message: "No task selected.".to_string(),
            });
            return;
        };
        let current = self
            .review_detail
            .as_ref()
            .and_then(|d| d.pointer("/task/last_agent"))
            .and_then(Value::as_str);
        let Some(next) = next_agent_after(current, &self.orchestrator_agents) else {
            self.last_action_result = Some(CommandResult {
                action: "review reassign".to_string(),
                success: false,
                message: "No ACP agents available to assign.".to_string(),
            });
            return;
        };
        match self.adapter.assign_task_agent(&task_id, &next).await {
            Ok(()) => {
                self.review_detail = None;
                self.review_detail_task_id = None;
                self.last_action_result = Some(CommandResult {
                    action: "review reassign".to_string(),
                    success: true,
                    message: format!("Assigned task {task_id} to {next}."),
                });
            }
            Err(e) => {
                self.last_action_result = Some(CommandResult {
                    action: "review reassign".to_string(),
                    success: false,
                    message: format!("Could not assign task {task_id}: {e}"),
                });
            }
        }
        self.refresh_safe().await;
    }

    /// Preview a deterministic draft plan for the current inputs.
    /// Nothing is created; the draft is stored for an explicit create.
    pub async fn preview_orchestrator_draft(&mut self) {
        let title = self.orchestrator_title_value();
        let details = self.orchestrator_details_value();
        if title.is_empty() || details.is_empty() {
            self.orchestrator_status = Some(CommandResult {
                action: "orchestrator draft".to_string(),
                success: false,
                message: "Title and details are both required to draft a plan.".to_string(),
            });
            return;
        }
        match self.adapter.draft_plan(&title, &details).await {
            Ok(draft) => {
                let titles = draft_task_titles(&draft);
                let preview: Vec<String> = titles.iter().take(3).cloned().collect();
                let mut message =
                    format!("Draft: {} task(s) ({})", titles.len(), preview.join(", "));
                if titles.len() > preview.len() {
                    message.push_str(", …");
                }
                message.push_str(" — run `orchestrator draft create` to import.");
                self.orchestrator_draft = Some(draft);
                self.orchestrator_status = Some(CommandResult {
                    action: "orchestrator draft".to_string(),
                    success: true,
                    message,
                });
            }
            Err(e) => {
                self.orchestrator_status = Some(CommandResult {
                    action: "orchestrator draft".to_string(),
                    success: false,
                    message: format!("Could not draft plan: {e}"),
                });
            }
        }
    }

    /// Import the drafted plan as workflow tasks (explicit confirm step;
    /// never runs implicitly). Clears inputs and the stored draft.
    pub async fn create_orchestrator_draft(&mut self) {
        let title = self.orchestrator_title_value();
        let details = self.orchestrator_details_value();
        if title.is_empty() || details.is_empty() {
            self.orchestrator_status = Some(CommandResult {
                action: "orchestrator draft create".to_string(),
                success: false,
                message: "Title and details are both required to draft a plan.".to_string(),
            });
            return;
        }
        match self.adapter.draft_plan_create(&title, &details).await {
            Ok(ids) => {
                self.orchestrator_title.clear();
                self.orchestrator_details.clear();
                self.orchestrator_draft = None;
                self.orchestrator_status = Some(CommandResult {
                    action: "orchestrator draft create".to_string(),
                    success: true,
                    message: format!("Created {} task(s) from draft — see Review.", ids.len()),
                });
            }
            Err(e) => {
                self.orchestrator_status = Some(CommandResult {
                    action: "orchestrator draft create".to_string(),
                    success: false,
                    message: format!("Could not create tasks from draft: {e}"),
                });
            }
        }
        self.refresh_safe().await;
    }

    /// Create a pull request from the current branch for the
    /// configured repository.
    pub async fn create_pull_request(&mut self) {
        let Some(repo) = self.configured_repo.clone() else {
            self.last_action_result = Some(CommandResult {
                action: "pr create".to_string(),
                success: false,
                message: "No repository configured for PR creation.".to_string(),
            });
            return;
        };
        let result = match self.adapter.create_pull_request(&repo).await {
            Ok(url) => CommandResult {
                action: "pr create".to_string(),
                success: true,
                message: format!("Created PR: {url}"),
            },
            Err(e) => CommandResult {
                action: "pr create".to_string(),
                success: false,
                message: format!("Could not create PR: {e}"),
            },
        };
        self.last_action_result = Some(result);
        self.force_pr_poll().await;
        self.refresh_safe().await;
    }

    /// Remove the selected Review task (undo for drafts and mistakes).
    pub async fn remove_review_task(&mut self) {
        let Some(task_id) = self.review_selected_task_id() else {
            self.last_action_result = Some(CommandResult {
                action: "review remove".to_string(),
                success: false,
                message: "No task selected.".to_string(),
            });
            return;
        };
        match self.adapter.remove_task(&task_id).await {
            Ok(()) => {
                self.review_detail = None;
                self.review_detail_task_id = None;
                self.last_action_result = Some(CommandResult {
                    action: "review remove".to_string(),
                    success: true,
                    message: format!("Removed task {task_id}."),
                });
            }
            Err(e) => {
                self.last_action_result = Some(CommandResult {
                    action: "review remove".to_string(),
                    success: false,
                    message: format!("Could not remove task {task_id}: {e}"),
                });
            }
        }
        self.refresh_safe().await;
    }

    /// Dispatch the selected Review task to its recorded agent (or the
    /// Orchestrator-selected fallback) via tracked ACP dispatch.
    /// Send a task to an ACP agent over daemon IPC, with optional
    /// prior sessions for continuation. Shared by the dispatch actions.
    fn run_acp_dispatch(
        &self,
        task_id: &str,
        agent: &str,
        prior_sessions: &[String],
    ) -> crate::dispatch::DispatchResult {
        let dispatcher = crate::dispatch::ACPDispatch::new(
            self.status
                .pointer("/daemon/reachable")
                .and_then(Value::as_bool)
                .unwrap_or(false),
            self.daemon
                .pointer("/socket_path")
                .and_then(Value::as_str)
                .map(String::from),
        );
        dispatcher.dispatch(&serde_json::json!({
            "task_id": task_id,
            "agent_id": agent,
            "prior_sessions": prior_sessions,
        }))
    }

    /// Shared tail for task dispatch actions: store the tracked run and
    /// build the status result. `resumed` is the number of prior
    /// sessions sent for continuation (0 for a fresh dispatch).
    fn record_task_dispatch(
        &mut self,
        action: &str,
        task_id: &str,
        agent: &str,
        result: crate::dispatch::DispatchResult,
        resumed: usize,
    ) -> CommandResult {
        if !result.success {
            return CommandResult {
                action: action.to_string(),
                success: false,
                message: result.message,
            };
        }
        let mut message = format!("Dispatched task {task_id} to {agent}");
        if resumed > 0 {
            message.push_str(&format!(" with {resumed} prior session(s)"));
        }
        match result.run_id {
            Some(run_id) => {
                message.push_str(&format!(" (run {run_id})."));
                self.orchestrator_last_run = Some(OrchestratorRun {
                    run_id,
                    task_id: task_id.to_string(),
                    agent: agent.to_string(),
                    status: Value::Null,
                });
                self.last_run_poll = None;
            }
            None => message.push('.'),
        }
        CommandResult {
            action: action.to_string(),
            success: true,
            message,
        }
    }

    /// Resolve the selected Review task plus its dispatch agent: the
    /// task's recorded `last_agent`, else the Orchestrator-selected one.
    fn review_dispatch_target(&self) -> Result<(String, String), String> {
        let Some(task_id) = self.review_selected_task_id() else {
            return Err("No task selected.".to_string());
        };
        let detail = self.review_detail.clone().unwrap_or(Value::Null);
        let Some(agent) = review_dispatch_agent(&detail, self.orchestrator_selected_agent()) else {
            return Err(format!(
                "Task {task_id} has no recorded agent and none is selected."
            ));
        };
        Ok((task_id, agent))
    }

    pub async fn dispatch_review_task(&mut self) {
        let outcome = match self.review_dispatch_target() {
            Ok((task_id, agent)) => {
                let result = self.run_acp_dispatch(&task_id, &agent, &[]);
                self.record_task_dispatch("review dispatch", &task_id, &agent, result, 0)
            }
            Err(message) => CommandResult {
                action: "review dispatch".to_string(),
                success: false,
                message,
            },
        };
        self.last_action_result = Some(outcome);
        self.refresh_safe().await;
    }

    /// Resume the selected Review task on its agent, sending the task's
    /// linked sessions as prior context so the agent continues with
    /// full history instead of starting blank.
    pub async fn resume_review_task(&mut self) {
        let outcome = match self.review_dispatch_target() {
            Ok((task_id, agent)) => {
                let priors = task_session_ids(&self.sessions, &task_id);
                if priors.is_empty() {
                    CommandResult {
                        action: "review resume".to_string(),
                        success: false,
                        message: format!(
                            "Task {task_id} has no captured sessions yet — dispatch it first."
                        ),
                    }
                } else {
                    let result = self.run_acp_dispatch(&task_id, &agent, &priors);
                    let resumed = priors.len();
                    self.record_task_dispatch("review resume", &task_id, &agent, result, resumed)
                }
            }
            Err(message) => CommandResult {
                action: "review resume".to_string(),
                success: false,
                message,
            },
        };
        self.last_action_result = Some(outcome);
        self.refresh_safe().await;
    }

    pub fn orchestrator_focus_next(&mut self) {
        self.orchestrator_field = self.orchestrator_field.next();
    }

    pub fn orchestrator_focus_previous(&mut self) {
        self.orchestrator_field = self.orchestrator_field.previous();
    }

    pub fn orchestrator_append(&mut self, c: char) {
        match self.orchestrator_field {
            OrchestratorField::Title => self.orchestrator_title.push(c),
            OrchestratorField::Details => self.orchestrator_details.push(c),
            OrchestratorField::Agent => {}
        }
    }

    pub fn orchestrator_backspace(&mut self) {
        match self.orchestrator_field {
            OrchestratorField::Title => {
                self.orchestrator_title.pop();
            }
            OrchestratorField::Details => {
                self.orchestrator_details.pop();
            }
            OrchestratorField::Agent => {}
        }
    }

    /// Insert terminal-pasted text into the focused Orchestrator
    /// field. Newlines and other control characters become spaces so
    /// a multi-line paste cannot submit the form mid-paste. Pastes on
    /// any other tab are ignored.
    pub fn orchestrator_paste(&mut self, text: &str) {
        if !self.is_orchestrator_tab() {
            return;
        }
        let cleaned: String = text
            .chars()
            .map(|c| if c.is_control() { ' ' } else { c })
            .collect();
        match self.orchestrator_field {
            OrchestratorField::Title => self.orchestrator_title.push_str(&cleaned),
            OrchestratorField::Details => self.orchestrator_details.push_str(&cleaned),
            OrchestratorField::Agent => {}
        }
    }

    pub fn orchestrator_title_value(&self) -> String {
        self.orchestrator_title.trim().to_string()
    }

    pub fn orchestrator_details_value(&self) -> String {
        self.orchestrator_details.trim().to_string()
    }

    pub fn orchestrator_selected_agent(&self) -> Option<&str> {
        self.orchestrator_agents
            .get(self.orchestrator_agent_index)
            .map(String::as_str)
    }

    pub fn orchestrator_can_submit(&self) -> bool {
        !self.orchestrator_busy
            && !self.orchestrator_title_value().is_empty()
            && !self.orchestrator_details_value().is_empty()
            && self.orchestrator_selected_agent().is_some()
    }

    pub fn orchestrator_agent_next(&mut self) {
        if !self.orchestrator_agents.is_empty() {
            self.orchestrator_agent_index =
                (self.orchestrator_agent_index + 1) % self.orchestrator_agents.len();
        }
    }

    pub fn orchestrator_agent_previous(&mut self) {
        if !self.orchestrator_agents.is_empty() {
            if self.orchestrator_agent_index == 0 {
                self.orchestrator_agent_index = self.orchestrator_agents.len() - 1;
            } else {
                self.orchestrator_agent_index -= 1;
            }
        }
    }

    pub fn clear_orchestrator_status(&mut self) {
        self.orchestrator_status = None;
    }

    fn sync_orchestrator_agents(&mut self) {
        self.orchestrator_agents = orchestrator_agent_list(&self.acp);
        if self.orchestrator_agents.is_empty() {
            self.orchestrator_agent_index = 0;
        } else if self.orchestrator_agent_index >= self.orchestrator_agents.len() {
            self.orchestrator_agent_index = self.orchestrator_agents.len() - 1;
        }
    }

    /// Poll the daemon for the last Orchestrator run's status, throttled
    /// to `RUN_POLL_INTERVAL`. Terminal runs and polling errors (e.g.
    /// daemon stopped) leave the last known status untouched.
    async fn poll_orchestrator_run(&mut self) {
        let run_id = match self.orchestrator_last_run.as_ref() {
            Some(run) => {
                let status = run
                    .status
                    .get("status")
                    .and_then(Value::as_str)
                    .unwrap_or("");
                if run_is_terminal(status) {
                    return;
                }
                run.run_id.clone()
            }
            None => return,
        };
        if run_id.is_empty() {
            return;
        }
        let due = self
            .last_run_poll
            .map(|t| t.elapsed() >= RUN_POLL_INTERVAL)
            .unwrap_or(true);
        if !due {
            return;
        }
        self.last_run_poll = Some(Instant::now());
        if let Ok(status) = self.adapter.fetch_run_status(&run_id).await {
            if let Some(run) = self.orchestrator_last_run.as_mut() {
                run.status = status;
            }
        }
    }

    /// Create a workflow task from the Orchestrator inputs and dispatch
    /// it to the selected ACP agent. The daemon tracks the dispatch as a
    /// run and its id is surfaced in the status message for follow-up.
    pub async fn submit_orchestrator(&mut self) {
        if self.orchestrator_busy {
            return;
        }

        let title = self.orchestrator_title_value();
        let details = self.orchestrator_details_value();
        let Some(agent) = self.orchestrator_selected_agent().map(str::to_string) else {
            self.orchestrator_status = Some(CommandResult {
                action: "orchestrator submit".to_string(),
                success: false,
                message: "Select a connected ACP tool before submitting.".to_string(),
            });
            return;
        };

        if title.is_empty() || details.is_empty() {
            self.orchestrator_status = Some(CommandResult {
                action: "orchestrator submit".to_string(),
                success: false,
                message: "Title and details are both required.".to_string(),
            });
            return;
        }

        self.orchestrator_busy = true;
        self.orchestrator_status = Some(CommandResult {
            action: "orchestrator submit".to_string(),
            success: true,
            message: format!("Creating task and dispatching to {agent}..."),
        });

        let result = match self.adapter.create_workflow_task(&title, &details).await {
            Ok(task_id) => {
                let dispatch = self.run_acp_dispatch(&task_id, &agent, &[]);
                if dispatch.success {
                    self.orchestrator_title.clear();
                    self.orchestrator_details.clear();
                    let mut message = format!("Created task {task_id} and dispatched to {agent}.");
                    if let Some(run_id) = dispatch.run_id {
                        message.push_str(&format!(" (run {run_id})"));
                        self.orchestrator_last_run = Some(OrchestratorRun {
                            run_id,
                            task_id: task_id.clone(),
                            agent: agent.clone(),
                            status: Value::Null,
                        });
                        self.last_run_poll = None;
                    }
                    CommandResult {
                        action: "orchestrator submit".to_string(),
                        success: true,
                        message,
                    }
                } else {
                    CommandResult {
                        action: "orchestrator submit".to_string(),
                        success: false,
                        message: format!(
                            "Created task {task_id}, but dispatch failed: {}",
                            dispatch.message
                        ),
                    }
                }
            }
            Err(e) => CommandResult {
                action: "orchestrator submit".to_string(),
                success: false,
                message: format!("Could not create workflow task: {e}"),
            },
        };

        self.orchestrator_busy = false;
        self.orchestrator_status = Some(result);
        self.refresh_safe().await;
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
            "tab 8" => self.set_tab_from_digit(7),
            "tab 9" => self.set_tab_from_digit(8),
            "tab 10" => self.set_tab_from_digit(9),
            "tab 11" => self.set_tab_from_digit(10),
            "orchestrator submit" => self.submit_orchestrator().await,
            "orchestrator draft" => self.preview_orchestrator_draft().await,
            "orchestrator draft create" => self.create_orchestrator_draft().await,
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapter::LoomCliAdapter;
    use serde_json::json;

    fn app() -> App {
        App::new(LoomCliAdapter::new("loom", crate::loom_dir()))
    }

    #[test]
    fn exposes_orchestrator_as_the_last_tab() {
        let app = app();
        let len = app.tabs.len();
        assert_eq!(app.tabs[len - 2], "Orchestrator");
        eprintln!(
            "len={}, orchestrator_index={}",
            len,
            app.orchestrator_tab_index().unwrap_or(999)
        );
        let idx = app.orchestrator_tab_index();
        eprintln!("idx={:?}", idx);
        eprintln!("idx={:?}, len-2={:?}", idx, Some(len - 2));
        assert_eq!(idx, Some(len - 2));
        assert_eq!(app.scroll_offsets.len(), app.tabs.len());
    }

    #[test]
    fn orchestrator_field_navigation_wraps() {
        let mut app = app();
        assert_eq!(app.orchestrator_field, OrchestratorField::Title);
        app.orchestrator_focus_next();
        assert_eq!(app.orchestrator_field, OrchestratorField::Details);
        app.orchestrator_focus_next();
        assert_eq!(app.orchestrator_field, OrchestratorField::Agent);
        app.orchestrator_focus_next();
        assert_eq!(app.orchestrator_field, OrchestratorField::Title);
        app.orchestrator_focus_previous();
        assert_eq!(app.orchestrator_field, OrchestratorField::Agent);
    }

    #[test]
    fn types_and_deletes_only_in_the_focused_orchestrator_field() {
        let mut app = app();
        app.orchestrator_append('A');
        app.orchestrator_append('B');
        app.orchestrator_focus_next();
        app.orchestrator_append('T');
        app.orchestrator_backspace();
        assert_eq!(app.orchestrator_title, "AB");
        assert_eq!(app.orchestrator_details, "");
    }

    #[test]
    fn merges_registered_handlers_with_agents_toml_entries() {
        let value = json!({
            "running": true,
            "handlers": ["codex", "  ", "claude-code", 7],
            "agents_toml": ["goose", "codex"],
            "has_default_handler": true,
        });
        assert_eq!(
            orchestrator_agent_list(&value),
            vec![
                "codex".to_string(),
                "claude-code".to_string(),
                "goose".to_string()
            ]
        );
    }

    #[test]
    fn empty_agent_sources_yield_no_agents() {
        let value = json!({
            "running": true,
            "handlers": [],
            "agents_toml": [],
            "has_default_handler": true,
        });
        assert!(orchestrator_agent_list(&value).is_empty());
    }

    #[test]
    fn rejects_incomplete_orchestrator_submissions() {
        let mut app = app();
        assert!(!app.orchestrator_can_submit());

        app.orchestrator_title = "Add dark mode".to_string();
        assert!(!app.orchestrator_can_submit());

        app.orchestrator_details = "Use theme tokens.".to_string();
        assert!(!app.orchestrator_can_submit());

        app.orchestrator_agents = vec!["codex".to_string()];
        assert!(app.orchestrator_can_submit());
    }

    #[test]
    fn run_terminal_states_stop_polling() {
        assert!(run_is_terminal("succeeded"));
        assert!(run_is_terminal("failed"));
        assert!(!run_is_terminal("dispatched"));
        assert!(!run_is_terminal("unknown"));
        assert!(!run_is_terminal(""));
    }

    #[test]
    fn formats_completed_run_line() {
        let run = OrchestratorRun {
            run_id: "abcdef1234567890".to_string(),
            task_id: "task-1".to_string(),
            agent: "codex".to_string(),
            status: json!({
                "status": "succeeded",
                "exit_code": 0,
                "session_id": "task-1.abcdef12",
            }),
        };
        let line = format_run_line(&run);
        assert!(line.starts_with("run abcdef12"), "got: {line}");
        assert!(line.contains("succeeded"), "got: {line}");
        assert!(line.contains("exit 0"), "got: {line}");
        assert!(line.contains("session"), "got: {line}");
    }

    #[test]
    fn formats_pending_run_line_without_session() {
        let run = OrchestratorRun {
            run_id: "abcdef1234567890".to_string(),
            task_id: "task-1".to_string(),
            agent: "codex".to_string(),
            status: Value::Null,
        };
        let line = format_run_line(&run);
        assert!(line.contains("dispatched"), "got: {line}");
        assert!(line.contains("no session yet"), "got: {line}");
        assert!(!line.contains("exit"), "got: {line}");
    }

    fn workflow_value() -> Value {
        json!({
            "tasks": [
                {"task_id": "t1", "title": "First", "phase": "Plan", "status": "Pending"},
                {"task_id": "t2", "title": "Second", "phase": "Implement", "status": "InProgress"},
            ]
        })
    }

    #[test]
    fn exposes_review_as_the_last_tab() {
        let app = app();
        assert_eq!(app.tabs.last(), Some(&"Review"));
        assert_eq!(app.review_tab_index(), Some(app.tabs.len() - 1));
        assert_eq!(app.scroll_offsets.len(), app.tabs.len());
    }

    #[test]
    fn extracts_review_task_ids_in_order() {
        assert_eq!(
            review_task_ids(&workflow_value()),
            vec!["t1".to_string(), "t2".to_string()]
        );
        assert!(review_task_ids(&Value::Null).is_empty());
    }

    #[test]
    fn review_selection_wraps_and_clamps() {
        let mut app = app();
        app.workflow = workflow_value();
        assert_eq!(app.review_selected_task_id().as_deref(), Some("t1"));
        app.review_select_next();
        assert_eq!(app.review_selected_task_id().as_deref(), Some("t2"));
        app.review_select_next();
        assert_eq!(app.review_selected_task_id().as_deref(), Some("t1"));
        app.review_select_prev();
        assert_eq!(app.review_selected_task_id().as_deref(), Some("t2"));

        app.workflow = Value::Null;
        app.review_clamp_selection();
        assert_eq!(app.review_selected_task_id(), None);
    }

    #[test]
    fn links_sessions_by_task_prefix() {
        let sessions = json!({
            "sessions": [
                {"session_id": "t1.Plan"},
                {"session_id": "t1.Implement"},
                {"session_id": "other.Task"},
                {"session_id": "t1"},
            ]
        });
        assert_eq!(
            task_session_ids(&sessions, "t1"),
            vec!["t1.Plan".to_string(), "t1.Implement".to_string()]
        );
        assert!(task_session_ids(&sessions, "missing").is_empty());
    }

    #[test]
    fn review_dispatch_agent_prefers_last_agent_then_fallback() {
        let detail = json!({"task": {"task_id": "t1", "last_agent": "goose"}});
        assert_eq!(
            review_dispatch_agent(&detail, Some("codex")).as_deref(),
            Some("goose")
        );
        let no_agent = json!({"task": {"task_id": "t1"}});
        assert_eq!(
            review_dispatch_agent(&no_agent, Some("codex")).as_deref(),
            Some("codex")
        );
        assert_eq!(review_dispatch_agent(&no_agent, None), None);
        assert_eq!(
            review_dispatch_agent(&Value::Null, Some("codex")).as_deref(),
            Some("codex")
        );
    }

    #[test]
    fn formats_review_task_line() {
        let task = json!({"task_id": "abc123", "title": "Fix it", "phase": "Verify", "status": "InProgress"});
        let line = format_review_task_line(&task);
        assert!(line.contains("Verify"), "got: {line}");
        assert!(line.contains("InProgress"), "got: {line}");
        assert!(line.contains("Fix it"), "got: {line}");
    }

    #[test]
    fn draft_task_titles_extract_in_order() {
        let draft = json!({
            "tasks": [
                {"id": "t1", "title": "Alpha"},
                {"id": "t2", "title": "Beta"},
                {"id": "t3"},
            ]
        });
        assert_eq!(
            draft_task_titles(&draft),
            vec!["Alpha".to_string(), "Beta".to_string()]
        );
        assert!(draft_task_titles(&Value::Null).is_empty());
    }

    #[test]
    fn agent_cycle_starts_at_first_and_wraps() {
        let agents = vec!["a".to_string(), "b".to_string(), "c".to_string()];
        assert_eq!(next_agent_after(None, &agents).as_deref(), Some("a"));
        assert_eq!(next_agent_after(Some("a"), &agents).as_deref(), Some("b"));
        assert_eq!(next_agent_after(Some("c"), &agents).as_deref(), Some("a"));
    }

    #[test]
    fn agent_cycle_handles_unknown_and_empty() {
        let agents = vec!["a".to_string(), "b".to_string()];
        assert_eq!(
            next_agent_after(Some("ghost"), &agents).as_deref(),
            Some("a")
        );
        let empty: Vec<String> = vec![];
        assert_eq!(next_agent_after(None, &empty), None);
        assert_eq!(next_agent_after(Some("a"), &empty), None);
    }

    #[tokio::test]
    async fn create_pull_request_requires_a_configured_repo() {
        let mut app = app();
        assert!(app.configured_repo.is_none());
        app.create_pull_request().await;
        let result = app.last_action_result.expect("status must be set");
        assert!(!result.success);
        assert!(result.message.contains("repository"));
    }

    #[test]
    fn kanban_selection_moves_and_clamps() {
        let mut app = app();
        app.workflow = json!({"tasks": [
            {"task_id": "t1", "phase": "Plan"},
            {"task_id": "t2", "phase": "Plan"},
            {"task_id": "t3", "phase": "Implement"},
        ]});

        assert_eq!(app.kanban_selected_task_id().as_deref(), Some("t1"));
        app.kanban_select_next_row();
        assert_eq!(app.kanban_selected_task_id().as_deref(), Some("t2"));
        app.kanban_select_next_row(); // clamps at the last row
        assert_eq!(app.kanban_selected_task_id().as_deref(), Some("t2"));
        app.kanban_select_prev_row();
        assert_eq!(app.kanban_selected_task_id().as_deref(), Some("t1"));

        app.kanban_select_next_col(); // row clamps into the new column
        assert_eq!(app.kanban_selected_task_id().as_deref(), Some("t3"));
        app.kanban_select_next_col(); // Verify: empty
        app.kanban_select_next_col(); // Finish: empty
        app.kanban_select_next_col(); // wraps back to Plan
        assert_eq!(app.kanban_selected_task_id().as_deref(), Some("t1"));
    }

    #[test]
    fn kanban_selection_wraps_columns_backwards_and_skips_empty() {
        let mut app = app();
        app.workflow = json!({"tasks": [{"task_id": "t9", "phase": "Finish"}]});
        assert_eq!(app.kanban_selected_task_id(), None); // Plan is empty
        app.kanban_select_prev_col(); // wraps to Finish
        assert_eq!(app.kanban_selected_task_id().as_deref(), Some("t9"));
    }

    #[test]
    fn select_task_by_id_points_flat_cursor_at_the_task() {
        let mut app = app();
        app.workflow = json!({"tasks": [
            {"task_id": "t1", "phase": "Plan"},
            {"task_id": "t2", "phase": "Plan"},
            {"task_id": "t3", "phase": "Implement"},
        ]});

        assert!(app.select_task_by_id("t3"));
        assert_eq!(app.task_selection_index, 2);
        assert_eq!(app.selected_task().unwrap().get("task_id").unwrap(), "t3");
        assert!(!app.select_task_by_id("missing"));

        // The dispatch menu acts on the task the Kanban cursor picked.
        app.open_dispatch_menu();
        assert!(app.dispatch_menu_open);
    }

    #[tokio::test]
    async fn kanban_actions_without_selection_report_failure() {
        let mut app = app();
        app.workflow = json!({"tasks": []});

        app.advance_kanban_task().await;
        let result = app
            .last_action_result
            .as_ref()
            .expect("advance must set a status");
        assert!(!result.success);
        assert!(result.message.contains("No task"));

        app.remove_kanban_task().await;
        let result = app
            .last_action_result
            .as_ref()
            .expect("remove must set a status");
        assert!(!result.success);
        assert!(result.message.contains("No task"));
    }

    #[tokio::test]
    async fn embedded_pane_dispatch_attempts_the_workflow_dispatch() {
        // `/bin/echo` stands in for the loom binary: the assertion is that
        // the dispatch path actually spawns `workflow dispatch <task> --agent
        // <agent>`, not that it formats a success string.
        let mut app = App::new(LoomCliAdapter::new("/bin/echo", crate::loom_dir()));
        app.workflow = json!({
            "tasks": [{ "task_id": "task-42", "last_agent": "claude-code" }]
        });

        let result = app.dispatch_embedded_pane();
        assert!(result.success, "dispatch should spawn: {}", result.message);

        let mut rx = app.embedded_log_rx.take().expect("log receiver installed");
        let line = tokio::time::timeout(std::time::Duration::from_secs(5), rx.recv())
            .await
            .expect("output line in time")
            .expect("stream open");
        assert!(
            line.contains("workflow dispatch task-42 --agent claude-code"),
            "unexpected dispatch command: {line}"
        );
    }

    #[test]
    fn paste_lands_in_the_focused_orchestrator_field_only() {
        let mut app = app();
        // Not on the Orchestrator tab: pastes are ignored.
        app.orchestrator_paste("hello\nworld");
        assert_eq!(app.orchestrator_title_value(), "");
        // On the form: newlines become spaces, one Event::Paste lands
        // whole — no mid-paste Enter submit.
        let idx = app.orchestrator_tab_index().expect("tab exists");
        app.set_tab_from_digit(idx);
        assert!(app.is_orchestrator_tab());
        app.orchestrator_paste("fix:\n- line one\n- line two");
        assert_eq!(app.orchestrator_title_value(), "fix: - line one - line two");
    }
}
