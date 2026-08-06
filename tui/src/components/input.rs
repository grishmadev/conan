use ratatui::{
    Frame,
    layout::{Constraint, HorizontalAlignment},
    style::Style,
    symbols::border,
    text::Line,
    widgets::{Block, Borders, Clear},
};

use crate::App;

pub trait InputScreen {
    fn render_input_block(&self, f: &mut Frame<'_>, input: &str, prompt: &str, cursor_pos: &usize);
}

impl InputScreen for App {
    fn render_input_block(&self, f: &mut Frame<'_>, input: &str, prompt: &str, cursor_pos: &usize) {
        let area = f.area();
        let text = Line::from(input)
            .alignment(HorizontalAlignment::Left)
            .style(Style::new().light_blue());

        let block = Block::new()
            .borders(Borders::ALL)
            .border_set(border::ROUNDED)
            .title_top(prompt)
            .title_bottom(Line::from(" OK<enter> ").right_aligned())
            .title_bottom(Line::from(" Cancel<esc> ").left_aligned());

        let area = area
            .centered_horizontally(Constraint::Length(90))
            .centered_vertically(Constraint::Length(3));

        let line_area = block.inner(area);
        #[allow(clippy::cast_possible_truncation)]
        let cposx = *cursor_pos as u16 + line_area.x;
        f.render_widget(Clear, area);
        f.set_cursor_position((cposx, line_area.y));
        f.render_widget(block, area);
        f.render_widget(text, line_area);
    }
}
