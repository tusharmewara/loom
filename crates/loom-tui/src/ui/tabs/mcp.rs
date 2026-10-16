use crate::app::App;
use crate::theme::Theme;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};
use ratatui::Frame;
use serde_json::Value;

pub fn draw_mcp(frame: &mut Frame, area: ratatui::layout::Rect, app: &App, theme: &Theme) {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(theme.border))
        .title(" MCP Servers ")
        .title_style(
            Style::default()
                .fg(theme.primary)
                .add_modifier(Modifier::BOLD),
        );

    let mut lines: Vec<Line> = Vec::new();

    let registered_count = app
        .mcp
        .pointer("/count")
        .and_then(Value::as_u64)
        .unwrap_or(0);
    let detected_count = app
        .status
        .pointer("/resources/counts/mcp_servers")
        .and_then(Value::as_u64)
        .unwrap_or(0);

    lines.push(Line::from(vec![
        Span::styled("Registered: ", theme.text_dim),
        Span::styled(
            format!("{}", registered_count),
            Style::default()
                .fg(theme.text_bright)
                .add_modifier(Modifier::BOLD),
        ),
    ]));
    if detected_count > registered_count {
        lines.push(Line::from(vec![
            Span::styled("Detected (cross-tool): ", theme.text_dim),
            Span::styled(
                format!("{}", detected_count),
                Style::default().fg(theme.warning),
            ),
        ]));
    }

    // Show registered servers first
    if let Some(servers) = app.mcp.pointer("/servers").and_then(Value::as_array) {
        if !servers.is_empty() {
            for s in servers {
                lines.push(Line::from(Span::styled("", Style::default())));
                let id = s.pointer("/id").and_then(Value::as_str).unwrap_or("?");
                let name = s.pointer("/name").and_then(Value::as_str).unwrap_or("?");

                let transport_str = s.pointer("/transport").map(|t| match t {
                    Value::String(s) => s.clone(),
                    _ => t.to_string(),
                });

                lines.push(Line::from(vec![
                    Span::styled("  ● ", Style::default().fg(theme.success)),
                    Span::styled(
                        id,
                        Style::default()
                            .fg(theme.text_bright)
                            .add_modifier(Modifier::BOLD),
                    ),
                ]));
                lines.push(Line::from(vec![
                    Span::styled("     Name: ", theme.text_dim),
                    Span::styled(name, theme.text),
                ]));
                if let Some(t) = transport_str {
                    lines.push(Line::from(vec![
                        Span::styled("     Transport: ", theme.text_dim),
                        Span::styled(t, theme.text),
                    ]));
                }
            }
        }
    }

    // Show cross-tool detected servers when no registered ones exist
    if registered_count == 0 {
        if let Some(sources) = app
            .scan
            .pointer("/system_audit/mcp_sources")
            .and_then(Value::as_array)
        {
            if !sources.is_empty() && detected_count > 0 {
                lines.push(Line::from(Span::styled("", Style::default())));
                lines.push(Line::from(Span::styled(
                    "Detected from other tools:",
                    Style::default()
                        .fg(theme.secondary)
                        .add_modifier(Modifier::BOLD),
                )));
                for s in sources {
                    let source = s.pointer("/source").and_then(Value::as_str).unwrap_or("?");
                    let count = s
                        .pointer("/server_count")
                        .and_then(Value::as_u64)
                        .unwrap_or(0);
                    let path = s.pointer("/path").and_then(Value::as_str).unwrap_or("");
                    lines.push(Line::from(vec![
                        Span::styled("  • ", theme.primary),
                        Span::styled(source, theme.text),
                        Span::styled(format!(" ({} servers)", count), theme.text_dim),
                    ]));
                    if !path.is_empty() {
                        lines.push(Line::from(vec![
                            Span::styled("    ", theme.text_dim),
                            Span::styled(path, theme.text_dim),
                        ]));
                    }
                }
                lines.push(Line::from(Span::styled("", Style::default())));
                lines.push(Line::from(Span::styled(
                    "Use 'loom mcp add <name>' to import servers into loom.",
                    theme.text_dim,
                )));
            } else {
                lines.push(Line::from(Span::styled("", Style::default())));
                lines.push(Line::from(Span::styled(
                    "No MCP servers registered or detected.",
                    theme.text_dim,
                )));
                lines.push(Line::from(Span::styled(
                    "Add configs in ~/.loom/mcp/ or import from tools.",
                    theme.text_dim,
                )));
            }
        } else {
            lines.push(Line::from(Span::styled("", Style::default())));
            lines.push(Line::from(Span::styled(
                "No MCP servers registered.",
                theme.text_dim,
            )));
            lines.push(Line::from(Span::styled(
                "Add configs in ~/.loom/mcp/ or import from tools.",
                theme.text_dim,
            )));
        }
    }

    frame.render_widget(
        Paragraph::new(lines)
            .block(block)
            .wrap(Wrap { trim: false }),
        area,
    );
}
