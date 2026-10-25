pub mod adapter;
pub mod app;
pub mod dispatch;
pub mod theme;
pub mod ui;

use crate::adapter::LoomCliAdapter;
use crate::app::App;
use crate::dispatch::Dispatcher;
use anyhow::Result;
use crossterm::event::{
    self, DisableBracketedPaste, EnableBracketedPaste, Event, KeyCode, KeyEventKind,
};
use crossterm::execute;
use crossterm::terminal::{disable_raw_mode, enable_raw_mode};
use ratatui::backend::CrosstermBackend;
use ratatui::Terminal;
use serde_json::Value;
use std::io;
use std::time::{Duration, Instant};

/// Resolve the default ecosystem directory (~/.loom/).
pub fn loom_dir() -> String {
    loom_core::loom_dir()
}

/// A Kanban-board gesture.
#[derive(Debug, PartialEq, Eq, Clone, Copy)]
enum KanbanKey {
    PrevCol,
    NextCol,
    PrevRow,
    NextRow,
    Open,
    Advance,
    Remove,
}

/// An Orchestrator-form gesture.
#[derive(Debug, PartialEq, Eq, Clone, Copy)]
enum OrchestratorKey {
    ClearStatus,
    Submit,
    FocusPrev,
    FocusNext,
    AgentPrev,
    AgentNext,
    Backspace,
    /// A printable character to append to the focused field.
    Text(char),
}

/// Map a key press to an Orchestrator gesture.
///
/// `Tab`/`Shift-Tab` are deliberately unbound: they are the only way out
/// of the form, which swallows the digit tab shortcuts as text input.
/// Field focus moved to ↑/↓ and tool selection to ←/→.
///
/// `control` is the key's modifier mask; any modified key is ignored so a
/// chord like Ctrl+C never lands in the text field.
fn orchestrator_key(code: KeyCode, control: bool) -> Option<OrchestratorKey> {
    if control {
        return None;
    }
    match code {
        KeyCode::Esc => Some(OrchestratorKey::ClearStatus),
        KeyCode::Enter => Some(OrchestratorKey::Submit),
        KeyCode::Up => Some(OrchestratorKey::FocusPrev),
        KeyCode::Down => Some(OrchestratorKey::FocusNext),
        KeyCode::Left => Some(OrchestratorKey::AgentPrev),
        KeyCode::Right => Some(OrchestratorKey::AgentNext),
        KeyCode::Backspace | KeyCode::Delete => Some(OrchestratorKey::Backspace),
        KeyCode::Char(c) if !c.is_control() => Some(OrchestratorKey::Text(c)),
        _ => None,
    }
}

/// Map a key press to a Kanban gesture.
///
/// The arrow keys deliberately stay unbound on the Kanban tab: `←`/`→` are
/// the global "previous/next tab" shortcut everywhere else (and are
/// documented as such in the help overlay), so consuming them here would
/// trap the user on the board with no way to leave it. Column movement is
/// available on `h`/`l`, which are unambiguous.
fn kanban_key(code: KeyCode) -> Option<KanbanKey> {
    match code {
        KeyCode::Char('h') => Some(KanbanKey::PrevCol),
        KeyCode::Char('l') => Some(KanbanKey::NextCol),
        KeyCode::Char('k') => Some(KanbanKey::PrevRow),
        KeyCode::Char('j') => Some(KanbanKey::NextRow),
        KeyCode::Enter | KeyCode::Char('a') => Some(KanbanKey::Open),
        KeyCode::Char('>') | KeyCode::Char('p') => Some(KanbanKey::Advance),
        KeyCode::Char('x') => Some(KanbanKey::Remove),
        _ => None,
    }
}

pub async fn run() -> Result<()> {
    let mut terminal = setup_terminal()?;
    let loom_dir = loom_dir();
    let adapter = LoomCliAdapter::new("loom", &loom_dir);
    let mut app = App::new(adapter);
    // Detect the current git repo so the PR/CI/diff view targets a real
    // repository instead of remaining dead code.
    if let Ok(Some(repo)) = app.adapter.fetch_current_repo().await {
        app.set_configured_repo(repo);
    }
    app.refresh_safe().await;

    let result = run_loop(&mut terminal, &mut app).await;
    restore_terminal(&mut terminal)?;
    result
}

