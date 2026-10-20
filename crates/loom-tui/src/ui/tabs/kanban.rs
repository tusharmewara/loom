//! Kanban board tab - visual task board with columns for each workflow phase.

use crate::app::{App, PHASES};
use crate::theme::Theme;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};
use ratatui::Frame;
use serde_json::Value;

const PHASE_COLORS: &[fn(&Theme) -> Color] =
    &[|t| t.primary, |t| t.warning, |t| t.secondary, |t| t.success];

pub fn draw_kanban(frame: &mut Frame, area: ratatui::layout::Rect, app: &App, theme: &Theme) {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(theme.border))
        .title(" Kanban ")
        .title_style(
            Style::default()
                .fg(theme.primary)
                .add_modifier(Modifier::BOLD),
        );
    frame.render_widget(block, area);

    // Calculate column width
    let col_width = (area.width.saturating_sub(4)) / PHASES.len() as u16;
    let inner_width = col_width.saturating_sub(4);

    let mut columns = Vec::new();

    // Build task lists per phase
    let tasks_vec: Vec<Value> = Vec::new();
    let tasks = app
        .workflow
        .pointer("/tasks")
        .and_then(Value::as_array)
        .unwrap_or(&tasks_vec);
    let selected_id = app.kanban_selected_task_id();

    for (i, phase_name) in PHASES.iter().enumerate() {
        let phase_tasks: Vec<&Value> = tasks
            .iter()
            .filter(|t| t.get("phase").and_then(Value::as_str) == Some(*phase_name))
            .collect();

        let phase_color = PHASE_COLORS[i](theme);
        let mut lines = Vec::new();

        // Phase header
        lines.push(Line::from(vec![Span::styled(
            format!(" {} ({}) ", phase_name, phase_tasks.len()),
            Style::default()
                .fg(phase_color)
                .add_modifier(Modifier::BOLD),
        )]));

        // Separator
        lines.push(Line::from(Span::styled(
            "─".repeat(inner_width as usize),
            theme.text_dim,
        )));

        if phase_tasks.is_empty() {
            lines.push(Line::from(Span::styled("  (no tasks)", theme.text_dim)));
        } else {
            for task in phase_tasks {
                let title = task
                    .get("title")
                    .and_then(Value::as_str)
                    .unwrap_or("(untitled)");
                let task_id = task.get("task_id").and_then(Value::as_str).unwrap_or("");
                let short_id = &task_id[..task_id.len().min(8)];
                let status = task.get("status").and_then(Value::as_str).unwrap_or("?");
                let worktree = task
                    .get("worktree_path")
                    .and_then(Value::as_str)
                    .map(|_| "🌿")
                    .unwrap_or("");
                let selected = selected_id.as_deref() == Some(task_id);

                // Truncate title to fit column (0-safe: inner_width can
                // drop below the marker budget on narrow terminals).
                let max_title_len = inner_width.saturating_sub(15) as usize;
                let display_title = if title.len() > max_title_len {
                    format!("{}…", &title[..max_title_len.saturating_sub(1)])
                } else {
                    title.to_string()
                };

                let marker = if selected { "▸ " } else { "• " };
                let title_style = if selected {
                    Style::default()
                        .fg(theme.selection_fg)
                        .bg(theme.selection_bg)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(theme.text).add_modifier(Modifier::BOLD)
                };
                let id_style = if selected {
                    Style::default()
                        .fg(theme.selection_fg)
                        .bg(theme.selection_bg)
                } else {
                    Style::default().fg(theme.text_dim)
                };

                lines.push(Line::from(vec![
                    Span::styled(format!("  {marker} "), theme.primary),
                    Span::styled(display_title, title_style),
                    Span::styled(format!(" [{}]", status), theme.text_dim),
                    Span::styled(format!(" {}", worktree), theme.success),
                ]));
                lines.push(Line::from(vec![
                    Span::styled("    ", theme.text_dim),
                    Span::styled(short_id, id_style),
                ]));
            }
        }

        let paragraph = Paragraph::new(lines)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(phase_color)),
            )
            .wrap(Wrap { trim: false });

        columns.push((col_width, paragraph));
    }

    // Render columns side by side using a layout
    use ratatui::layout::{Constraint, Direction, Layout};
    let constraints: Vec<Constraint> = (0..PHASES.len())
        .map(|_| Constraint::Percentage(100 / PHASES.len() as u16))
        .collect();

    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints(constraints)
        .margin(1)
        .split(area);

    for (i, (_, paragraph)) in columns.into_iter().enumerate() {
        if i < chunks.len() {
            frame.render_widget(paragraph, chunks[i]);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_phase_filtering() {
        let tasks = json!([
            {"phase": "Plan", "title": "Task 1"},
            {"phase": "Implement", "title": "Task 2"},
            {"phase": "Plan", "title": "Task 3"},
        ]);
        let plan_tasks: Vec<_> = tasks
            .as_array()
            .unwrap()
            .iter()
            .filter(|t| t.get("phase").and_then(Value::as_str) == Some("Plan"))
            .collect();
        assert_eq!(plan_tasks.len(), 2);
    }
}
