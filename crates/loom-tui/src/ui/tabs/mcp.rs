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

    let count = app.mcp.pointer("/count").and_then(Value::as_u64).unwrap_or(0);
    lines.push(Line::from(vec![
        Span::styled("Registered: ", theme.text_dim),
        Span::styled(
            format!("{}", count),
            Style::default().fg(theme.text_bright).add_modifier(Modifier::BOLD),
        ),
    ]));

    if let Some(servers) = app.mcp.pointer("/servers").and_then(Value::as_array) {
        if servers.is_empty() {
            lines.push(Line::from(Span::styled("", Style::default())));
            lines.push(Line::from(Span::styled(
                "No MCP servers registered.",
                theme.text_dim,
            )));
            lines.push(Line::from(Span::styled(
                "Add configs in ~/.loom/mcp/ or import from tools.",
                theme.text_dim,
            )));
        } else {
            for s in servers {
                lines.push(Line::from(Span::styled("", Style::default())));
                let id = s.pointer("/id").and_then(Value::as_str).unwrap_or("?");
                let name = s.pointer("/name").and_then(Value::as_str).unwrap_or("?");

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

                if let Some(transport) = s.pointer("/transport") {
                    let transport_str = match transport {
                        Value::String(s) => s.clone(),
                        _ => transport.to_string(),
                    };
                    lines.push(Line::from(vec![
                        Span::styled("     Transport: ", theme.text_dim),
                        Span::styled(transport_str, theme.text),
                    ]));
                }
            }
        }
    }

    frame.render_widget(
        Paragraph::new(lines).block(block).wrap(Wrap { trim: false }),
        area,
    );
}
