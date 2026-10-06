use crate::app::{format_review_task_line, format_run_line, task_session_ids, App};
use crate::theme::Theme;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};
use ratatui::Frame;
use serde_json::Value;

fn section_block<'a>(title: &'a str, theme: &'a Theme) -> Block<'a> {
    Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(theme.border))
        .title(format!(" {title} "))
        .title_style(
            Style::default()
                .fg(theme.text_dim)
                .add_modifier(Modifier::BOLD),
        )
}

fn dim(text: &str, theme: &Theme) -> Line<'static> {
    Line::from(Span::styled(text.to_string(), Style::default().fg(theme.text_dim)))
}

fn draw_task_list(frame: &mut Frame, area: Rect, app: &App, theme: &Theme) {
    let tasks = app
        .workflow
        .pointer("/tasks")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();

    let mut lines: Vec<Line> = Vec::new();
    if tasks.is_empty() {
        lines.push(dim("No workflow tasks yet.", theme));
        lines.push(dim("Create one from the Orchestrator tab.", theme));
    } else {
        for (index, task) in tasks.iter().enumerate() {
            let selected = index == app.review_selection_index;
            let style = if selected {
                Style::default()
                    .fg(theme.selection_fg)
                    .bg(theme.selection_bg)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(theme.text)
            };
            lines.push(Line::from(vec![
                Span::styled(if selected { "▸ " } else { "  " }, style),
                Span::styled(format_review_task_line(task), style),
            ]));
        }
    }

    frame.render_widget(
        Paragraph::new(lines)
            .block(section_block("Tasks", theme))
            .wrap(Wrap { trim: false }),
        area,
    );
}

fn draw_task_detail(frame: &mut Frame, area: Rect, app: &App, theme: &Theme) {
    let mut lines: Vec<Line> = Vec::new();
    let selected_id = app.review_selected_task_id();

    match (selected_id, app.review_detail.as_ref()) {
        (None, _) => {
            lines.push(dim("Select a task to review it.", theme));
        }
        (Some(task_id), detail) => {
            let task = detail.and_then(|d| d.pointer("/task"));
            let title = task
                .and_then(|t| t.get("title"))
                .and_then(Value::as_str)
                .unwrap_or("(loading…)");
            let phase = task
                .and_then(|t| t.get("phase"))
                .and_then(Value::as_str)
                .unwrap_or("?");
            let status = task
                .and_then(|t| t.get("status"))
                .and_then(Value::as_str)
                .unwrap_or("?");
            let agent = task
                .and_then(|t| t.get("last_agent"))
                .and_then(Value::as_str)
                .unwrap_or("(no recorded agent)");

            lines.push(Line::from(vec![
                Span::styled(
                    title.to_string(),
                    Style::default()
                        .fg(theme.text_bright)
                        .add_modifier(Modifier::BOLD),
                ),
            ]));
            lines.push(Line::from(vec![
                Span::styled(format!("{phase} · {status} · @{agent}"), theme.text_dim),
            ]));
            lines.push(dim("", theme));

            lines.push(Line::from(Span::styled(
                "Phase results",
                Style::default()
                    .fg(theme.secondary)
                    .add_modifier(Modifier::BOLD),
            )));
            let results = task
                .and_then(|t| t.get("phase_results"))
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default();
            if results.is_empty() {
                lines.push(dim("(none yet)", theme));
            } else {
                for result in &results {
                    let phase = result
                        .get("phase")
                        .and_then(Value::as_str)
                        .unwrap_or("?");
                    let agent = result
                        .get("agent")
                        .and_then(Value::as_str)
                        .unwrap_or("?");
                    let summary: String = result
                        .get("summary")
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .chars()
                        .take(160)
                        .collect();
                    lines.push(Line::from(vec![
                        Span::styled(format!("- {phase} · @{agent}: "), theme.text),
                        Span::styled(summary, theme.text_dim),
                    ]));
                }
            }
            lines.push(dim("", theme));

            lines.push(Line::from(Span::styled(
                "Sessions",
                Style::default()
                    .fg(theme.secondary)
                    .add_modifier(Modifier::BOLD),
            )));
            let sessions = task_session_ids(&app.sessions, &task_id);
            if sessions.is_empty() {
                lines.push(dim("(none captured yet)", theme));
            } else {
                for session in &sessions {
                    lines.push(Line::from(Span::styled(
                        format!("- {session}"),
                        Style::default().fg(theme.text),
                    )));
                }
            }
            lines.push(dim("", theme));

            lines.push(Line::from(Span::styled(
                "Tracked run",
                Style::default()
                    .fg(theme.secondary)
                    .add_modifier(Modifier::BOLD),
            )));
            match app.orchestrator_last_run.as_ref() {
                Some(run) if run.task_id == task_id => {
                    lines.push(Line::from(Span::styled(
                        format_run_line(run),
                        Style::default().fg(theme.text),
                    )));
                }
                _ => lines.push(dim("(no tracked run for this task)", theme)),
            }
        }
    }

    lines.push(dim("", theme));
    lines.push(Line::from(vec![
        Span::styled("↑↓/j/k select", theme.text_dim),
        Span::styled(" · ", theme.text_dim),
        Span::styled("Enter dispatch", theme.text_dim),
        Span::styled(" · ", theme.text_dim),
        Span::styled("R resume with context", theme.text_dim),
        Span::styled(" · ", theme.text_dim),
        Span::styled("a reassign agent", theme.text_dim),
        Span::styled(" · ", theme.text_dim),
        Span::styled("x remove", theme.text_dim),
    ]));

    frame.render_widget(
        Paragraph::new(lines)
            .block(section_block("Review", theme))
            .wrap(Wrap { trim: false }),
        area,
    );
}

