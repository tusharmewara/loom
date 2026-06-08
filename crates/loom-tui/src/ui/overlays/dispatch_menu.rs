use crate::app::App;
use crate::theme::Theme;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, List, ListItem};
use ratatui::Frame;
use serde_json::Value;

use super::centered_rect;

pub fn draw_dispatch_menu(frame: &mut Frame, area: Rect, app: &App, theme: &Theme) {
    let popup_area = centered_rect(50, 40, area);
    frame.render_widget(Clear, popup_area);

    let task = app.selected_task();
    let task_title = task
        .and_then(|t| t.pointer("/title").and_then(Value::as_str))
        .unwrap_or("Unknown task");
    let task_id = task
        .and_then(|t| t.pointer("/task_id").and_then(Value::as_str))
        .unwrap_or("?");

    let dispatchers: [(&str, char, bool); 3] = [
        ("Terminal Launch", 't', true),
        ("ACP Dispatch", 'a', app.status.pointer("/daemon/reachable").and_then(|v| v.as_bool()).unwrap_or(false)),
        ("Embedded Pane", 'e', true),
    ];

    let items: Vec<ListItem> = dispatchers
        .iter()
        .enumerate()
        .map(|(i, (name, shortcut, available))| {
            let is_selected = i == app.dispatch_menu_selected;
            let style = if is_selected {
                Style::default()
                    .fg(theme.selection_fg)
                    .bg(theme.selection_bg)
                    .add_modifier(Modifier::BOLD)
            } else if !available {
                Style::default().fg(theme.text_dim)
            } else {
                Style::default().fg(theme.text)
            };
            let prefix = if is_selected { "▸ " } else { "  " };
            let avail_text = if *available {
                format!("[{}]", shortcut)
            } else {
                "not available".to_string()
            };
            ListItem::new(Line::from(Span::styled(
                format!("{}{}  {}", prefix, name, avail_text),
                style,
            )))
        })
        .collect();

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(theme.border_focused))
        .title(format!(
            " Dispatch: {} ({}) ",
            task_title,
            &task_id[..8.min(task_id.len())]
        ))
        .title_style(
            Style::default()
                .fg(theme.primary)
                .add_modifier(Modifier::BOLD),
        );

    let list = List::new(items).block(block).highlight_style(
        Style::default()
            .fg(theme.selection_fg)
            .bg(theme.selection_bg)
            .add_modifier(Modifier::BOLD),
    );

    frame.render_widget(list, popup_area);
}
