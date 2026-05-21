use crate::app::App;
use crate::theme::Theme;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};
use ratatui::Frame;
use serde_json::Value;

pub fn draw_status_bar(frame: &mut Frame, area: ratatui::layout::Rect, app: &App, theme: &Theme) {
    let spans = build_status_spans(app, theme);
    frame.render_widget(
        Paragraph::new(Line::from(spans)).block(
            Block::default()
                .borders(Borders::TOP)
                .border_style(Style::default().fg(theme.border)),
        ),
        area,
    );
}

fn build_status_spans<'a>(app: &App, theme: &Theme) -> Vec<Span<'a>> {
    let mut spans: Vec<Span> = Vec::new();

    let daemon_ok = app
        .status
        .pointer("/daemon/reachable")
        .and_then(Value::as_bool)
        .unwrap_or(false);

    if daemon_ok {
        spans.push(Span::styled(
            " ● ",
            Style::default().fg(theme.success),
        ));
        spans.push(Span::styled(
            "Daemon",
            Style::default().fg(theme.success),
        ));
    } else {
        spans.push(Span::styled(
            " ○ ",
            Style::default().fg(theme.error),
        ));
        spans.push(Span::styled(
            "Daemon",
            Style::default().fg(theme.error),
        ));
    }

    spans.push(Span::styled(" │ ", theme.text_dim));

    let tool_count = app
        .scan
        .pointer("/system_audit/tool_hits")
        .and_then(Value::as_array)
        .map(|a| a.len())
        .unwrap_or(0);
    let skill_count = app
        .skills
        .pointer("/count")
        .and_then(Value::as_u64)
        .unwrap_or(0);
    let mcp_count = app
        .mcp
        .pointer("/count")
        .and_then(Value::as_u64)
        .unwrap_or(0);
    let task_count = app
        .workflow
        .pointer("/count")
        .and_then(Value::as_u64)
        .unwrap_or(0);

    spans.push(Span::styled("Tools:", theme.text_dim));
    spans.push(Span::styled(format!("{} ", tool_count), theme.text));
    spans.push(Span::styled("Skills:", theme.text_dim));
    spans.push(Span::styled(format!("{} ", skill_count), theme.text));
    spans.push(Span::styled("MCP:", theme.text_dim));
    spans.push(Span::styled(format!("{} ", mcp_count), theme.text));
    spans.push(Span::styled("Tasks:", theme.text_dim));
    spans.push(Span::styled(format!("{} ", task_count), theme.text));

    spans.push(Span::styled("│ ", theme.text_dim));

    if app.action_in_progress {
        spans.push(Span::styled(
            " RUNNING… ",
            Style::default()
                .fg(theme.warning)
                .add_modifier(Modifier::BOLD),
        ));
    } else if let Some(result) = &app.last_action_result {
        if result.success {
            spans.push(Span::styled(
                format!("✓ {} ", result.action),
                Style::default().fg(theme.success),
            ));
        } else {
            spans.push(Span::styled(
                format!("✗ {} ", result.action),
                Style::default().fg(theme.error),
            ));
        }
    }

    if let Some(err) = &app.last_error {
        spans.push(Span::styled(
            format!("⚠ {} ", err),
            Style::default().fg(theme.error),
        ));
    }

    spans.push(Span::styled(
        format!("┃ {:.1}s", app.last_refresh.elapsed().as_secs_f64()),
        theme.text_dim,
    ));

    spans
}
