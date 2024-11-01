use std::cmp::{max, min};

use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Color, Style},
    text::{Line, ToText},
    widgets::{Block, Borders, Widget},
};

#[derive(Debug, Clone)]
pub struct Table {
    y: usize,
    pub data: Vec<String>,
}

impl Table {
    pub fn new(data: Vec<String>) -> Self {
        Self { y: 0, data }
    }

    pub fn scroll_down(&mut self, by: Option<usize>) {
        self.y = max(self.y.saturating_sub(by.unwrap_or(1)), 0);
    }

    pub fn scroll_up(&mut self, by: Option<usize>) {
        self.y = min(self.y.saturating_add(by.unwrap_or(1)), self.data.len() - 1);
    }
}

impl Widget for &Table {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let height = area.height;
        if height < 1 {
            return;
        }

        let innner_height = (height - 2) as usize;
        let starting = min(self.y, self.data.len().saturating_sub(innner_height));
        let messages_to_render = self.data.iter().rev().skip(starting).take(innner_height);
        for (index, message) in messages_to_render.rev().enumerate() {
            buf.set_stringn(
                area.x + 1,
                area.y + index as u16 + 1,
                message.to_string(),
                (area.width - 2) as usize,
                Style::new().bg(if self.y == starting + (innner_height - index) - 1 {
                    Color::LightRed
                } else {
                    Color::Reset
                }),
            );
        }

        Block::new().borders(Borders::ALL).render(area, buf);
    }
}
