//! Runtime tab - runtime selection and status.

use crate::app::App;
use crate::theme::Theme;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Cell, Paragraph, Row, Table, Wrap};
use ratatui::Frame;
use serde_json::Value;

pub fn draw_runtime(frame: &mut Frame, area: ratatui::layout::Rect, app: &App, theme: &Theme) {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(theme.border))
        .title(" Runtime ")
        .title_style(
            Style::default()
                .fg(theme.primary)
                .add_modifier(Modifier::BOLD),
        );

    let mut lines: Vec<Line> = Vec::new();

    // Get workflow config for current runtime
    let workflow = &app.workflow;
    let current_runtime = workflow
        .pointer("/runtime")
        .and_then(Value::as_str)
        .unwrap_or("process");

    lines.push(Line::from(vec![
        Span::styled(
            "Current Runtime: ",
            Style::default().fg(theme.text).add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            current_runtime.to_string(),
            Style::default()
                .fg(theme.primary)
                .add_modifier(Modifier::BOLD),
        ),
    ]));

    lines.push(Line::from(Span::styled("", Style::default())));

    // Runtime table with all available runtimes
    let rows = vec![
        Row::new(vec![
            Cell::from(Span::styled(
                "process",
                Style::default().fg(if current_runtime == "process" {
                    theme.success
                } else {
                    theme.text
                }),
            )),
            Cell::from(Span::styled("✓", theme.success)),
            Cell::from(Span::styled(
                "Host process execution (default)",
                theme.text_dim,
            )),
        ]),
        Row::new(vec![
            Cell::from(Span::styled(
                "docker",
                Style::default().fg(if current_runtime == "docker" {
                    theme.success
                } else {
                    theme.text
                }),
            )),
            Cell::from(Span::styled(check_docker(), theme.text_dim)),
            Cell::from(Span::styled("Docker container execution", theme.text_dim)),
        ]),
        Row::new(vec![
            Cell::from(Span::styled(
                "podman",
                Style::default().fg(if current_runtime == "podman" {
                    theme.success
                } else {
                    theme.text
                }),
            )),
            Cell::from(Span::styled(check_podman(), theme.text_dim)),
            Cell::from(Span::styled("Podman container execution", theme.text_dim)),
        ]),
        Row::new(vec![
            Cell::from(Span::styled(
                "kubernetes",
                Style::default().fg(if current_runtime == "kubernetes" {
                    theme.success
                } else {
                    theme.text
                }),
            )),
            Cell::from(Span::styled(check_kubernetes(), theme.text_dim)),
            Cell::from(Span::styled(
                "Kubernetes pod execution (kubectl)",
                theme.text_dim,
            )),
        ]),
    ];

    let header = Row::new(vec![
        Cell::from(Span::styled("Runtime", theme.text_dim)),
        Cell::from(Span::styled("Available", theme.text_dim)),
        Cell::from(Span::styled("Description", theme.text_dim)),
    ])
    .style(
        Style::default()
            .fg(theme.text_dim)
            .add_modifier(Modifier::BOLD),
    );

    let table = Table::new(
        rows,
        [
            ratatui::layout::Constraint::Length(12),
            ratatui::layout::Constraint::Length(12),
            ratatui::layout::Constraint::Min(20),
        ],
    )
    .header(header)
    .block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(theme.border)),
    )
    .row_highlight_style(
        Style::default()
            .fg(theme.primary)
            .add_modifier(Modifier::BOLD),
    );

    // Summary lines
    lines.push(Line::from(Span::styled(
        "Available Runtimes:",
        Style::default()
            .fg(theme.secondary)
            .add_modifier(Modifier::BOLD),
    )));

    frame.render_widget(
        Paragraph::new(lines)
            .block(block.clone())
            .wrap(Wrap { trim: false }),
        area,
    );

    // Render table below the summary
    use ratatui::layout::{Constraint, Direction, Layout};
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(area.height / 2), Constraint::Min(5)])
        .margin(1)
        .split(area);

    frame.render_widget(table, chunks[1]);
}

fn check_docker() -> &'static str {
    if std::process::Command::new("docker")
        .arg("version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
    {
        "✓ Available"
    } else {
        "○ Unavailable"
    }
}

fn check_podman() -> &'static str {
    if std::process::Command::new("podman")
        .arg("version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
    {
        "✓ Available"
    } else {
        "○ Unavailable"
    }
}

fn check_kubernetes() -> &'static str {
    if std::process::Command::new("kubectl")
        .arg("version")
        .arg("--client")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
    {
        "✓ Available"
    } else {
        "○ Unavailable"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_docker_check() {
        let result = check_docker();
        assert!(result == "✓ Available" || result == "○ Unavailable");
    }

    #[test]
    fn test_podman_check() {
        let result = check_podman();
        assert!(result == "✓ Available" || result == "○ Unavailable");
    }

    #[test]
    fn test_kubernetes_check() {
        let result = check_kubernetes();
        assert!(result == "✓ Available" || result == "○ Unavailable");
    }
}
