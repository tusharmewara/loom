mod loom;
pub use loom::*;
mod catppuccin;
pub use catppuccin::*;

use ratatui::style::Color;

#[derive(Clone, Copy, Debug)]
pub struct Theme {
    pub primary: Color,
    pub secondary: Color,
    pub accent: Color,
    pub success: Color,
    pub warning: Color,
    pub error: Color,
    pub info: Color,
    pub text: Color,
    pub text_dim: Color,
    pub text_bright: Color,
    pub background: Color,
    pub surface: Color,
    pub border: Color,
    pub border_focused: Color,
    pub selection_bg: Color,
    pub selection_fg: Color,
    pub scrollbar_thumb: Color,
    pub scrollbar_track: Color,
    pub tab_active: Color,
    pub tab_inactive: Color,
    pub tab_divider: Color,
    pub header_bg: Color,
}

impl Theme {
    pub fn from_name(name: &str) -> Theme {
        match name {
            "catppuccin" => catppuccin_theme(),
            _ => loom_theme(),
        }
    }
}

pub struct ThemeManager {
    themes: Vec<&'static str>,
    current: usize,
}

impl Default for ThemeManager {
    fn default() -> Self {
        Self::new()
    }
}

impl ThemeManager {
    pub fn new() -> Self {
        Self {
            themes: vec!["loom", "catppuccin"],
            current: 0,
        }
    }

    pub fn current(&self) -> Theme {
        Theme::from_name(self.themes[self.current])
    }

    pub fn cycle_next(&mut self) -> Theme {
        self.current = (self.current + 1) % self.themes.len();
        self.current()
    }

    pub fn cycle_prev(&mut self) -> Theme {
        if self.current == 0 {
            self.current = self.themes.len() - 1;
        } else {
            self.current -= 1;
        }
        self.current()
    }

    pub fn current_name(&self) -> &'static str {
        self.themes[self.current]
    }

    pub fn names(&self) -> &[&'static str] {
        &self.themes
    }

    pub fn set(&mut self, name: &str) -> Option<Theme> {
        if let Some(pos) = self.themes.iter().position(|t| *t == name) {
            self.current = pos;
            Some(self.current())
        } else {
            None
        }
    }
}