pub fn draw_review(frame: &mut Frame, area: Rect, app: &App, theme: &Theme) {
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(38), Constraint::Percentage(62)])
        .split(area);
    draw_task_list(frame, chunks[0], app, theme);
    draw_task_detail(frame, chunks[1], app, theme);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapter::LoomCliAdapter;
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;

    fn rendered(app: &App) -> String {
        let backend = TestBackend::new(120, 40);
        let mut terminal = Terminal::new(backend).unwrap();
        let theme = app.theme.clone();
        terminal
            .draw(|f| draw_review(f, f.area(), app, &theme))
            .unwrap();
        terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|cell| cell.symbol())
            .collect::<Vec<_>>()
            .join("")
    }

    fn app_with_tasks() -> App {
        let mut app = App::new(LoomCliAdapter::new("loom", crate::loom_dir()));
        app.workflow = serde_json::json!({
            "tasks": [
                {"task_id": "t1", "title": "First task", "phase": "Plan", "status": "Pending"},
            ]
        });
        app.sessions = serde_json::json!({
            "sessions": [{"session_id": "t1.Plan"}]
        });
        app.review_detail = Some(serde_json::json!({
            "task": {
                "task_id": "t1",
                "title": "First task",
                "phase": "Plan",
                "status": "Pending",
                "last_agent": "goose",
                "phase_results": [
                    {"phase": "Plan", "agent": "goose", "summary": "did planning"}
                ],
            }
        }));
        app.review_detail_task_id = Some("t1".to_string());
        app.tab_index = app.review_tab_index().unwrap();
        app
    }

    #[test]
    fn draw_review_shows_tasks_detail_and_sessions() {
        let screen = rendered(&app_with_tasks());
        assert!(screen.contains("Tasks"), "missing task list pane");
        assert!(screen.contains("First task"), "missing task title");
        assert!(screen.contains("Phase results"), "missing results section");
        assert!(screen.contains("did planning"), "missing result summary");
        assert!(screen.contains("t1.Plan"), "missing linked session");
        assert!(screen.contains("Enter dispatch"), "missing key hints");
    }

    #[test]
    fn draw_review_empty_state_renders() {
        let mut app = App::new(LoomCliAdapter::new("loom", crate::loom_dir()));
        app.tab_index = app.review_tab_index().unwrap_or(0);
        let screen = rendered(&app);
        assert!(screen.contains("No workflow tasks yet"), "got: {screen}");
    }
}
