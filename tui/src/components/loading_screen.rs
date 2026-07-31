use ratatui::{
    Frame,
    layout::Constraint,
    symbols::border,
    text::Line,
    widgets::{Block, Borders, Clear},
};

use crate::App;

pub trait LoadingScreen {
    fn render_loading_screen(&self, f: &mut Frame<'_>, text: &str);
}

impl LoadingScreen for App {
    fn render_loading_screen(&self, f: &mut Frame<'_>, text: &str) {
        let area = f.area();
        let centered_rect = area
            .centered_horizontally(Constraint::Length(30))
            .centered_vertically(Constraint::Length(3));
        let block = Block::new()
            .borders(Borders::ALL)
            .border_set(border::ROUNDED);
        let text = Line::from(text).centered();
        let text_area = block.inner(centered_rect);
        f.render_widget(Clear, centered_rect);
        f.render_widget(block, centered_rect);
        f.render_widget(text, text_area);
    }
}
