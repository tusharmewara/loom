use crate::app::App;
use crate::theme::Theme;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};
use ratatui::Frame;
use serde_json::Value;

pub fn draw_skills(frame: &mut Frame, area: ratatui::layout::Rect, app: &App, theme: &Theme) {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(theme.border))
        .title(" Skills ")
        .title_style(
            Style::default()
                .fg(theme.primary)
                .add_modifier(Modifier::BOLD),
        );

    let mut lines: Vec<Line> = Vec::new();

    let count = app.skills.pointer("/count").and_then(Value::as_u64).unwrap_or(0);
    lines.push(Line::from(vec![
        Span::styled("Registered: ", theme.text_dim),
        Span::styled(
            format!("{}", count),
            Style::default().fg(theme.text_bright).add_modifier(Modifier::BOLD),
        ),
    ]));

    if let Some(skills) = app.skills.pointer("/skills").and_then(Value::as_array) {
        if skills.is_empty() {
            lines.push(Line::from(Span::styled("", Style::default())));
            lines.push(Line::from(Span::styled(
                "No skills in daemon registry.",
                theme.text_dim,
            )));
            lines.push(Line::from(Span::styled(
                "Check ~/.loom/skills/ for local skills.",
                theme.text_dim,
            )));
        } else {
            for s in skills {
                lines.push(Line::from(Span::styled("", Style::default())));
                if let Some(name) = s.as_str() {
                    lines.push(Line::from(vec![
                        Span::styled("  • ", theme.primary),
                        Span::styled(name, theme.text),
                    ]));
                } else if let Some(name) = s.pointer("/name").and_then(Value::as_str) {
                    let source = s.pointer("/source").and_then(Value::as_str).unwrap_or("");
                    lines.push(Line::from(vec![
                        Span::styled("  • ", theme.primary),
                        Span::styled(name, Style::default().fg(theme.text).add_modifier(Modifier::BOLD)),
                        Span::styled(format!(" ({})", source), theme.text_dim),
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
