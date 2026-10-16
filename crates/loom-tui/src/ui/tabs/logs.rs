use crate::app::App;
use crate::theme::Theme;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};
use ratatui::Frame;
use serde_json::Value;

pub fn draw_logs(frame: &mut Frame, area: ratatui::layout::Rect, app: &App, theme: &Theme) {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(theme.border))
        .title(" Logs / Activity ")
        .title_style(
            Style::default()
                .fg(theme.primary)
                .add_modifier(Modifier::BOLD),
        );

    let mut lines: Vec<Line> = Vec::new();

    lines.push(Line::from(vec![
        Span::styled("Last refresh: ", theme.text_dim),
        Span::styled(
            format!("{:.1}s ago", app.last_refresh.elapsed().as_secs_f64()),
            theme.text,
        ),
    ]));

    if let Some(result) = &app.last_action_result {
        lines.push(Line::from(vec![
            Span::styled("Last command: ", theme.text_dim),
            Span::styled(
                format!("[{}] {}", result.action, result.message),
                if result.success {
                    Style::default().fg(theme.success)
                } else {
                    Style::default().fg(theme.error)
                },
            ),
        ]));
    }

    if let Some(err) = &app.last_error {
        lines.push(Line::from(vec![
            Span::styled("Last error: ", theme.text_dim),
            Span::styled(err, Style::default().fg(theme.error)),
        ]));
    }

    lines.push(Line::from(Span::styled("", Style::default())));
    lines.push(Line::from(Span::styled(
        "Workflow Tasks",
        Style::default()
            .fg(theme.secondary)
            .add_modifier(Modifier::BOLD),
    )));

    let task_count = app
        .workflow
        .pointer("/count")
        .and_then(Value::as_u64)
        .unwrap_or(0);
    lines.push(Line::from(vec![
        Span::styled("  Active: ", theme.text_dim),
        Span::styled(format!("{}", task_count), theme.text_bright),
    ]));

    if let Some(tasks) = app.workflow.pointer("/tasks").and_then(Value::as_array) {
        for (i, t) in tasks.iter().enumerate() {
            let id = t.pointer("/task_id").and_then(Value::as_str).unwrap_or("?");
            let title = t.pointer("/title").and_then(Value::as_str).unwrap_or("");
            let phase = t.pointer("/phase").and_then(Value::as_str).unwrap_or("?");
            let status = t.pointer("/status").and_then(Value::as_str).unwrap_or("?");

            let prefix = if i == app.task_selection_index && app.tab_index == 6 {
                "▸ "
            } else {
                "  "
            };

            let phase_color = match phase {
                "completed" => theme.success,
                "failed" | "error" => theme.error,
                "running" | "in_progress" => theme.warning,
                _ => theme.text,
            };

            lines.push(Line::from(vec![
                Span::styled(prefix, Style::default().fg(theme.primary)),
                Span::styled(
                    format!("{} ", &id[..8.min(id.len())]),
                    Style::default().fg(theme.text_dim),
                ),
                Span::styled(
                    format!("{} ", title),
                    Style::default().fg(theme.text).add_modifier(Modifier::BOLD),
                ),
                Span::styled(phase, Style::default().fg(phase_color)),
                Span::styled(format!(" ({})", status), theme.text_dim),
            ]));
        }
    }

    lines.push(Line::from(Span::styled("", Style::default())));
    lines.push(Line::from(Span::styled(
        "Hooks",
        Style::default()
            .fg(theme.secondary)
            .add_modifier(Modifier::BOLD),
    )));

    let hook_count = app
        .hooks
        .pointer("/count")
        .and_then(Value::as_u64)
        .unwrap_or(0);
    lines.push(Line::from(vec![
        Span::styled("  Registered: ", theme.text_dim),
        Span::styled(format!("{}", hook_count), theme.text_bright),
    ]));

    if let Some(hooks) = app.hooks.pointer("/hooks").and_then(Value::as_array) {
        for h in hooks {
            let id = h.pointer("/id").and_then(Value::as_str).unwrap_or("?");
            let event = h
                .pointer("/event_type")
                .and_then(Value::as_str)
                .unwrap_or("?");
            let path = h
                .pointer("/script_path")
                .and_then(Value::as_str)
                .unwrap_or("");
            lines.push(Line::from(vec![
                Span::styled("  • ", theme.primary),
                Span::styled(id, theme.text),
                Span::styled(format!(" | {} | {}", event, path), theme.text_dim),
            ]));
        }
    }

    frame.render_widget(
        Paragraph::new(lines)
            .block(block)
            .wrap(Wrap { trim: false }),
        area,
    );
}
