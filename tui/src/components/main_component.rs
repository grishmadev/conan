use conanprotocol::msg::Mode;
use ratatui::{
    Frame,
    layout::{HorizontalAlignment, Rect},
    style::{Color, Style},
    symbols::border,
    text::Line,
    widgets::{Block, Borders, List, ListDirection, ListItem},
};

use crate::App;

pub trait MainComponents {
    fn render_chats(&mut self, f: &mut Frame<'_>, selected: bool, area: Rect);
    fn render_chat_bar(&mut self, f: &mut Frame<'_>, selected: bool, area: Rect);
    fn render_contact_list(&mut self, f: &mut Frame<'_>, area: Rect, selected: bool);
}

impl MainComponents for App {
    fn render_chats(&mut self, f: &mut Frame<'_>, selected: bool, area: Rect) {
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
        if self.contact_idx.selected().is_none() {
            let line = Line::from("Select Contact to Start Chatting...").centered();
            let line_area = chat_block.inner(area);
            f.render_widget(line, line_area);
        }
        let chats = self
            .chats
            .iter()
            .map(|c| {
                let sub = if c.sender_id == 1 { "you" } else { "they" };
                format!("{} - {}", sub, c.data)
            })
            .collect::<Vec<_>>();
        let chats = List::new(chats).direction(ListDirection::BottomToTop);
        let chats_area = chat_block.inner(area);
        f.render_widget(chats, chats_area);
        f.render_widget(chat_block, area);
    }

    fn render_chat_bar(&mut self, f: &mut Frame<'_>, selected: bool, area: Rect) {
        let text = Line::from(self.chat_buf.clone())
            .left_aligned()
            .style(Style::default());
        let chat_style = if selected {
            Style::new().light_blue()
        } else {
            Style::default()
        };
        let bar_block = Block::new()
            .borders(Borders::ALL)
            .border_set(border::ROUNDED)
            .style(chat_style);

        let line_area = bar_block.inner(area);
        if let Mode::Insert { cursor_pos } = self.mode {
            #[allow(clippy::cast_possible_truncation)]
            let cpos = cursor_pos as u16 + line_area.x;
            f.set_cursor_position((cpos, line_area.y));
        }
        f.render_widget(text, line_area);
        f.render_widget(bar_block, area);
    }

    fn render_contact_list(&mut self, f: &mut Frame<'_>, area: Rect, selected: bool) {
        let left_block = Block::new()
            .borders(Borders::ALL)
            .border_set(border::ROUNDED)
            .title_top(" Contact ");

        let contact_style = if selected {
            Style::new().light_blue()
        } else {
            Style::default()
        };
        let list_items = self
            .contacts
            .iter()
            .map(|i| {
                ListItem::new(i.name.clone()).style(if i.connected {
                    Style::new().green()
                } else {
                    Style::new().white()
                })
            })
            .collect::<Vec<_>>();
        let contact_list = List::new(list_items)
            .block(left_block)
            .style(contact_style)
            .highlight_style(Style::default().bg(Color::LightBlue));

        f.render_stateful_widget(contact_list, area, &mut self.contact_idx);
    }
}
