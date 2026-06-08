use crate::app::App;
use crate::theme::Theme;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
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

    let popup_area = centered_rect(80, 75, area);
    frame.render_widget(Clear, popup_area);

    let inner = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(theme.border_focused))
        .title(format!(
            " Embedded Pane: {} ",
            task.pointer("/task_id").and_then(Value::as_str).unwrap_or("?")
        ))
        .title_style(
            Style::default()
                .fg(theme.primary)
                .add_modifier(Modifier::BOLD),
        );
    let area = inner.inner(popup_area);
    frame.render_widget(inner, popup_area);

    // Split: left (task context 45%) | right (live output 55%)
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(45), Constraint::Percentage(55)])
        .split(area);

    // ── Left: Task context ──────────────────────────────────────
    draw_task_context(frame, chunks[0], task, theme);

    // ── Right: Live output log ───────────────────────────────────
    draw_output_log(frame, chunks[1], app, theme);
}

fn draw_task_context(frame: &mut Frame, area: Rect, task: &Value, theme: &Theme) {
    let task_id = task.pointer("/task_id").and_then(Value::as_str).unwrap_or("?");
    let title = task.pointer("/title").and_then(Value::as_str).unwrap_or("Untitled");
    let description = task.pointer("/description").and_then(Value::as_str).unwrap_or("");
    let phase = task.pointer("/phase").and_then(Value::as_str).unwrap_or("unknown");
    let status = task.pointer("/status").and_then(Value::as_str).unwrap_or("unknown");
    let provider = task.pointer("/provider").and_then(Value::as_str).unwrap_or("");
    let model = task.pointer("/model").and_then(Value::as_str).unwrap_or("");

    let mut lines: Vec<Line> = Vec::new();

    // Header
    lines.push(Line::from(Span::styled(
        format!(" {}", title),
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

    // Progress bar
    if let Some(progress) = task.pointer("/progress").and_then(Value::as_f64) {
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            " Progress:",
            Style::default()
                .fg(theme.secondary)
                .add_modifier(Modifier::UNDERLINED),
        )));
        let pct = (progress * 100.0) as u16;
        let bar_width = 30usize;
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

    let block = Block::default()
        .borders(Borders::RIGHT)
        .title(" Task Context ")
        .title_style(Style::default().fg(theme.secondary));
    let paragraph = Paragraph::new(lines).block(block).wrap(Wrap { trim: false });
    frame.render_widget(paragraph, area);
}

fn draw_output_log(frame: &mut Frame, area: Rect, app: &App, theme: &Theme) {
    let block = Block::default()
        .borders(Borders::LEFT)
        .title(" Output Log ")
        .title_style(Style::default().fg(theme.secondary));

    let mut lines: Vec<Line> = Vec::new();

    if app.embedded_log.is_empty() {
        lines.push(Line::from(Span::styled(
            " No output yet. Dispatch the task to see live output here.\n\n j/k — scroll • Esc — close • r — refresh",
            theme.text_dim,
        )));
    } else {
        let visible_height = area.height.saturating_sub(2) as usize;
        let scroll = app.embedded_log_scroll.min(
            app.embedded_log.len().saturating_sub(visible_height),
        );
        for line in app.embedded_log.iter().skip(scroll).take(visible_height) {
            // Color based on content heuristics
            let style = if line.contains("error") || line.contains("FAIL") {
                Style::default().fg(theme.error)
            } else if line.contains("warning") || line.contains("WARN") {
                Style::default().fg(theme.warning)
            } else if line.contains("✓") || line.contains("ok") {
                Style::default().fg(theme.success)
            } else if line.contains("→") || line.contains("=>") {
                Style::default().fg(theme.primary)
            } else {
                Style::default().fg(theme.text)
            };
            lines.push(Line::from(Span::styled(line, style)));
        }

        // Footer with scroll indicator
        if app.embedded_log.len() > visible_height {
            let total = app.embedded_log.len();
            let at = scroll + visible_height;
            lines.push(Line::from(Span::styled(
                format!(
                    "\n Lines {}-{}/{}  (j/k to scroll)",
                    scroll + 1,
                    at.min(total),
                    total
                ),
                theme.text_dim,
            )));
        }
    }

    let paragraph = Paragraph::new(lines).block(block).wrap(Wrap { trim: false });
    frame.render_widget(paragraph, area);
}
