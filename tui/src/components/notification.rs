use ratatui::{
    Frame,
    layout::Rect,
    symbols::border,
    text::Line,
    widgets::{Block, Borders, Clear},
};

use crate::App;

pub trait Notification {
    fn render_notification(&self, f: &mut Frame<'_>, text: &str);
}

impl Notification for App {
    fn render_notification(&self, f: &mut Frame<'_>, text: &str) {
        let area = f.area();
        #[allow(clippy::cast_possible_truncation)]
        let notif_width = if text.len() < 30 { 30 } else { text.len() } as u16;
        let notif_area = Rect::new(
            area.right() - 10 - notif_width,
            area.top() + 1,
            notif_width + 20,
            5,
        );
        let block = Block::new()
            .border_set(border::ROUNDED)
            .borders(Borders::ALL);
        let line = Line::from(text).centered();
        let line_area = block.inner(notif_area);
        f.render_widget(Clear, notif_area);
        f.render_widget(block, notif_area);
        f.render_widget(line, line_area);
    }
}
