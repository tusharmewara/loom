use crate::app::App;
use crate::theme::Theme;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};
use ratatui::Frame;
use serde_json::Value;

pub fn draw_sessions(frame: &mut Frame, area: ratatui::layout::Rect, app: &App, theme: &Theme) {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(theme.border))
        .title(" Sessions ")
        .title_style(
            Style::default()
                .fg(theme.primary)
                .add_modifier(Modifier::BOLD),
        );

    let mut lines: Vec<Line> = Vec::new();

    let count = app.sessions.pointer("/count").and_then(Value::as_u64).unwrap_or(0);
    lines.push(Line::from(vec![
        Span::styled("Stored: ", theme.text_dim),
        Span::styled(
            format!("{}", count),
            Style::default().fg(theme.text_bright).add_modifier(Modifier::BOLD),
        ),
    ]));

    if let Some(sessions) = app.sessions.pointer("/sessions").and_then(Value::as_array) {
        if sessions.is_empty() {
            lines.push(Line::from(Span::styled("", Style::default())));
            lines.push(Line::from(Span::styled(
                "No sessions stored yet.",
                theme.text_dim,
            )));
            lines.push(Line::from(Span::styled(
                "Use: loom sessions export <id> / import <path>",
                theme.text_dim,
            )));
        } else {
            for (i, s) in sessions.iter().enumerate() {
                lines.push(Line::from(Span::styled("", Style::default())));
                let id = s.pointer("/session_id").and_then(Value::as_str).unwrap_or("?");
                let tool = s.pointer("/source_tool").and_then(Value::as_str).unwrap_or("?");
                let msgs = s.pointer("/message_count").and_then(Value::as_u64).unwrap_or(0);
                let topic = s.pointer("/topic").and_then(Value::as_str).unwrap_or("");
                let exported = s.pointer("/exported_at").and_then(Value::as_str).unwrap_or("");

                let prefix = if i == app.task_selection_index && app.tab_index == 5 {
                    "▸ "
                } else {
                    "  "
                };

                lines.push(Line::from(vec![
                    Span::styled(prefix, Style::default().fg(theme.primary)),
                    Span::styled(
                        id,
                        Style::default()
                            .fg(theme.text_bright)
                            .add_modifier(Modifier::BOLD),
                    ),
                ]));
                lines.push(Line::from(vec![
                    Span::styled("     Tool: ", theme.text_dim),
                    Span::styled(tool, theme.text),
                    Span::styled(" | Messages: ", theme.text_dim),
                    Span::styled(format!("{}", msgs), theme.text),
                ]));
                if !topic.is_empty() {
                    lines.push(Line::from(vec![
                        Span::styled("     Topic: ", theme.text_dim),
                        Span::styled(topic, theme.text_bright),
                    ]));
                }
                if !exported.is_empty() {
                    lines.push(Line::from(vec![
                        Span::styled("     Exported: ", theme.text_dim),
                        Span::styled(exported, theme.text_dim),
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
