use crate::app::App;
use crate::theme::Theme;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};
use ratatui::Frame;
use serde_json::Value;

pub fn draw_knowledge(frame: &mut Frame, area: ratatui::layout::Rect, app: &App, theme: &Theme) {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(theme.border))
        .title(" Knowledge Graph ")
        .title_style(
            Style::default()
                .fg(theme.primary)
                .add_modifier(Modifier::BOLD),
        );

    let mut lines: Vec<Line> = Vec::new();

    let stats = &app.knowledge;

    if stats.is_null() {
        lines.push(Line::from(Span::styled(
            "No data. Run 'loom knowledge build' first.",
            theme.text_dim,
        )));
        lines.push(Line::from(Span::styled("", Style::default())));
        lines.push(Line::from(Span::styled(
            "Or start a daemon to see live stats.",
            theme.text_dim,
        )));
    } else {
        // Entity count
        let entity_count = stats.pointer("/entity_count").and_then(Value::as_u64).unwrap_or(0);
        let rel_count = stats.pointer("/relationship_count").and_then(Value::as_u64).unwrap_or(0);

        lines.push(Line::from(vec![
            Span::styled("Entities:      ", Style::default().fg(theme.text).add_modifier(Modifier::BOLD)),
            Span::styled(format!("{}", entity_count), Style::default().fg(theme.text_bright).add_modifier(Modifier::BOLD)),
        ]));
        lines.push(Line::from(vec![
            Span::styled("Relationships: ", Style::default().fg(theme.text).add_modifier(Modifier::BOLD)),
            Span::styled(format!("{}", rel_count), Style::default().fg(theme.text_bright).add_modifier(Modifier::BOLD)),
        ]));

        // Type breakdown
        if let Some(types) = stats.pointer("/type_breakdown").and_then(Value::as_object) {
            if !types.is_empty() {
                lines.push(Line::from(Span::styled("", Style::default())));
                lines.push(Line::from(Span::styled(
                    "Entity types:",
                    Style::default().fg(theme.text_dim).add_modifier(Modifier::BOLD),
                )));
                for (k, v) in types.iter() {
                    let count = v.as_u64().unwrap_or(0);
                    lines.push(Line::from(vec![
                        Span::styled("  • ", theme.primary),
                        Span::styled(format!("{}: ", k), theme.text),
                        Span::styled(format!("{}", count), Style::default().fg(theme.text_bright)),
                    ]));
                }
            }
        }

        // Top predicates
        if let Some(preds) = stats.pointer("/top_predicates").and_then(Value::as_object) {
            if !preds.is_empty() {
                lines.push(Line::from(Span::styled("", Style::default())));
                lines.push(Line::from(Span::styled(
                    "Top relationships:",
                    Style::default().fg(theme.text_dim).add_modifier(Modifier::BOLD),
                )));
                for (k, v) in preds.iter() {
                    let count = v.as_u64().unwrap_or(0);
                    lines.push(Line::from(vec![
                        Span::styled("  • ", theme.primary),
                        Span::styled(format!("{} ({})", k, count), theme.text),
                    ]));
                }
            }
        }

        // Most connected
        if let Some(connected) = stats.pointer("/most_connected").and_then(Value::as_array) {
            if !connected.is_empty() {
                lines.push(Line::from(Span::styled("", Style::default())));
                lines.push(Line::from(Span::styled(
                    "Most connected:",
                    Style::default().fg(theme.text_dim).add_modifier(Modifier::BOLD),
                )));
                for entry in connected {
                    let name = entry.pointer("/name").and_then(Value::as_str).unwrap_or("");
                    let degree = entry.pointer("/degree").and_then(Value::as_u64).unwrap_or(0);
                    let etype = entry.pointer("/entity_type").and_then(Value::as_str).unwrap_or("");
                    if !name.is_empty() {
                        lines.push(Line::from(vec![
                            Span::styled("  • ", theme.primary),
                            Span::styled(name, Style::default().fg(theme.text).add_modifier(Modifier::BOLD)),
                            Span::styled(format!(" ({}", etype), theme.text_dim),
                            Span::styled(format!(", deg={})", degree), Style::default().fg(theme.text_bright)),
                        ]));
                    }
                }
            }
        }
    }

    frame.render_widget(
        Paragraph::new(lines).block(block).wrap(Wrap { trim: false }),
        area,
    );
}
