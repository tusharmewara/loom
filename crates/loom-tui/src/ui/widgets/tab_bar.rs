use crate::app::App;
use crate::theme::Theme;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};
use ratatui::Frame;

pub fn draw_tab_bar(frame: &mut Frame, area: ratatui::layout::Rect, app: &App, theme: &Theme) {
    let mut spans: Vec<Span> = Vec::new();

    for (i, tab) in app.tabs.iter().enumerate() {
        if i > 0 {
            spans.push(Span::styled(" │ ", Style::default().fg(theme.tab_divider)));
        }

        let is_active = i == app.tab_index;

        let style = if is_active {
            Style::default()
                .fg(theme.tab_active)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(theme.tab_inactive)
        };

        let prefix = if is_active { "●" } else { "○" };

        spans.push(Span::styled(prefix.to_string(), style));
        spans.push(Span::styled(format!(" {} ", tab), style));
    }

    frame.render_widget(
        Paragraph::new(Line::from(spans)).block(
            Block::default()
                .borders(Borders::BOTTOM)
                .border_style(Style::default().fg(theme.border)),
        ),
        area,
    );
}
