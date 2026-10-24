use conandatabase::entities::peer::Peer;
use conanprotocol::comm::enums::group_actions::GroupOptions;
use ratatui::{
    Frame,
    layout::{Constraint, HorizontalAlignment},
    style::Stylize,
    text::Line,
    widgets::{Block, Borders, Cell, Clear, List, ListState, Row, Table, TableState},
};

use crate::App;

pub trait GroupDescription {
    fn render_group_description(
        &self,
        f: &mut Frame<'_>,
        group_idx: &u16,
        options: &[GroupOptions],
        selected_option: &Option<u16>,
        members: &[(Peer, bool)],
        selected_member: &Option<u16>,
    );
}

impl GroupDescription for App {
    fn render_group_description(
        &self,
        f: &mut Frame<'_>,
        group_idx: &u16,
        options: &[GroupOptions],
        selected_option: &Option<u16>,
        members: &[(Peer, bool)],
        selected_member: &Option<u16>,
    ) {
        let area = f.area();
        let box_area = area
            .centered_horizontally(Constraint::Percentage(80))
            .centered_vertically(Constraint::Percentage(80));
        let title = self.groups.get(*group_idx as usize).map_or_else(
            || " Group Description ".to_string(),
            |g| format!(" {} ", g.name),
        );
        let block = Block::default().title(title).borders(Borders::ALL);
        let table_area = block.inner(box_area);

        f.render_widget(Clear, box_area);
        f.render_widget(block, box_area);
        if members.is_empty() {
            let empty_screen = Line::from("No members in this Group")
                .centered()
                .alignment(HorizontalAlignment::Center);
            f.render_widget(empty_screen, table_area);
        } else {
            let mut rows = Vec::new();
            for (member, is_admin) in members {
                let privilege = if *is_admin {
                    Cell::from("Admin").green().bold()
                } else {
                    Cell::from("Member").cyan()
                };
                rows.push(Row::new(vec![Cell::from(member.name.clone()), privilege]));
            }
            let mut table_state = TableState::default();
            table_state.select(selected_member.map(|f| f as usize));
            let table = Table::default()
                .header(Row::new(vec![Cell::from("Name"), Cell::from("Privilege")]))
                .rows(rows)
                .widths([Constraint::Percentage(50), Constraint::Percentage(50)])
                .row_highlight_style(ratatui::style::Style::default().reversed())
                .highlight_symbol(">> ");
            f.render_stateful_widget(table, table_area, &mut table_state);

            if let Some(selected) = selected_option {
                #[allow(clippy::cast_possible_truncation)]
                let list_len = options.len() as u16 + 2;
                let popup_area = area
                    .centered_horizontally(Constraint::Percentage(40))
                    .centered_vertically(Constraint::Length(list_len));
                f.render_widget(Clear, popup_area);
                let popup_block = Block::default().title(" Options ").borders(Borders::ALL);
                let popup_inner = popup_block.inner(popup_area);
                f.render_widget(popup_block, popup_area);

                let mut list_state = ListState::default();
                // let opt_sel = self.group_option_idx.min(options.len().saturating_sub(1));
                list_state.select(Some(*selected as usize));
                let list = List::new(options.iter().map(ToString::to_string))
                    .highlight_style(ratatui::style::Style::default().reversed())
                    .highlight_symbol(">> ");
                f.render_stateful_widget(list, popup_inner, &mut list_state);
            }
        }
    }
}
