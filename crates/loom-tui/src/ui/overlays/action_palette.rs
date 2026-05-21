use crate::app::App;
use crate::theme::Theme;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, List, ListItem};
use ratatui::Frame;

use super::centered_rect;

pub fn draw_action_palette(frame: &mut Frame, area: Rect, app: &App, theme: &Theme) {
    let popup_area = centered_rect(50, 40, area);
    frame.render_widget(Clear, popup_area);

    let items: Vec<ListItem> = app
        .daemon_actions
        .iter()
        .enumerate()
        .map(|(i, action)| {
            let is_selected = i == app.action_palette_selected;
            let style = if is_selected {
                Style::default()
                    .fg(theme.selection_fg)
                    .bg(theme.selection_bg)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(theme.text)
            };
            let prefix = if is_selected { "▸ " } else { "  " };
            ListItem::new(Line::from(Span::styled(
                format!("{}daemon {}", prefix, action),
                style,
            )))
        })
        .collect();

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(theme.border_focused))
        .title(" Daemon Actions ")
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
