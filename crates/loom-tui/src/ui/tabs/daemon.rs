use crate::app::App;
use crate::theme::Theme;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};
use ratatui::Frame;
use serde_json::Value;

pub fn draw_daemon(frame: &mut Frame, area: ratatui::layout::Rect, app: &App, theme: &Theme) {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(theme.border))
        .title(" Daemon ")
        .title_style(
            Style::default()
                .fg(theme.primary)
                .add_modifier(Modifier::BOLD),
        );

    let mut lines: Vec<Line> = Vec::new();

    let running = app.daemon.pointer("/running").and_then(Value::as_bool);
    let status = app.daemon.pointer("/status").and_then(Value::as_str);
    let socket = app.daemon.pointer("/socket_path").and_then(Value::as_str);
    let error = app.daemon.pointer("/error").and_then(Value::as_str);

    let is_running = running.unwrap_or(false);
    lines.push(Line::from(vec![
        Span::styled(
            "Status: ",
            Style::default().fg(theme.text).add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            if is_running {
                "● Running"
            } else {
                "○ Stopped"
            },
            Style::default()
                .fg(if is_running {
                    theme.success
                } else {
                    theme.error
                })
                .add_modifier(Modifier::BOLD),
        ),
    ]));
    if let Some(s) = status {
        lines.push(Line::from(vec![
            Span::styled("Detail: ", theme.text_dim),
            Span::styled(s, theme.text),
        ]));
    }
    if let Some(s) = socket {
        lines.push(Line::from(vec![
            Span::styled("Socket: ", theme.text_dim),
            Span::styled(s, theme.text),
        ]));
    }
    lines.push(Line::from(vec![
        Span::styled("Error: ", theme.text_dim),
        Span::styled(error.unwrap_or("none"), theme.text),
    ]));

    if let Some(d) = app.daemon.pointer("/data") {
        if let Some(version) = d.pointer("/version").and_then(Value::as_str) {
            lines.push(Line::from(vec![
                Span::styled("Version: ", theme.text_dim),
                Span::styled(version, theme.text_bright),
            ]));
        }

        if let Some(subsystems) = d.pointer("/subsystems") {
            lines.push(Line::from(Span::styled("", Style::default())));
            lines.push(Line::from(Span::styled(
                "Subsystems",
                Style::default()
                    .fg(theme.secondary)
                    .add_modifier(Modifier::BOLD),
            )));
            if let Some(obj) = subsystems.as_object() {
                for (k, v) in obj {
                    let ok = v.as_bool().unwrap_or(false);
                    lines.push(Line::from(vec![
                        Span::styled(
                            if ok { "  ✓ " } else { "  ✗ " },
                            Style::default().fg(if ok { theme.success } else { theme.error }),
                        ),
                        Span::styled(k, theme.text),
                    ]));
                }
            }
        }
    }

    if let Some(result) = &app.last_action_result {
        lines.push(Line::from(Span::styled("", Style::default())));
        lines.push(Line::from(vec![
            Span::styled("Last action: ", theme.text_dim),
            Span::styled(
                format!("[{}] {}", result.action, result.message),
                if result.success {
                    Style::default().fg(theme.success)
                } else {
                    Style::default().fg(theme.error)
                },
            ),
        ]));
    }

    frame.render_widget(
        Paragraph::new(lines)
            .block(block)
            .wrap(Wrap { trim: false }),
        area,
    );
}
