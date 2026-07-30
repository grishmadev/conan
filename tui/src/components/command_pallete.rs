use ratatui::{
    Frame,
    layout::Constraint,
    symbols::border,
    text::Line,
    widgets::{Block, Borders, Clear},
};

use crate::App;

pub trait CommandPallete {
    fn render_command_palette(&self, f: &mut Frame<'_>, text: &str, cursor_pos: &usize);
}

impl CommandPallete for App {
    fn render_command_palette(&self, f: &mut Frame<'_>, text: &str, cursor_pos: &usize) {
        let area = f.area();
        let width = 60;
        let block = Block::new()
            .title_top(" Command ")
            .borders(Borders::ALL)
            .border_set(border::ROUNDED);
        let line = Line::from(text);
        let block_area = area
            .centered_vertically(Constraint::Max(3))
            .centered_horizontally(Constraint::Length(width));
        let line_area = block.inner(block_area);
        #[allow(clippy::cast_possible_truncation)]
        let cposx = *cursor_pos as u16 + line_area.x;
        f.render_widget(Clear, block_area);
        f.set_cursor_position((cposx, line_area.y));
        f.render_widget(block, block_area);
        f.render_widget(line, line_area);
    }
}
