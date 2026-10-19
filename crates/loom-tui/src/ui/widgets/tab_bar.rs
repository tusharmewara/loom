use crate::app::App;
use crate::theme::Theme;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};
use ratatui::Frame;

/// Shortcut character for each tab position, matching the global key
/// handling (`1`..`9`, `0`, `!`, `@`, `#`). The last two tabs have no
/// direct shortcut and are reached with Tab/→.
fn tab_key_label(index: usize) -> char {
    match index {
        0..=8 => char::from(b'1' + index as u8),
        9 => '0',
        10 => '!',
        11 => '@',
        12 => '#',
        _ => ' ',
    }
}

/// Indices of the tab window rendered on the bar: [start, end).
/// All tabs when they fit; otherwise scroll just enough that the
/// active tab stays visible on narrow terminals.
fn visible_window(widths: &[u16], active: usize, avail: u16) -> (usize, usize) {
    let len = widths.len();
    let total: u16 = widths.iter().sum();
    if total <= avail || len == 0 {
        return (0, len);
    }

    // Reserve one column per truncation indicator so it always fits.
    let budget = avail.saturating_sub(2);

    // Walk left from the active tab while the window still fits.
    let mut start = active.min(len - 1);
    let mut used = widths[start];
    while start > 0 && used + widths[start - 1] <= budget {
        used += widths[start - 1];
        start -= 1;
    }

    // The window must always contain the active tab.
    let mut end = active + 1;
    while end < len && used + widths[end] <= budget {
        used += widths[end];
        end += 1;
    }
    (start, end)
}

pub fn draw_tab_bar(frame: &mut Frame, area: Rect, app: &App, theme: &Theme) {
    let widths: Vec<u16> = app
        .tabs
        .iter()
        .enumerate()
        .map(|(i, t)| tab_key_label(i).len_utf8() as u16 + t.len() as u16 + 2)
        .collect();

    let avail = area.width.saturating_sub(2); // borders
    let (start, end) = visible_window(&widths, app.tab_index, avail);

    let mut spans: Vec<Span> = Vec::new();
    if start > 0 {
        spans.push(Span::styled("…", Style::default().fg(theme.text_dim)));
    }

    for i in start..end {
        let is_active = i == app.tab_index;
        let style = if is_active {
            Style::default()
                .fg(theme.tab_active)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(theme.tab_inactive)
        };
        spans.push(Span::styled(
            format!("{} {} ", tab_key_label(i), app.tabs[i]),
            style,
        ));
    }

    if end < app.tabs.len() {
        spans.push(Span::styled("…", Style::default().fg(theme.text_dim)));
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tab_key_labels_match_the_global_digit_shortcuts() {
        let labels: String = (0..15).map(tab_key_label).collect();
        assert_eq!(labels, "1234567890!@#  ");
    }

    #[test]
    fn window_shows_everything_when_it_fits() {
        let widths = vec![10, 10, 10];
        assert_eq!(visible_window(&widths, 2, 40), (0, 3));
    }

    #[test]
    fn window_keeps_the_active_tab_visible_when_narrow() {
        let widths = vec![10, 10, 10, 10, 10];
        // Budget 12 (avail 14 - 2 indicators) fits one 10-wide tab:
        // exactly the active one.
        assert_eq!(visible_window(&widths, 3, 14), (3, 4));
        // Active at either edge never collapses out of view.
        assert_eq!(visible_window(&widths, 0, 14), (0, 1));
        assert_eq!(visible_window(&widths, 4, 14), (4, 5));
    }

    #[test]
    fn window_extends_right_after_scrolling_left() {
        let widths = vec![6, 6, 6, 6, 6, 6];
        // avail 15 -> budget 13: tabs 1 and 2 (8 each incl. padding
        // would fit, 6-wide fits two) -> window [1, 3).
        assert_eq!(visible_window(&widths, 2, 15), (1, 3));
    }
}
