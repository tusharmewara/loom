use crate::app::App;
use crate::theme::Theme;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};
use ratatui::Frame;
use serde_json::Value;

pub fn draw_sidebar(frame: &mut Frame, area: Rect, app: &App, theme: &Theme) {
    let lines = build_sidebar_lines(app, theme);
    let block = Block::default()
        .borders(Borders::LEFT)
        .border_style(Style::default().fg(theme.border))
        .title("Status")
        .title_style(Style::default().fg(theme.text_dim));
    frame.render_widget(
        Paragraph::new(lines)
            .block(block)
            .wrap(Wrap { trim: false }),
        area,
    );
}

fn build_sidebar_lines<'a>(app: &App, theme: &Theme) -> Vec<Line<'a>> {
    let mut lines = Vec::new();

    let daemon_ok = app
        .status
        .pointer("/daemon/reachable")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let daemon_status = app
        .status
        .pointer("/daemon/status")
        .and_then(Value::as_str)
        .unwrap_or("unknown");

    lines.push(Line::from(vec![
        Span::styled(
            if daemon_ok { "● " } else { "○ " },
            Style::default()
                .fg(if daemon_ok { theme.success } else { theme.error })
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled("Daemon", Style::default().fg(theme.text)),
    ]));
    lines.push(Line::from(Span::styled(
        format!(" {}", daemon_status),
        theme.text_dim,
    )));
    lines.push(Line::from(Span::styled("", Style::default())));

    lines.push(Line::from(Span::styled(
        "── Resources ──",
        theme.text_dim,
    )));

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
    let session_count = app
        .sessions
        .pointer("/count")
        .and_then(Value::as_u64)
        .unwrap_or(0);
    let hook_count = app
        .hooks
        .pointer("/count")
        .and_then(Value::as_u64)
        .unwrap_or(0);
    let task_count = app
        .workflow
        .pointer("/count")
        .and_then(Value::as_u64)
        .unwrap_or(0);

    lines.push(format_line("Tools", tool_count as u64, theme));
    lines.push(format_line("Skills", skill_count, theme));
    lines.push(format_line("MCP", mcp_count, theme));
    lines.push(format_line("Sessions", session_count, theme));
    lines.push(format_line("Hooks", hook_count, theme));
    lines.push(format_line("Tasks", task_count, theme));

    lines.push(Line::from(Span::styled("", Style::default())));

    let total = app
        .status
        .pointer("/resources/total")
        .and_then(Value::as_u64)
        .unwrap_or(0);
    lines.push(Line::from(vec![
        Span::styled("Total: ", theme.text_dim),
        Span::styled(
            format!("{}", total),
            Style::default().fg(theme.text_bright),
        ),
    ]));

    lines.push(Line::from(Span::styled("", Style::default())));
    lines.push(Line::from(Span::styled(
        "── Health ──",
        theme.text_dim,
    )));

    let doctor_pass = app
        .doctor
        .pointer("/summary/pass")
        .and_then(Value::as_u64)
        .unwrap_or(0);
    let doctor_warn = app
        .doctor
        .pointer("/summary/warn")
        .and_then(Value::as_u64)
        .unwrap_or(0);
    let doctor_fail = app
        .doctor
        .pointer("/summary/fail")
        .and_then(Value::as_u64)
        .unwrap_or(0);

    lines.push(Line::from(vec![
        Span::styled(
            format!("✓ {} ", doctor_pass),
            Style::default().fg(theme.success),
        ),
        Span::styled(
            format!("⚠ {} ", doctor_warn),
            Style::default().fg(theme.warning),
        ),
        Span::styled(
            format!("✗ {}", doctor_fail),
            Style::default().fg(theme.error),
        ),
    ]));

    lines.push(Line::from(Span::styled("", Style::default())));
    let last_refresh = app.last_refresh.elapsed().as_secs_f64();
    lines.push(Line::from(vec![
        Span::styled("Updated ", theme.text_dim),
        Span::styled(
            format!("{:.1}s ago", last_refresh),
            theme.text_dim,
        ),
    ]));

    lines
}

fn format_line<'a>(label: &'a str, count: u64, theme: &Theme) -> Line<'a> {
    Line::from(vec![
        Span::styled(format!(" {} ", dot(count)), theme.text_dim),
        Span::styled(label, theme.text_dim),
        Span::styled(
            format!("  {}", count),
            Style::default().fg(theme.text_bright),
        ),
    ])
}

fn dot(count: u64) -> &'static str {
    if count > 0 {
        "•"
    } else {
        " "
    }
}
