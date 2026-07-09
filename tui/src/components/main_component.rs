use ratatui::{
    Frame,
    layout::{HorizontalAlignment, Rect},
    style::{Color, Style},
    symbols::border,
    text::Line,
    widgets::{Block, Borders, List, ListItem, ListState},
};

use crate::App;

pub trait MainComponents {
    fn render_chats(&self, f: &mut Frame<'_>, selected: bool, area: Rect);
    fn render_contact_list(
        &self,
        f: &mut Frame<'_>,
        list: Vec<String>,
        idx: usize,
        area: Rect,
        selected: bool,
    );
}

impl MainComponents for App {
    fn render_chats(&self, f: &mut Frame<'_>, selected: bool, area: Rect) {
        let chat_style = if selected {
            Style::new().light_blue()
        } else {
            Style::default()
        };
        let chat_block = Block::new()
            .borders(Borders::ALL)
            .border_set(border::ROUNDED)
            .title_top(Line::from(" Chat ").alignment(HorizontalAlignment::Center))
            .style(chat_style);
        f.render_widget(chat_block, area);
    }
    fn render_contact_list(
        &self,
        f: &mut Frame<'_>,
        list: Vec<String>,
        idx: usize,
        area: Rect,
        selected: bool,
    ) {
        let left_block = Block::new()
            .borders(Borders::ALL)
            .border_set(border::ROUNDED)
            .title_top(" Contact ");

        let contact_style = if selected {
            Style::new().light_blue()
        } else {
            Style::default()
        };
        let list_items = list
            .iter()
            .map(|i| ListItem::new(i.clone()).style(Style::default()))
            .collect::<Vec<_>>();
        let contact_list = List::new(list_items)
            .block(left_block)
            .style(contact_style)
            .highlight_style(Style::new().bg(Color::LightBlue));

        let mut state = ListState::default();
        state.select(Some(idx));

        f.render_stateful_widget(contact_list, area, &mut state);
    }
}
