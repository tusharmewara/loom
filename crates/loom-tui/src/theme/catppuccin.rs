use ratatui::style::Color;

use super::Theme;

pub fn catppuccin_theme() -> Theme {
    Theme {
        primary: Color::Rgb(137, 180, 250),
        secondary: Color::Rgb(166, 227, 161),
        accent: Color::Rgb(243, 139, 168),
        success: Color::Rgb(166, 227, 161),
        warning: Color::Rgb(249, 226, 175),
        error: Color::Rgb(210, 15, 57),
        info: Color::Rgb(186, 194, 222),
        text: Color::Rgb(186, 194, 222),
        text_dim: Color::Rgb(108, 112, 134),
        text_bright: Color::Rgb(205, 214, 244),
        background: Color::Rgb(30, 30, 46),
        surface: Color::Rgb(49, 50, 68),
        border: Color::Rgb(69, 71, 90),
        border_focused: Color::Rgb(137, 180, 250),
        selection_bg: Color::Rgb(137, 180, 250),
        selection_fg: Color::Rgb(30, 30, 46),
        scrollbar_thumb: Color::Rgb(108, 112, 134),
        scrollbar_track: Color::Rgb(69, 71, 90),
        tab_active: Color::Rgb(137, 180, 250),
        tab_inactive: Color::Rgb(108, 112, 134),
        tab_divider: Color::Rgb(69, 71, 90),
        header_bg: Color::Rgb(49, 50, 68),
    }
}
