//! Graph visualizer tab - knowledge graph entity/relationship visualization.

use crate::app::App;
use crate::theme::Theme;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};
use ratatui::Frame;
use serde_json::Value;

pub fn draw_graph(frame: &mut Frame, area: ratatui::layout::Rect, app: &App, theme: &Theme) {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(theme.border))
        .title(" Knowledge Graph ")
        .title_style(
            Style::default()
                .fg(theme.primary)
                .add_modifier(Modifier::BOLD),
        );

    let stats = &app.knowledge;
    let entity_count = stats
        .pointer("/entity_count")
        .and_then(Value::as_u64)
        .unwrap_or(0);
    let relationship_count = stats
        .pointer("/relationship_count")
        .and_then(Value::as_u64)
        .unwrap_or(0);

    let mut lines = Vec::new();

    // Stats header
    lines.push(Line::from(vec![
        Span::styled("Entities: ", theme.text_dim),
        Span::styled(
            format!("{}", entity_count),
            Style::default()
                .fg(theme.text_bright)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled("  Relationships: ", theme.text_dim),
        Span::styled(
            format!("{}", relationship_count),
            Style::default()
                .fg(theme.text_bright)
                .add_modifier(Modifier::BOLD),
        ),
    ]));

    lines.push(Line::from(Span::styled("", Style::default())));

    // Entity types breakdown
    if let Some(types) = stats.pointer("/entity_types").and_then(Value::as_object) {
        lines.push(Line::from(Span::styled(
            "Entity Types:",
            Style::default()
                .fg(theme.secondary)
                .add_modifier(Modifier::BOLD),
        )));
        for (type_name, count) in types {
            let cnt = count.as_u64().unwrap_or(0);
            lines.push(Line::from(vec![
                Span::styled("  • ", theme.primary),
                Span::styled(type_name, theme.text),
                Span::styled(format!(" ({})", cnt), theme.text_dim),
            ]));
        }
        lines.push(Line::from(Span::styled("", Style::default())));
    }

    // Relationship types breakdown
    if let Some(rel_types) = stats
        .pointer("/relationship_types")
        .and_then(Value::as_object)
    {
        lines.push(Line::from(Span::styled(
            "Relationship Types:",
            Style::default()
                .fg(theme.secondary)
                .add_modifier(Modifier::BOLD),
        )));
        for (type_name, count) in rel_types {
            let cnt = count.as_u64().unwrap_or(0);
            lines.push(Line::from(vec![
                Span::styled("  → ", theme.warning),
                Span::styled(type_name, theme.text),
                Span::styled(format!(" ({})", cnt), theme.text_dim),
            ]));
        }
        lines.push(Line::from(Span::styled("", Style::default())));
    }

    // Sample entities
    if let Some(entities) = stats.pointer("/entities").and_then(Value::as_array) {
        lines.push(Line::from(Span::styled(
            "Sample Entities:",
            Style::default()
                .fg(theme.secondary)
                .add_modifier(Modifier::BOLD),
        )));
        for e in entities.iter().take(20) {
            let name = e.pointer("/name").and_then(Value::as_str).unwrap_or("?");
            let type_name = e.pointer("/type").and_then(Value::as_str).unwrap_or("?");
            let degree = e.pointer("/degree").and_then(Value::as_u64).unwrap_or(0);
            lines.push(Line::from(vec![
                Span::styled("  • ", theme.primary),
                Span::styled(
                    name,
                    Style::default().fg(theme.text).add_modifier(Modifier::BOLD),
                ),
                Span::styled(format!(" ({}) ", type_name), theme.text_dim),
                Span::styled(format!("deg={}", degree), theme.text_dim),
            ]));
        }
        if entities.len() > 20 {
            lines.push(Line::from(Span::styled(
                format!("  … and {} more", entities.len() - 20),
                theme.text_dim,
            )));
        }
    }

    // Sample relationships
    if let Some(relationships) = stats.pointer("/relationships").and_then(Value::as_array) {
        lines.push(Line::from(Span::styled("", Style::default())));
        lines.push(Line::from(Span::styled(
            "Sample Relationships:",
            Style::default()
                .fg(theme.secondary)
                .add_modifier(Modifier::BOLD),
        )));
        for r in relationships.iter().take(15) {
            let from = r.pointer("/from").and_then(Value::as_str).unwrap_or("?");
            let to = r.pointer("/to").and_then(Value::as_str).unwrap_or("?");
            let predicate = r
                .pointer("/predicate")
                .and_then(Value::as_str)
                .unwrap_or("?");
            let weight = r.pointer("/weight").and_then(Value::as_f64).unwrap_or(1.0);
            lines.push(Line::from(vec![
                Span::styled("  ", theme.text_dim),
                Span::styled(from, theme.text),
                Span::styled(" —", theme.warning),
                Span::styled(format!(" {} ", predicate), theme.primary),
                Span::styled("→ ", theme.warning),
                Span::styled(to, theme.text),
                Span::styled(format!(" (w={:.2})", weight), theme.text_dim),
            ]));
        }
        if relationships.len() > 15 {
            lines.push(Line::from(Span::styled(
                format!("  … and {} more", relationships.len() - 15),
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

#[cfg(test)]
mod tests {

    use serde_json::json;

    #[test]
    fn test_stats_extraction() {
        let stats = json!({
            "entity_count": 42,
            "relationship_count": 17,
            "entity_types": {"Task": 10, "Worktree": 5, "PR": 3},
            "relationship_types": {"references": 8, "targets": 4},
        });
        assert_eq!(stats["entity_count"].as_u64(), Some(42));
        assert_eq!(stats["entity_types"]["Task"].as_u64(), Some(10));
    }
}
