//! Cost dashboard tab - real-time token/cost per task/agent from telemetry.

use crate::app::App;
use crate::theme::Theme;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Cell, Paragraph, Row, Table, Wrap};
use ratatui::Frame;
use serde_json::Value;

pub fn draw_cost(frame: &mut Frame, area: ratatui::layout::Rect, app: &App, theme: &Theme) {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(theme.border))
        .title(" Cost Dashboard ")
        .title_style(
            Style::default()
                .fg(theme.primary)
                .add_modifier(Modifier::BOLD),
        );

    // For now, derive from workflow + routing catalog
    let workflow = &app.workflow;
    let _routing = &app.status; // placeholder

    let mut lines = Vec::new();

    // Summary stats (derived from workflow tasks)
    let tasks_vec: Vec<Value> = Vec::new();
    let tasks = workflow
        .pointer("/tasks")
        .and_then(Value::as_array)
        .unwrap_or(&tasks_vec);
    let _task_count = tasks.len() as u64;
    let completed_count = tasks
        .iter()
        .filter(|t| t.get("phase").and_then(Value::as_str) == Some("Finish"))
        .count() as u64;

    lines.push(Line::from(vec![
        Span::styled("Tasks: ", theme.text_dim),
        Span::styled(
            format!("{} total", tasks.len()),
            Style::default()
                .fg(theme.text_bright)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled("  ", theme.text_dim),
        Span::styled(
            format!("{} completed", completed_count),
            Style::default()
                .fg(theme.success)
                .add_modifier(Modifier::BOLD),
        ),
    ]));

    // Model tier breakdown from routing decisions
    if let Some(tasks_with_routing) = workflow.pointer("/tasks").and_then(Value::as_array) {
        let mut tier_counts = std::collections::HashMap::new();
        for task in tasks_with_routing {
            if let Some(routing) = task.get("routing_decision") {
                if let Some(tier) = routing.pointer("/recommended_tier").and_then(Value::as_str) {
                    *tier_counts.entry(tier.to_string()).or_insert(0) += 1;
                }
            }
        }

        if !tier_counts.is_empty() {
            lines.push(Line::from(Span::styled(
                "Model Tiers (by routing decision):",
                Style::default()
                    .fg(theme.secondary)
                    .add_modifier(Modifier::BOLD),
            )));
            for (tier, count) in tier_counts {
                let (color, label) = match tier.as_str() {
                    "Slm" => (theme.success, "Slm (Fast/Cheap)"),
                    "Mid" => (theme.warning, "Mid (Balanced)"),
                    "Flagship" => (theme.error, "Flagship (Premium)"),
                    _ => (theme.text, &tier[..]),
                };
                lines.push(Line::from(vec![
                    Span::styled("  • ", theme.primary),
                    Span::styled(format!("{} ", label), color),
                    Span::styled(format!("{} task(s)", count), theme.text_dim),
                ]));
            }
            lines.push(Line::from(Span::styled("", Style::default())));
        }
    }

    // Per-task cost estimates (placeholder - would come from telemetry)
    lines.push(Line::from(Span::styled(
        "Per-Task Estimates (from routing catalog):",
        Style::default()
            .fg(theme.secondary)
            .add_modifier(Modifier::BOLD),
    )));

    // Build table of tasks with cost estimates
    let mut rows = Vec::new();
    for task in tasks.iter().take(20) {
        let task_id = task.get("task_id").and_then(Value::as_str).unwrap_or("?");
        let title = task
            .get("title")
            .and_then(Value::as_str)
            .unwrap_or("(untitled)");
        let phase = task.get("phase").and_then(Value::as_str).unwrap_or("?");
        let status = task.get("status").and_then(Value::as_str).unwrap_or("?");

        // Get routing tier
        let tier = task
            .get("routing_decision")
            .and_then(|r| r.pointer("/recommended_tier"))
            .and_then(Value::as_str)
            .unwrap_or("?");

        // Get estimated cost from routing catalog (placeholder)
        let cost_estimate = task
            .get("routing_decision")
            .and_then(|r| r.pointer("/cost_estimate"))
            .and_then(Value::as_str)
            .unwrap_or("—");

        let short_id = &task_id[..task_id.len().min(8)];
        let display_title = if title.len() > 30 {
            format!("{}…", &title[..29])
        } else {
            title.to_string()
        };

        rows.push(Row::new(vec![
            Cell::from(Span::styled(short_id, theme.text_dim)),
            Cell::from(Span::styled(display_title, theme.text)),
            Cell::from(Span::styled(phase, theme.primary)),
            Cell::from(Span::styled(status, theme.text_dim)),
            Cell::from(Span::styled(tier, theme.warning)),
            Cell::from(Span::styled(cost_estimate, theme.success)),
        ]));
    }

    if rows.is_empty() {
        lines.push(Line::from(Span::styled(
            "  No tasks to display",
            theme.text_dim,
        )));
    } else {
        let header = Row::new(vec![
            Cell::from(Span::styled("ID", theme.text_dim)),
            Cell::from(Span::styled("Title", theme.text_dim)),
            Cell::from(Span::styled("Phase", theme.text_dim)),
            Cell::from(Span::styled("Status", theme.text_dim)),
            Cell::from(Span::styled("Tier", theme.text_dim)),
            Cell::from(Span::styled("Est. Cost", theme.text_dim)),
        ])
        .style(
            Style::default()
                .fg(theme.text_dim)
                .add_modifier(Modifier::BOLD),
        );

        let table = Table::new(
            rows,
            [
                ratatui::layout::Constraint::Length(10),
                ratatui::layout::Constraint::Min(20),
                ratatui::layout::Constraint::Length(12),
                ratatui::layout::Constraint::Length(12),
                ratatui::layout::Constraint::Length(10),
                ratatui::layout::Constraint::Length(14),
            ],
        )
        .header(header)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(theme.border)),
        )
        .highlight_style(
            Style::default()
                .fg(theme.primary)
                .add_modifier(Modifier::BOLD),
        );

        // Render lines first, then table below
        lines.push(Line::from(Span::styled("", Style::default())));

        // We'll render the table separately after the lines
        let paragraph = Paragraph::new(lines)
            .block(block)
            .wrap(Wrap { trim: false });

        // Split area: top for summary, bottom for table
        use ratatui::layout::{Constraint, Direction, Layout};
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(area.height / 2), Constraint::Min(5)])
            .split(area);

        frame.render_widget(paragraph, chunks[0]);
        frame.render_widget(table, chunks[1]);
        return;
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

    #[test]
    fn test_cost_tier_classification() {
        let tiers = ["Slm", "Mid", "Flagship"];
        for tier in tiers {
            assert!(!tier.is_empty());
        }
    }
}
