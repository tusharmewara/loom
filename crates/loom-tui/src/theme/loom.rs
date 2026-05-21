use ratatui::style::Color;

use super::Theme;

pub fn loom_theme() -> Theme {
    Theme {
        primary: Color::Cyan,
        secondary: Color::Blue,
        accent: Color::Magenta,
        success: Color::Green,
        warning: Color::Yellow,
        error: Color::Red,
        info: Color::White,
        text: Color::White,
        text_dim: Color::DarkGray,
        text_bright: Color::Cyan,
        background: Color::Reset,
        surface: Color::Reset,
        border: Color::DarkGray,
        border_focused: Color::Cyan,
        selection_bg: Color::Cyan,
        selection_fg: Color::Black,
        scrollbar_thumb: Color::Gray,
        scrollbar_track: Color::DarkGray,
        tab_active: Color::Cyan,
        tab_inactive: Color::DarkGray,
        tab_divider: Color::DarkGray,
        header_bg: Color::Reset,
    }
}
