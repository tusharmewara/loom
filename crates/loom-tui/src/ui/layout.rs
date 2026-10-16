use ratatui::layout::{Constraint, Direction, Layout, Rect};

pub const SIDEBAR_MIN_WIDTH: u16 = 25;
pub const COMPACT_BREAKPOINT: u16 = 120;

pub struct UILayout {
    pub top_bar: Rect,
    pub main_area: Rect,
    pub sidebar: Option<Rect>,
    pub content: Rect,
    pub status_bar: Rect,
}

pub fn compute_layout(area: Rect, show_sidebar: bool) -> UILayout {
    let is_compact = area.width < COMPACT_BREAKPOINT || !show_sidebar;

    let vertical = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Min(1),
            Constraint::Length(1),
        ])
        .split(area);

    let top_bar = vertical[0];
    let status_bar = vertical[2];
    let middle = vertical[1];

    if is_compact {
        UILayout {
            top_bar,
            main_area: middle,
            sidebar: None,
            content: middle,
            status_bar,
        }
    } else {
        let horizontal = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Min(40), Constraint::Length(SIDEBAR_MIN_WIDTH)])
            .split(middle);

        UILayout {
            top_bar,
            main_area: middle,
            sidebar: Some(horizontal[1]),
            content: horizontal[0],
            status_bar,
        }
    }
}
