use crate::app::{format_run_line, App, OrchestratorField};
use crate::theme::Theme;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};
use ratatui::Frame;
use serde_json::Value;

fn field_block<'a>(title: &'a str, focused: bool, theme: &'a Theme) -> Block<'a> {
    Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(if focused {
            theme.border_focused
        } else {
            theme.border
        }))
        .title(format!(" {title} "))
        .title_style(
            Style::default()
                .fg(if focused {
                    theme.primary
                } else {
                    theme.text_dim
                })
                .add_modifier(Modifier::BOLD),
        )
}

fn input_line(value: &str, focused: bool, placeholder: &str, theme: &Theme) -> Line<'static> {
    if value.is_empty() {
        Line::from(Span::styled(
            placeholder.to_string(),
            Style::default().fg(theme.text_dim),
        ))
    } else if focused {
        Line::from(vec![
            Span::styled(value.to_string(), Style::default().fg(theme.text_bright)),
            Span::styled("▌", Style::default().fg(theme.primary)),
        ])
    } else {
        Line::from(Span::styled(
            value.to_string(),
            Style::default().fg(theme.text),
        ))
    }
}

fn status_line(app: &App, theme: &Theme) -> Line<'static> {
    if app.orchestrator_busy {
        return Line::from(Span::styled(
            "Working: creating the task and dispatching it...",
            Style::default()
                .fg(theme.warning)
                .add_modifier(Modifier::BOLD),
        ));
    }

    if let Some(status) = &app.orchestrator_status {
        return Line::from(Span::styled(
            status.message.clone(),
            Style::default()
                .fg(if status.success {
                    theme.success
                } else {
                    theme.error
                })
                .add_modifier(Modifier::BOLD),
        ));
    }

    if !app.orchestrator_can_submit() {
        return Line::from(Span::styled(
            "Enter a title, details, and select one connected ACP tool.",
            Style::default().fg(theme.text_dim),
        ));
    }

    Line::from(Span::styled(
        "Ready: press Enter to create and dispatch this task.",
        Style::default().fg(theme.success),
    ))
}

pub fn draw_orchestrator(frame: &mut Frame, area: Rect, app: &App, theme: &Theme) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(4),
            Constraint::Length(3),
            Constraint::Min(5),
            Constraint::Min(7),
            Constraint::Length(5),
        ])
        .split(area);

    let acp_running = app
        .acp
        .pointer("/running")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let has_default = app
        .acp
        .pointer("/has_default_handler")
        .and_then(Value::as_bool)
        .unwrap_or(false);

    let mut header = vec![Line::from(vec![
        Span::styled(
            "Focused intake: ",
            Style::default().fg(theme.text).add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            "create one workflow task and send it to one connected ACP tool.",
            Style::default().fg(theme.text_dim),
        ),
    ])];
    header.push(Line::from(vec![
        Span::styled("ACP server: ", theme.text_dim),
        Span::styled(
            if acp_running {
                "connected"
            } else {
                "not connected"
            },
            Style::default().fg(if acp_running {
                theme.success
            } else {
                theme.error
            }),
        ),
        Span::styled(
            format!(
                " · {} tool{}",
                app.orchestrator_agents.len(),
                if app.orchestrator_agents.len() == 1 {
                    ""
                } else {
                    "s"
                }
            ),
            Style::default().fg(theme.text),
        ),
        Span::styled(
            format!(
                " · default handler: {}",
                if has_default { "yes" } else { "no" }
            ),
            Style::default().fg(theme.text_dim),
        ),
    ]));
    frame.render_widget(
        Paragraph::new(header)
            .block(field_block("Orchestrator", false, theme))
            .wrap(Wrap { trim: false }),
        chunks[0],
    );

    frame.render_widget(
        Paragraph::new(input_line(
            &app.orchestrator_title,
            app.orchestrator_field == OrchestratorField::Title,
            "Task title, for example: Add dark mode to settings",
            theme,
        ))
        .block(field_block(
            "Title",
            app.orchestrator_field == OrchestratorField::Title,
            theme,
        ))
        .wrap(Wrap { trim: false }),
        chunks[1],
    );

    frame.render_widget(
        Paragraph::new(input_line(
            &app.orchestrator_details,
            app.orchestrator_field == OrchestratorField::Details,
            "Details and acceptance notes, for example: Use theme tokens; update the toggle.",
            theme,
        ))
        .block(field_block(
            "Details",
            app.orchestrator_field == OrchestratorField::Details,
            theme,
        ))
        .wrap(Wrap { trim: true }),
        chunks[2],
    );

    let mut tools: Vec<Line> = Vec::new();
    if app.orchestrator_agents.is_empty() {
        tools.push(Line::from(Span::styled(
            "No connected ACP tools found.",
            Style::default().fg(theme.warning),
        )));
        tools.push(Line::from(Span::styled(
            "Register one with: loom add-acp-agent <agent_id>",
            Style::default().fg(theme.text_dim),
        )));
    } else {
        for (index, agent) in app.orchestrator_agents.iter().enumerate() {
            let selected = index == app.orchestrator_agent_index;
            let focused = app.orchestrator_field == OrchestratorField::Agent;
            let style = if selected && focused {
                Style::default()
                    .fg(theme.selection_fg)
                    .bg(theme.selection_bg)
                    .add_modifier(Modifier::BOLD)
            } else if selected {
                Style::default()
                    .fg(theme.text_bright)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(theme.text)
            };
            tools.push(Line::from(vec![
                Span::styled(if selected { "▸ " } else { "  " }, style),
                Span::styled(agent.clone(), style),
                Span::styled("  [connected]", theme.text_dim),
            ]));
        }
    }
    frame.render_widget(
        Paragraph::new(tools)
            .block(field_block(
                "ACP tool",
                app.orchestrator_field == OrchestratorField::Agent,
                theme,
            ))
            .wrap(Wrap { trim: false }),
        chunks[3],
    );

    let mut footer = vec![status_line(app, theme)];
    if let Some(run) = &app.orchestrator_last_run {
        footer.push(Line::from(vec![
            Span::styled("Last run: ", theme.text_dim),
            Span::styled(format_run_line(run), Style::default().fg(theme.text)),
        ]));
    }
    footer.push(Line::from(vec![
        Span::styled("↑↓ field", theme.text_dim),
        Span::styled(" · ", theme.text_dim),
        Span::styled("←→ select tool", theme.text_dim),
        Span::styled(" · ", theme.text_dim),
        Span::styled("Enter send", theme.text_dim),
        Span::styled(" · ", theme.text_dim),
        Span::styled("Esc clear status", theme.text_dim),
        Span::styled(" · ", theme.text_dim),
        Span::styled("Tab next tab", theme.text_dim),
        Span::styled(" · ", theme.text_dim),
        Span::styled("Ctrl+C quit", theme.text_dim),
    ]));
    frame.render_widget(
        Paragraph::new(footer)
            .block(field_block("Status", false, theme))
            .wrap(Wrap { trim: false }),
        chunks[4],
    );
}
