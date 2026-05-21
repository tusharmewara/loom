use crate::app::App;
use crate::theme::Theme;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};
use ratatui::Frame;
use serde_json::Value;

pub fn draw_discovery(frame: &mut Frame, area: ratatui::layout::Rect, app: &App, theme: &Theme) {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(theme.border))
        .title(" Discovery ")
        .title_style(
            Style::default()
                .fg(theme.primary)
                .add_modifier(Modifier::BOLD),
        );

    let mut lines: Vec<Line> = Vec::new();

    let phase1 = app
        .scan
        .pointer("/ecosystem/phase_1_duration_us")
        .and_then(Value::as_u64)
        .unwrap_or(0);
    let phase2 = app
        .scan
        .pointer("/ecosystem/phase_2/phase_2_duration_us")
        .and_then(Value::as_u64)
        .unwrap_or(0);
    let warnings = app
        .scan
        .pointer("/warnings")
        .and_then(Value::as_array)
        .map(|a| a.len())
        .unwrap_or(0);

    lines.push(Line::from(vec![
        Span::styled("Phase 1: ", theme.text_dim),
        Span::styled(format!("{}μs", phase1), theme.text),
    ]));
    lines.push(Line::from(vec![
        Span::styled("Phase 2: ", theme.text_dim),
        Span::styled(format!("{}μs", phase2), theme.text),
    ]));
    lines.push(Line::from(vec![
        Span::styled("Warnings: ", theme.text_dim),
        Span::styled(
            format!("{}", warnings),
            if warnings > 0 {
                Style::default().fg(theme.warning)
            } else {
                Style::default().fg(theme.text)
            },
        ),
    ]));

    let counts = app.scan.pointer("/ecosystem/resource_counts");
    if let Some(c) = counts {
        if let Some(obj) = c.as_object() {
            lines.push(Line::from(Span::styled("", Style::default())));
            lines.push(Line::from(Span::styled(
                "Resource counts",
                Style::default()
                    .fg(theme.secondary)
                    .add_modifier(Modifier::BOLD),
            )));
            for (k, v) in obj {
                lines.push(Line::from(vec![
                    Span::styled("  ", theme.text_dim),
                    Span::styled(format!("{}: ", k), theme.text_dim),
                    Span::styled(
                        format!("{}", v.as_u64().unwrap_or(0)),
                        theme.text_bright,
                    ),
                ]));
            }
        }
    }

    if let Some(tools) = app
        .scan
        .pointer("/system_audit/tool_hits")
        .and_then(Value::as_array)
    {
        lines.push(Line::from(Span::styled("", Style::default())));
        lines.push(Line::from(Span::styled(
            "Detected tools",
            Style::default()
                .fg(theme.secondary)
                .add_modifier(Modifier::BOLD),
        )));
        for t in tools {
            let name = t.pointer("/tool").and_then(Value::as_str).unwrap_or("?");
            let path = t.pointer("/path").and_then(Value::as_str).unwrap_or("?");
            lines.push(Line::from(vec![
                Span::styled("  • ", theme.primary),
                Span::styled(format!("{} ", name), theme.text),
                Span::styled(format!("→ {}", path), theme.text_dim),
            ]));
        }
    }

    if let Some(skill_src) = app
        .scan
        .pointer("/system_audit/skill_sources")
        .and_then(Value::as_array)
    {
        lines.push(Line::from(Span::styled("", Style::default())));
        lines.push(Line::from(Span::styled(
            "Skill sources",
            Style::default()
                .fg(theme.secondary)
                .add_modifier(Modifier::BOLD),
        )));
        for s in skill_src {
            let source = s.pointer("/source").and_then(Value::as_str).unwrap_or("?");
            let count = s.pointer("/skill_count").and_then(Value::as_u64).unwrap_or(0);
            let path = s.pointer("/path").and_then(Value::as_str).unwrap_or("?");
            lines.push(Line::from(vec![
                Span::styled("  • ", theme.primary),
                Span::styled(
                    format!("{} ({} skills)", source, count),
                    theme.text,
                ),
                Span::styled(format!(" → {}", path), theme.text_dim),
            ]));
        }
    }

    if let Some(mcp_src) = app
        .scan
        .pointer("/system_audit/mcp_sources")
        .and_then(Value::as_array)
    {
        lines.push(Line::from(Span::styled("", Style::default())));
        lines.push(Line::from(Span::styled(
            "MCP sources",
            Style::default()
                .fg(theme.secondary)
                .add_modifier(Modifier::BOLD),
        )));
        for m in mcp_src {
            let source = m.pointer("/source").and_then(Value::as_str).unwrap_or("?");
            let count = m.pointer("/server_count").and_then(Value::as_u64).unwrap_or(0);
            let path = m.pointer("/path").and_then(Value::as_str).unwrap_or("?");
            lines.push(Line::from(vec![
                Span::styled("  • ", theme.primary),
                Span::styled(
                    format!("{} ({} servers)", source, count),
                    theme.text,
                ),
                Span::styled(format!(" → {}", path), theme.text_dim),
            ]));
        }
    }

    frame.render_widget(
        Paragraph::new(lines).block(block).wrap(Wrap { trim: false }),
        area,
    );
}
