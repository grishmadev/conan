use ratatui::{
    Frame,
    layout::{Constraint, HorizontalAlignment, Rect},
    style::Style,
    symbols::{border, line},
    text::Line,
    widgets::{Block, Borders, Widget},
};

pub fn render_new_peer_block(f: &mut Frame<'_>, input: &str, area: Rect) {
    let input = if input.is_empty() {
        "Enter peer's Address"
    } else {
        input
    };
    let text = Line::from(input)
        .alignment(HorizontalAlignment::Left)
        .style(Style::new().light_blue().rapid_blink());
    let block = Block::new()
        .borders(Borders::ALL)
        .border_set(border::ROUNDED)
        .title_top(" Peer Address ");
    let area = area
        .centered_horizontally(Constraint::Length(60))
        .centered_vertically(Constraint::Length(3));
    let line_area = block.inner(area);
    let cposx = line_area.x + input.len() as u16;
    f.set_cursor_position((cposx, line_area.y));
    f.render_widget(block, area);
    f.render_widget(text, line_area);
}
