use crate::app::App;
use crate::theme::Theme;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph, Wrap};
use ratatui::Frame;
use serde_json::Value;

use super::centered_rect;

pub fn draw_embedded_pane(frame: &mut Frame, area: Rect, app: &App, theme: &Theme) {
    let task = match &app.embedded_task {
        Some(t) => t,
        None => return,
    };

    let popup_area = centered_rect(75, 70, area);
    frame.render_widget(Clear, popup_area);

    let task_id = task.pointer("/task_id").and_then(Value::as_str).unwrap_or("?");
    let title = task.pointer("/title").and_then(Value::as_str).unwrap_or("Untitled");
    let description = task.pointer("/description").and_then(Value::as_str).unwrap_or("");
    let phase = task.pointer("/phase").and_then(Value::as_str).unwrap_or("unknown");
    let status = task.pointer("/status").and_then(Value::as_str).unwrap_or("unknown");
    let provider = task.pointer("/provider").and_then(Value::as_str).unwrap_or("");
    let model = task.pointer("/model").and_then(Value::as_str).unwrap_or("");

    let mut lines = Vec::new();

    // Task header
    lines.push(Line::from(Span::styled(
        format!(" {} ", title),
        Style::default()
            .fg(theme.primary)
            .add_modifier(Modifier::BOLD),
    )));
    lines.push(Line::from(Span::styled(
        format!(" ID: {}", task_id),
        Style::default().fg(theme.text_dim),
    )));

    // Phase + Status
    let status_style = match status {
        "completed" | "active" => Style::default().fg(theme.success),
        "failed" | "error" => Style::default().fg(theme.error),
        _ => Style::default().fg(theme.text),
    };
    lines.push(Line::from(Span::styled(
        format!(" Phase: {}  Status: {}", phase, status),
        status_style,
    )));

    // Provider info
    if !provider.is_empty() || !model.is_empty() {
        lines.push(Line::from(Span::styled(
            format!(" Provider: {}  Model: {}", provider, model),
            Style::default().fg(theme.text_dim),
        )));
    }

    // Description
    if !description.is_empty() {
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            " Description:",
            Style::default()
                .fg(theme.secondary)
                .add_modifier(Modifier::UNDERLINED),
        )));
        // Word-wrap description
        for chunk in description.split('\n') {
            lines.push(Line::from(Span::styled(
                chunk,
                Style::default().fg(theme.text),
            )));
        }
    }

    // Agent info
    if let Some(agent_id) = task.pointer("/agent_id").and_then(Value::as_str) {
        if !agent_id.is_empty() {
            lines.push(Line::from(""));
            lines.push(Line::from(Span::styled(
                format!(" Assigned to: {}", agent_id),
                Style::default().fg(theme.text),
            )));
        }
    }

    // Steps
    if let Some(steps) = task.pointer("/steps").and_then(Value::as_array) {
        if !steps.is_empty() {
            lines.push(Line::from(""));
            lines.push(Line::from(Span::styled(
                format!(" Steps ({}):", steps.len()),
                Style::default()
                    .fg(theme.secondary)
                    .add_modifier(Modifier::UNDERLINED),
            )));
            for (i, step) in steps.iter().enumerate() {
                let step_name = step.pointer("/name").and_then(Value::as_str).unwrap_or("");
                let step_status = step.pointer("/status").and_then(Value::as_str).unwrap_or("");
                let marker = match step_status {
                    "completed" => "✓",
                    "in_progress" | "running" => "→",
                    "failed" => "✗",
                    _ => "·",
                };
                let step_style = match step_status {
                    "completed" => Style::default().fg(theme.success),
                    "failed" => Style::default().fg(theme.error),
                    "in_progress" | "running" => Style::default().fg(theme.primary),
                    _ => Style::default().fg(theme.text_dim),
                };
                lines.push(Line::from(Span::styled(
                    format!("   {}  {}. {}", marker, i + 1, step_name),
                    step_style,
                )));
            }
        }
    }

    // Progress bar if phases are available
    if let Some(progress) = task.pointer("/progress").and_then(Value::as_f64) {
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            " Progress:",
            Style::default()
                .fg(theme.secondary)
                .add_modifier(Modifier::UNDERLINED),
        )));
        let pct = (progress * 100.0) as u16;
        let bar_width = 40usize;
        let filled = ((pct as f64 / 100.0) * bar_width as f64) as usize;
        let empty = bar_width.saturating_sub(filled);
        let bar = format!(
            " [{}{}] {}%",
            "█".repeat(filled),
            "░".repeat(empty),
            pct
        );
        lines.push(Line::from(Span::styled(bar, Style::default().fg(theme.primary))));
    }

    // Footer hint
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        " Esc to close | r to refresh ",
        Style::default().fg(theme.text_dim),
    )));

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(theme.border_focused))
        .title(format!(" Embedded Pane: {} ", task_id))
        .title_style(
            Style::default()
                .fg(theme.primary)
                .add_modifier(Modifier::BOLD),
        );

    let paragraph = Paragraph::new(lines)
        .block(block)
        .wrap(Wrap { trim: false });

    frame.render_widget(paragraph, popup_area);
}