fn setup_terminal() -> Result<Terminal<CrosstermBackend<io::Stdout>>> {
    enable_raw_mode()?;
    // Bracketed paste keeps a multi-line terminal paste a single
    // Event::Paste, so pasting a description cannot submit the
    // Orchestrator form mid-paste.
    execute!(io::stdout(), EnableBracketedPaste)?;
    let stdout = io::stdout();
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;
    terminal.clear()?;
    Ok(terminal)
}

fn restore_terminal(terminal: &mut Terminal<CrosstermBackend<io::Stdout>>) -> Result<()> {
    let _ = execute!(io::stdout(), DisableBracketedPaste);
    disable_raw_mode()?;
    terminal.clear()?;
    terminal.show_cursor()?;
    Ok(())
}

async fn run_loop(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    app: &mut App,
) -> Result<()> {
    let refresh_interval = Duration::from_secs(1);
    let mut last_tick = Instant::now();

    loop {
        terminal.draw(|f| ui::draw(f, app))?;

        if event::poll(Duration::from_millis(100))? {
            let ev = event::read()?;
            if let Event::Key(key) = ev {
                if key.kind != KeyEventKind::Press {
                    continue;
                }

                if app.help_overlay_open {
                    match key.code {
                        KeyCode::Esc | KeyCode::Char('?') => app.close_help(),
                        _ => {}
                    }
                    continue;
                }

                if app.dispatch_menu_open {
                    match key.code {
                        KeyCode::Esc => app.close_dispatch_menu(),
                        KeyCode::Enter => {
                            // Get default runtime from config
                            let default_runtime = app
                                .adapter
                                .fetch_config()
                                .await
                                .ok()
                                .and_then(|v| {
                                    v.pointer("/workflow/runtime")
                                        .and_then(|s| s.as_str().map(String::from))
                                })
                                .unwrap_or_else(|| "process".to_string());

                            // The embedded pane (menu index 2) has no
                            // Dispatcher: `App::dispatch_embedded_pane` owns
                            // that path, the TUI just shows its result.
                            if app.dispatch_menu_selected == 2 {
                                app.last_action_result = Some(app.dispatch_embedded_pane().await);
                            } else {
                                let dispatchers: Vec<Box<dyn crate::dispatch::Dispatcher>> = vec![
                                    Box::new(crate::dispatch::TerminalLaunch),
                                    Box::new(crate::dispatch::ACPDispatch::new(
                                        app.status
                                            .pointer("/daemon/reachable")
                                            .and_then(Value::as_bool)
                                            .unwrap_or(false),
                                        app.daemon
                                            .pointer("/socket_path")
                                            .and_then(Value::as_str)
                                            .map(String::from),
                                    )),
                                    Box::new(crate::dispatch::RuntimeDispatch::new(
                                        crate::dispatch::default_runtime_registry(),
                                        default_runtime,
                                        app.adapter.loom_dir.clone(),
                                    )),
                                ];
                                // Menu index 3 (runtime) is the third dispatcher.
                                let index = match app.dispatch_menu_selected {
                                    0 => 0,
                                    1 => 1,
                                    3 => 2,
                                    _ => 0,
                                };
                                if let Some(task) = app.selected_task().cloned() {
                                    let result = dispatchers[index].dispatch(&task);
                                    let task_id = task
                                        .pointer("/task_id")
                                        .and_then(Value::as_str)
                                        .unwrap_or("");
                                    let agent_id = task
                                        .pointer("/agent_id")
                                        .and_then(Value::as_str)
                                        .unwrap_or("");
                                    let title = task
                                        .pointer("/title")
                                        .and_then(Value::as_str)
                                        .unwrap_or("");
                                    let phase_name = match index {
                                        0 => "terminal",
                                        1 => "acp",
                                        _ => "runtime",
                                    };
                                    crate::dispatch::record_dispatch_usage(
                                        &app.adapter.loom_dir,
                                        task_id,
                                        phase_name,
                                        agent_id,
                                        title,
                                    );
                                    app.last_action_result = Some(crate::adapter::CommandResult {
                                        action: dispatchers[index].name().to_string(),
                                        success: result.success,
                                        message: result.message,
                                    });
                                }
                            }
                            app.close_dispatch_menu();
                        }
                        KeyCode::Down | KeyCode::Char('j') => app.dispatch_menu_next(),
                        KeyCode::Up | KeyCode::Char('k') => app.dispatch_menu_prev(),
                        KeyCode::Char('t') => {
                            app.dispatch_menu_selected = 0;
                            if let Some(task) = app.selected_task() {
                                let dispatcher = crate::dispatch::TerminalLaunch;
                                let result = dispatcher.dispatch(task);
                                let task_id = task
                                    .pointer("/task_id")
                                    .and_then(Value::as_str)
                                    .unwrap_or("");
                                let agent_id = task
                                    .pointer("/agent_id")
                                    .and_then(Value::as_str)
                                    .unwrap_or("");
                                let title = task
                                    .pointer("/title")
                                    .and_then(Value::as_str)
                                    .unwrap_or("");
                                crate::dispatch::record_dispatch_usage(
                                    &app.adapter.loom_dir,
                                    task_id,
                                    "terminal",
                                    agent_id,
                                    title,
                                );
                                app.last_action_result = Some(crate::adapter::CommandResult {
                                    action: dispatcher.name().to_string(),
                                    success: result.success,
                                    message: result.message,
                                });
                            }
                            app.close_dispatch_menu();
                        }
                        KeyCode::Char('a') => {
                            app.dispatch_menu_selected = 1;
                            let task = app.selected_task().cloned();
                            if let Some(task) = task {
                                let dispatcher = crate::dispatch::ACPDispatch::new(
                                    app.status
                                        .pointer("/daemon/reachable")
                                        .and_then(Value::as_bool)
                                        .unwrap_or(false),
                                    app.daemon
                                        .pointer("/socket_path")
                                        .and_then(Value::as_str)
                                        .map(String::from),
                                );
                                let result = dispatcher.dispatch(&task);
                                let task_id = task
                                    .pointer("/task_id")
                                    .and_then(Value::as_str)
                                    .unwrap_or("");
                                let agent_id = task
                                    .pointer("/agent_id")
                                    .and_then(Value::as_str)
                                    .unwrap_or("");
                                let title = task
                                    .pointer("/title")
                                    .and_then(Value::as_str)
                                    .unwrap_or("");
                                crate::dispatch::record_dispatch_usage(
                                    &app.adapter.loom_dir,
                                    task_id,
                                    "acp",
                                    agent_id,
                                    title,
                                );
                                app.last_action_result = Some(crate::adapter::CommandResult {
                                    action: dispatcher.name().to_string(),
                                    success: result.success,
                                    message: result.message,
                                });
                            }
                            app.close_dispatch_menu();
                        }
                        KeyCode::Char('e') => {
                            app.dispatch_menu_selected = 2;
                            // Single owner: `App::dispatch_embedded_pane`
                            // spawns the dispatch and wires the pane state.
                            app.last_action_result = Some(app.dispatch_embedded_pane().await);
                            app.close_dispatch_menu();
                        }
                        KeyCode::Char('r') => {
                            app.dispatch_menu_selected = 3;
                            let task = app.selected_task().cloned();
                            if let Some(task) = task {
                                let default_runtime = app
                                    .adapter
                                    .fetch_config()
                                    .await
                                    .ok()
                                    .and_then(|v| {
                                        v.pointer("/workflow/runtime")
                                            .and_then(|s| s.as_str().map(String::from))
                                    })
                                    .unwrap_or_else(|| "process".to_string());
                                let dispatcher = crate::dispatch::RuntimeDispatch::new(
                                    crate::dispatch::default_runtime_registry(),
                                    default_runtime,
                                    app.adapter.loom_dir.clone(),
                                );
                                let result = dispatcher.dispatch(&task);
                                let task_id = task
                                    .pointer("/task_id")
                                    .and_then(Value::as_str)
                                    .unwrap_or("");
                                let agent_id = task
                                    .pointer("/agent_id")
                                    .and_then(Value::as_str)
                                    .unwrap_or("");
                                let title = task
                                    .pointer("/title")
                                    .and_then(Value::as_str)
                                    .unwrap_or("");
                                crate::dispatch::record_dispatch_usage(
                                    &app.adapter.loom_dir,
                                    task_id,
                                    "runtime",
                                    agent_id,
                                    title,
                                );
                                app.last_action_result = Some(crate::adapter::CommandResult {
                                    action: dispatcher.name().to_string(),
                                    success: result.success,
                                    message: result.message,
                                });
                            }
                            app.close_dispatch_menu();
                        }
                        _ => {}
                    }
                    continue;
                }

                if app.action_palette_open {
                    match key.code {
                        KeyCode::Esc | KeyCode::Char('D') => app.close_action_palette(),
                        KeyCode::Enter => app.execute_selected_action().await,
                        KeyCode::Down | KeyCode::Char('j') => app.action_palette_next(),
                        KeyCode::Up | KeyCode::Char('k') => app.action_palette_prev(),
                        _ => {}
                    }
                    continue;
                }

                if app.command_palette_open {
                    match key.code {
                        KeyCode::Esc => {
                            app.close_command_palette();
                        }
                        KeyCode::Enter => {
                            // If embedded pane is active, send input to the child process
                            if app.embedded_task.is_some() && !app.command_palette_query.is_empty()
                            {
                                if let Some(ref mut stdin) = app.embedded_stdin {
                                    use tokio::io::AsyncWriteExt;
                                    let mut line = app.command_palette_query.clone();
                                    line.push('\n');
                                    let _ = stdin.write_all(line.as_bytes()).await;
                                    app.embedded_log
                                        .push(format!("> {}", app.command_palette_query));
                                    app.close_command_palette();
                                    continue;
                                }
                            }
                            if let Some(cmd_name) = app.command_palette_selected_name() {
                                let should_continue = app.execute_named_command(&cmd_name).await;
                                app.close_command_palette();
                                if !should_continue {
                                    break;
                                }
                            }
                        }
                        KeyCode::Down | KeyCode::Char('j') => {
                            let max = app.command_palette_count();
                            if max > 0 {
                                app.command_palette_selected =
                                    (app.command_palette_selected + 1) % max;
                            }
                        }
                        KeyCode::Up | KeyCode::Char('k') => {
                            let max = app.command_palette_count();
                            if max > 0 {
                                app.command_palette_selected = if app.command_palette_selected == 0
                                {
                                    max - 1
                                } else {
                                    app.command_palette_selected - 1
                                };
                            }
                        }
                        KeyCode::Backspace => {
                            app.command_palette_backspace();
                        }
                        KeyCode::Delete => {
                            app.command_palette_backspace();
                        }
                        KeyCode::Char(c)
                            if c.is_alphanumeric() || c == '-' || c == '_' || c == ' ' =>
                        {
                            app.command_palette_append(c);
                        }
                        _ => {}
                    }
                    continue;
                }

                // Orchestrator tab: printable keys are consumed as form
                // input so typing titles like "query" can never trigger
                // global shortcuts (notably `q` for quit).
                //
                // Tab / Shift-Tab are deliberately NOT consumed: they are
                // the only way out of a form tab that swallows the digit
                // shortcuts, so they must reach the global tab navigation.
                // Field focus moved to ↑/↓ and tool selection to ←/→.
                if app.is_orchestrator_tab() {
                    // Ctrl+C quits the TUI from anywhere.
                    if key.code == KeyCode::Char('c')
                        && key
                            .modifiers
                            .contains(crossterm::event::KeyModifiers::CONTROL)
                    {
                        break;
                    }
                    match orchestrator_key(
                        key.code,
                        key.modifiers
                            .contains(crossterm::event::KeyModifiers::CONTROL),
                    ) {
                        Some(OrchestratorKey::ClearStatus) => {
                            app.clear_orchestrator_status();
                            continue;
                        }
                        Some(OrchestratorKey::Submit) => {
                            app.submit_orchestrator().await;
                            continue;
                        }
                        Some(OrchestratorKey::FocusPrev) => {
                            app.orchestrator_focus_previous();
                            continue;
                        }
                        Some(OrchestratorKey::FocusNext) => {
                            app.orchestrator_focus_next();
                            continue;
                        }
                        Some(OrchestratorKey::AgentPrev) => {
                            app.orchestrator_agent_previous();
                            continue;
                        }
                        Some(OrchestratorKey::AgentNext) => {
                            app.orchestrator_agent_next();
                            continue;
                        }
                        Some(OrchestratorKey::Backspace) => {
                            app.orchestrator_backspace();
                            continue;
                        }
                        Some(OrchestratorKey::Text(c)) => {
                            app.orchestrator_append(c);
                            continue;
                        }
                        None => {}
                    }
                }

                // Review tab: navigate tasks and dispatch. All other keys
                // fall through to the global shortcuts below.
                if app.is_review_tab() {
                    match key.code {
                        KeyCode::Up | KeyCode::Char('k') => {
                            app.review_select_prev();
                            continue;
                        }
                        KeyCode::Down | KeyCode::Char('j') => {
                            app.review_select_next();
                            continue;
                        }
                        KeyCode::Enter => {
                            app.dispatch_review_task().await;
                            continue;
                        }
                        KeyCode::Char('R') => {
                            app.resume_review_task().await;
                            continue;
                        }
                        KeyCode::Char('x') => {
                            app.remove_review_task().await;
                            continue;
                        }
                        KeyCode::Char('a') => {
                            app.cycle_review_task_agent().await;
                            continue;
                        }
                        _ => {}
                    }
                }

                // Kanban tab: hjkl move the selection across columns and
                // rows, Enter or a opens the dispatch menu for the selected
                // task. Other keys fall through to the global shortcuts.
                if app.is_kanban_tab() {
                    match kanban_key(key.code) {
                        Some(KanbanKey::PrevCol) => {
                            app.kanban_select_prev_col();
                            continue;
                        }
                        Some(KanbanKey::NextCol) => {
                            app.kanban_select_next_col();
                            continue;
                        }
                        Some(KanbanKey::PrevRow) => {
                            app.kanban_select_prev_row();
                            continue;
                        }
                        Some(KanbanKey::NextRow) => {
                            app.kanban_select_next_row();
                            continue;
                        }
                        Some(KanbanKey::Open) => {
                            if let Some(task_id) = app.kanban_selected_task_id() {
                                if app.select_task_by_id(&task_id) {
                                    app.open_dispatch_menu();
                                }
                            }
                            continue;
                        }
                        Some(KanbanKey::Advance) => {
                            app.advance_kanban_task().await;
                            continue;
                        }
                        Some(KanbanKey::Remove) => {
                            app.remove_kanban_task().await;
                            continue;
                        }
                        None => {}
                    }
                }

                // Embedded pane: Esc to close, j/k to scroll log, i to send input, Ctrl-C to interrupt
                if app.embedded_task.is_some() {
                    match key.code {
                        KeyCode::Esc => {
                            app.close_embedded_pane();
                            continue;
                        }
                        KeyCode::Down | KeyCode::Char('j') => {
                            app.embedded_log_scroll_down();
                            continue;
                        }
                        KeyCode::Up | KeyCode::Char('k') => {
                            app.embedded_log_scroll_up();
                            continue;
                        }
                        KeyCode::Char('c')
                            if key
                                .modifiers
                                .contains(crossterm::event::KeyModifiers::CONTROL) =>
                        {
                            // Send interrupt (SIGINT) to the child process
                            if let Some(ref mut child) = app.embedded_child {
                                let _ = child.start_kill();
                                app.embedded_log
                                    .push("⏹  Process interrupted (SIGINT)".to_string());
                            }
                            continue;
                        }
                        KeyCode::Char('i') => {
                            // Open command palette prefilled for stdin input
                            app.command_palette_query = String::new();
                            app.command_palette_open = true;
                            app.command_palette_selected = 0;
                            // After user types and presses Enter, we'll send input
                            // The Enter handler below checks for this pattern
                            continue;
                        }
                        _ => {}
                    }
                }

                match key.code {
                    KeyCode::Char('q') => break,
                    KeyCode::Char('r') => app.refresh_safe().await,
                    KeyCode::Char('s') => app.run_scan_command().await,
                    KeyCode::Char('d') if app.tab_index == 8 => {
                        app.toggle_diff_view();
                        app.force_pr_poll().await;
                    }
                    KeyCode::Char('d') => app.run_doctor_command().await,
                    KeyCode::Char('?') => app.toggle_help(),
                    KeyCode::Tab | KeyCode::Right => app.next_tab(),
                    KeyCode::BackTab | KeyCode::Left => app.prev_tab(),
                    KeyCode::Down => {
                        if app.tab_index == 8 {
                            app.next_pull();
                            app.force_pr_poll().await;
                        } else if app.is_task_tab() {
                            app.select_next_task();
                        } else {
                            app.scroll_down();
                        }
                    }
                    KeyCode::Up => {
                        if app.tab_index == 8 {
                            app.prev_pull();
                            app.force_pr_poll().await;
                        } else if app.is_task_tab() {
                            app.select_prev_task();
                        } else {
                            app.scroll_up();
                        }
                    }
                    KeyCode::Char('1') => app.set_tab_from_digit(0),
                    KeyCode::Char('2') => app.set_tab_from_digit(1),
                    KeyCode::Char('3') => app.set_tab_from_digit(2),
                    KeyCode::Char('4') => app.set_tab_from_digit(3),
                    KeyCode::Char('5') => app.set_tab_from_digit(4),
                    KeyCode::Char('6') => app.set_tab_from_digit(5),
                    KeyCode::Char('7') => app.set_tab_from_digit(6),
                    KeyCode::Char('8') => app.set_tab_from_digit(7),
                    KeyCode::Char('9') => app.set_tab_from_digit(8),
                    KeyCode::Char('0') => app.set_tab_from_digit(9),
                    KeyCode::Char('!') => app.set_tab_from_digit(10),
                    KeyCode::Char('@') => app.set_tab_from_digit(11),
                    KeyCode::Char('#') => app.set_tab_from_digit(12),
                    KeyCode::Char('c') if app.tab_index == 8 && app.embedded_task.is_none() => {
                        app.create_pull_request().await
                    }
                    KeyCode::Char('D') => app.toggle_action_palette(),
                    KeyCode::Char('a') if app.is_task_tab() => app.open_dispatch_menu(),
                    KeyCode::Char(']') => app.next_theme(),
                    KeyCode::Char('[') => app.prev_theme(),
                    KeyCode::Char('S') => app.toggle_sidebar(),
                    KeyCode::Char('p')
                        if key
                            .modifiers
                            .contains(crossterm::event::KeyModifiers::CONTROL) =>
                    {
                        app.toggle_command_palette();
                    }
                    _ => {}
                }
            } else if let Event::Paste(text) = ev {
                app.orchestrator_paste(&text);
            }
        }

        if last_tick.elapsed() >= refresh_interval {
            app.refresh_safe().await;
            last_tick = Instant::now();
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kanban_does_not_swallow_the_tab_navigation_keys() {
        // `←`/`→` are the global previous/next tab shortcut. If the Kanban
        // tab consumed them, the user could never leave the board.
        assert_eq!(kanban_key(KeyCode::Left), None);
        assert_eq!(kanban_key(KeyCode::Right), None);
        // Tab / Shift-Tab must always reach the global handler.
        assert_eq!(kanban_key(KeyCode::Tab), None);
        assert_eq!(kanban_key(KeyCode::BackTab), None);
    }

    #[test]
    fn kanban_binds_hjkl_and_the_action_keys() {
        assert_eq!(kanban_key(KeyCode::Char('h')), Some(KanbanKey::PrevCol));
        assert_eq!(kanban_key(KeyCode::Char('l')), Some(KanbanKey::NextCol));
        assert_eq!(kanban_key(KeyCode::Char('k')), Some(KanbanKey::PrevRow));
        assert_eq!(kanban_key(KeyCode::Char('j')), Some(KanbanKey::NextRow));
        assert_eq!(kanban_key(KeyCode::Enter), Some(KanbanKey::Open));
        assert_eq!(kanban_key(KeyCode::Char('a')), Some(KanbanKey::Open));
        assert_eq!(kanban_key(KeyCode::Char('>')), Some(KanbanKey::Advance));
        assert_eq!(kanban_key(KeyCode::Char('x')), Some(KanbanKey::Remove));
    }

    #[test]
    fn orchestrator_does_not_swallow_the_tab_navigation_keys() {
        // The form swallows digits as text, so Tab / Shift-Tab are the only
        // way out. They must reach the global tab navigation.
        assert_eq!(orchestrator_key(KeyCode::Tab, false), None);
        assert_eq!(orchestrator_key(KeyCode::BackTab, false), None);
    }

    #[test]
    fn orchestrator_binds_form_keys_and_arrows() {
        assert_eq!(
            orchestrator_key(KeyCode::Esc, false),
            Some(OrchestratorKey::ClearStatus)
        );
        assert_eq!(
            orchestrator_key(KeyCode::Enter, false),
            Some(OrchestratorKey::Submit)
        );
        assert_eq!(
            orchestrator_key(KeyCode::Up, false),
            Some(OrchestratorKey::FocusPrev)
        );
        assert_eq!(
            orchestrator_key(KeyCode::Down, false),
            Some(OrchestratorKey::FocusNext)
        );
        assert_eq!(
            orchestrator_key(KeyCode::Left, false),
            Some(OrchestratorKey::AgentPrev)
        );
        assert_eq!(
            orchestrator_key(KeyCode::Right, false),
            Some(OrchestratorKey::AgentNext)
        );
        assert_eq!(
            orchestrator_key(KeyCode::Backspace, false),
            Some(OrchestratorKey::Backspace)
        );
        assert_eq!(
            orchestrator_key(KeyCode::Char('x'), false),
            Some(OrchestratorKey::Text('x'))
        );
    }

    #[test]
    fn orchestrator_never_types_a_control_chord_into_the_field() {
        // Ctrl+C must quit, never insert a literal 'c'.
        assert_eq!(orchestrator_key(KeyCode::Char('c'), true), None);
    }
}
