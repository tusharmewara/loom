use crate::theme::Theme;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::widgets::Widget;

pub struct Scrollbar {
    pub offset: usize,
    pub content_len: usize,
    pub visible_lines: usize,
    pub theme: Theme,
}

impl Widget for Scrollbar {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if self.content_len <= self.visible_lines || self.visible_lines == 0 {
            return;
        }
        let track_height = area.height as usize;
        let thumb_size = (track_height * self.visible_lines / self.content_len).max(1);
        let thumb_pos = if self.content_len > self.visible_lines {
            (track_height - thumb_size) * self.offset / (self.content_len - self.visible_lines)
        } else {
            0
        };

        for y in 0..track_height {
            let cell = &mut buf[(area.x, area.y + y as u16)];
            if y >= thumb_pos && y < thumb_pos + thumb_size {
                cell.set_symbol("▓");
                cell.set_style(Style::default().fg(self.theme.scrollbar_thumb));
            } else {
                cell.set_symbol("░");
                cell.set_style(Style::default().fg(self.theme.scrollbar_track));
            }
        }
    }
}
