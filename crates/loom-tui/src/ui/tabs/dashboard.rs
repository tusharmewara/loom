use crate::app::App;
use crate::theme::Theme;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};
use ratatui::Frame;
use serde_json::Value;

pub fn draw_dashboard(frame: &mut Frame, area: ratatui::layout::Rect, app: &App, theme: &Theme) {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(theme.border))
        .title(" Dashboard ")
        .title_style(
            Style::default()
                .fg(theme.primary)
                .add_modifier(Modifier::BOLD),
        );

    let mut lines: Vec<Line> = Vec::new();

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
    let total = app
        .status
        .pointer("/resources/total")
        .and_then(Value::as_u64)
        .unwrap_or(0);

    let indicator = if daemon_ok { "●" } else { "○" };
    let daemon_color = if daemon_ok {
        theme.success
    } else {
        theme.error
    };
    lines.push(Line::from(vec![
        Span::styled(
            "Daemon: ",
            Style::default().fg(theme.text).add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            indicator,
            Style::default()
                .fg(daemon_color)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(format!(" {} ", daemon_status), theme.text),
    ]));

    // Runtime status
    let workflow = &app.workflow;
    let current_runtime = workflow
        .pointer("/runtime")
        .and_then(Value::as_str)
        .unwrap_or("process");
    lines.push(Line::from(vec![
        Span::styled(
            "Runtime: ",
            Style::default().fg(theme.text).add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            current_runtime.to_string(),
            Style::default()
                .fg(theme.primary)
                .add_modifier(Modifier::BOLD),
        ),
    ]));

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
            "Health:  ",
            Style::default().fg(theme.text).add_modifier(Modifier::BOLD),
        ),
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

    lines.push(Line::from(Span::styled(
        format!("Resources: {} total", total),
        Style::default().fg(theme.text),
    )));

    let resource_counts = app.status.pointer("/resources/counts");
    if let Some(counts) = resource_counts {
        if let Some(obj) = counts.as_object() {
            let mut count_spans = Vec::new();
            for (i, (k, v)) in obj.iter().enumerate() {
                if i > 0 {
                    count_spans.push(Span::styled("  ", theme.text_dim));
                }
                let val_str = format!("{}", v.as_u64().unwrap_or(0));
                count_spans.push(Span::styled(format!("{} ", k), theme.text_dim));
                count_spans.push(Span::styled(
                    val_str,
                    Style::default().fg(theme.text_bright),
                ));
            }
            lines.push(Line::from(count_spans));
        }
    }

    lines.push(Line::from(Span::styled("", Style::default())));

    // Use resource counts from status for consistent numbers across dashboard.
    // The individual tab views (MCP, Skills) may show registered-only counts,
    // but the dashboard aggregates all detected resources.
    let tool_count = app
        .scan
        .pointer("/system_audit/tool_hits")
        .and_then(Value::as_array)
        .map(|a| a.len())
        .unwrap_or(0);

    let resource_counts = app.status.pointer("/resources/counts");
    let skill_count = resource_counts
        .and_then(|c| c.pointer("/skills").and_then(Value::as_u64))
        .unwrap_or(0);
    let mcp_count = resource_counts
        .and_then(|c| c.pointer("/mcp_servers").and_then(Value::as_u64))
        .unwrap_or(0);
    let session_count = resource_counts
        .and_then(|c| c.pointer("/sessions").and_then(Value::as_u64))
        .unwrap_or(0);
    // hooks, tasks don't appear in resource_counts — fall back to their own commands
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

    lines.push(Line::from(vec![
        Span::styled(
            format!("Tools: {}", tool_count),
            Style::default().fg(theme.text),
        ),
        Span::styled("  ", theme.text_dim),
        Span::styled(
            format!("Skills: {}", skill_count),
            Style::default().fg(theme.text),
        ),
        Span::styled("  ", theme.text_dim),
        Span::styled(
            format!("MCP: {}", mcp_count),
            Style::default().fg(theme.text),
        ),
    ]));
    lines.push(Line::from(vec![
        Span::styled(
            format!("Sessions: {}", session_count),
            Style::default().fg(theme.text),
        ),
        Span::styled("  ", theme.text_dim),
        Span::styled(
            format!("Hooks: {}", hook_count),
            Style::default().fg(theme.text),
        ),
        Span::styled("  ", theme.text_dim),
        Span::styled(
            format!("Tasks: {}", task_count),
            Style::default().fg(theme.text),
        ),
    ]));

    if let Some(warnings) = app.scan.pointer("/warnings").and_then(Value::as_array) {
        if !warnings.is_empty() {
            lines.push(Line::from(Span::styled("", Style::default())));
            lines.push(Line::from(Span::styled(
                "Warnings",
                Style::default()
                    .fg(theme.warning)
                    .add_modifier(Modifier::BOLD),
            )));
            for w in warnings {
                if let Some(msg) = w.as_str() {
                    lines.push(Line::from(Span::styled(
                        format!("  ⚠ {}", msg),
                        Style::default().fg(theme.warning),
                    )));
                }
            }
        }
    }

    frame.render_widget(
        Paragraph::new(lines)
            .block(block)
            .wrap(Wrap { trim: false }),
        area,
    );
}
