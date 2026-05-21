use crate::app::App;
use crate::theme::Theme;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};
use ratatui::Frame;

use super::centered_rect;

#[derive(Clone, Debug)]
pub struct Command {
    pub name: &'static str,
    pub description: &'static str,
    pub shortcut: Option<char>,
}

pub fn all_commands() -> Vec<Command> {
    vec![
        Command { name: "daemon start", description: "Start loomd daemon", shortcut: None },
        Command { name: "daemon stop", description: "Stop loomd daemon", shortcut: None },
        Command { name: "daemon restart", description: "Restart loomd daemon", shortcut: None },
        Command { name: "daemon install-service", description: "Install as user service", shortcut: None },
        Command { name: "daemon uninstall-service", description: "Uninstall user service", shortcut: None },
        Command { name: "theme next", description: "Switch to next theme", shortcut: Some(']') },
        Command { name: "theme prev", description: "Switch to previous theme", shortcut: Some('[') },
        Command { name: "sidebar toggle", description: "Show/hide sidebar", shortcut: Some('S') },
        Command { name: "tab 1", description: "Go to Dashboard", shortcut: Some('1') },
        Command { name: "tab 2", description: "Go to Daemon", shortcut: Some('2') },
        Command { name: "tab 3", description: "Go to Discovery", shortcut: Some('3') },
        Command { name: "tab 4", description: "Go to MCP", shortcut: Some('4') },
        Command { name: "tab 5", description: "Go to Skills", shortcut: Some('5') },
        Command { name: "tab 6", description: "Go to Sessions", shortcut: Some('6') },
        Command { name: "tab 7", description: "Go to Logs", shortcut: Some('7') },
        Command { name: "refresh", description: "Refresh all data", shortcut: Some('r') },
        Command { name: "scan", description: "Run ecosystem scan", shortcut: Some('s') },
        Command { name: "doctor", description: "Run doctor checks", shortcut: Some('d') },
        Command { name: "help", description: "Toggle help overlay", shortcut: Some('?') },
        Command { name: "quit", description: "Quit TUI", shortcut: Some('q') },
    ]
}

pub fn fuzzy_match(query: &str, name: &str) -> bool {
    if query.is_empty() {
        return true;
    }
    let query_lower = query.to_lowercase();
    let name_lower = name.to_lowercase();

    if name_lower.contains(&query_lower) {
        return true;
    }

    let mut query_chars = query_lower.chars().peekable();
    for c in name_lower.chars() {
        if query_chars.peek() == Some(&c) {
            query_chars.next();
        }
        if query_chars.peek().is_none() {
            return true;
        }
    }
    false
}

pub fn draw_command_palette(
    frame: &mut Frame,
    area: Rect,
    app: &App,
    theme: &Theme,
    query: &str,
) {
    let popup_area = centered_rect(55, 60, area);
    frame.render_widget(
        ratatui::widgets::Clear,
        popup_area,
    );

    let all = all_commands();
    let filtered: Vec<&Command> = all
        .iter()
        .filter(|cmd| fuzzy_match(query, cmd.name))
        .collect();

    let selected = app.command_palette_selected.min(filtered.len().saturating_sub(1));

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(theme.border_focused))
        .title(format!(" Command Palette: {} ", if query.is_empty() { "type to search...".to_string() } else { format!("\"{}\"", query) }))
        .title_style(
            Style::default()
                .fg(theme.primary)
                .add_modifier(Modifier::BOLD),
        );

    let mut lines: Vec<Line> = Vec::new();

    lines.push(Line::from(Span::styled(
        format!("  {} ", query),
        Style::default().fg(theme.text).add_modifier(Modifier::REVERSED),
    )));

    lines.push(Line::from(Span::styled("", Style::default())));

    for (i, cmd) in filtered.iter().enumerate() {
        let is_selected = i == selected;
        let style = if is_selected {
            Style::default()
                .fg(theme.selection_fg)
                .bg(theme.selection_bg)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(theme.text)
        };

        let prefix = if is_selected { "▸ " } else { "  " };
        let shortcut_str = cmd
            .shortcut
            .map(|c| format!("[{}]", c))
            .unwrap_or_default();

        lines.push(Line::from(vec![
            Span::styled(prefix, style),
            Span::styled(cmd.name, style),
            Span::styled(
                format!("  {} ", cmd.description),
                if is_selected {
                    Style::default().fg(theme.selection_fg)
                } else {
                    Style::default().fg(theme.text_dim)
                },
            ),
            Span::styled(shortcut_str, theme.text_dim),
        ]));
    }

    if filtered.is_empty() {
        lines.push(Line::from(Span::styled(
            "  No matching commands",
            theme.text_dim,
        )));
    }

    lines.push(Line::from(Span::styled(
        "",
        Style::default(),
    )));
    lines.push(Line::from(vec![
        Span::styled("  ↑↓ navigate ", theme.text_dim),
        Span::styled("· ", theme.text_dim),
        Span::styled("Enter execute ", theme.text_dim),
        Span::styled("· ", theme.text_dim),
        Span::styled("Esc close", theme.text_dim),
    ]));

    frame.render_widget(
        Paragraph::new(lines).block(block).wrap(Wrap { trim: false }),
        popup_area,
    );
}