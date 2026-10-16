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

    let registered_count = app
        .skills
        .pointer("/count")
        .and_then(Value::as_u64)
        .unwrap_or(0);
    let detected_count = app
        .status
        .pointer("/resources/counts/skills")
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

    // Show registered skills first
    if let Some(skills) = app.skills.pointer("/skills").and_then(Value::as_array) {
        if !skills.is_empty() {
            lines.push(Line::from(Span::styled("", Style::default())));
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
                        Span::styled(
                            name,
                            Style::default().fg(theme.text).add_modifier(Modifier::BOLD),
                        ),
                        Span::styled(format!(" ({})", source), theme.text_dim),
                    ]));
                }
            }
        }
    }

    // Show detected skills from scan when none registered
    if registered_count == 0 {
        if let Some(sources) = app
            .scan
            .pointer("/system_audit/skill_sources")
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
                        .pointer("/skill_count")
                        .and_then(Value::as_u64)
                        .unwrap_or(0);
                    let path = s.pointer("/path").and_then(Value::as_str).unwrap_or("");
                    lines.push(Line::from(vec![
                        Span::styled("  • ", theme.primary),
                        Span::styled(source, theme.text),
                        Span::styled(format!(" ({} skills)", count), theme.text_dim),
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
                    "Use 'loom skills install <path>' to import skills into loom.",
                    theme.text_dim,
                )));
            } else {
                lines.push(Line::from(Span::styled("", Style::default())));
                lines.push(Line::from(Span::styled(
                    "No skills in daemon registry.",
                    theme.text_dim,
                )));
                lines.push(Line::from(Span::styled(
                    "Check ~/.loom/skills/ for local skills.",
                    theme.text_dim,
                )));
            }
        } else {
            lines.push(Line::from(Span::styled("", Style::default())));
            lines.push(Line::from(Span::styled(
                "No skills in daemon registry.",
                theme.text_dim,
            )));
            lines.push(Line::from(Span::styled(
                "Check ~/.loom/skills/ for local skills.",
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
