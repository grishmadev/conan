use ratatui::{
    Frame,
    layout::{Constraint, HorizontalAlignment},
    style::Stylize,
    text::Line,
    widgets::{Block, Borders, Cell, Clear, Row, Table},
};

use crate::App;

pub trait GroupDescription {
    fn render_group_description(&self, f: &mut Frame<'_>, idx: u16);
}

impl GroupDescription for App {
    fn render_group_description(&self, f: &mut Frame<'_>, idx: u16) {
        let area = f.area();
        let box_area = area
            .centered_horizontally(Constraint::Percentage(80))
            .centered_vertically(Constraint::Percentage(80));
        let title = self.groups.get(idx as usize).map_or_else(
            || " Group Description ".to_string(),
            |g| format!(" {} ", g.name),
        );
        let block = Block::default().title(title).borders(Borders::ALL);
        let table_area = block.inner(box_area);

        let Some(members) = self.group_members.as_ref() else {
            return;
        };
        f.render_widget(Clear, box_area);
        f.render_widget(block, box_area);
        let mut rows = Vec::new();
        if members.is_empty() {
            let empty_screen = Line::from("No members in this Group")
                .centered()
                .alignment(HorizontalAlignment::Center);
            f.render_widget(empty_screen, table_area);
        } else {
            for (member, is_admin) in members {
                let privilege = if *is_admin {
                    Cell::from("Admin").green().bold()
                } else {
                    Cell::from("Member").cyan()
                };
                rows.push(Row::new(vec![Cell::from(member.name.clone()), privilege]));
            }
            let table = Table::default()
                .header(Row::new(vec![Cell::from("Name"), Cell::from("Privilege")]))
                .rows(rows)
                .widths([Constraint::Percentage(50), Constraint::Percentage(50)]);
            f.render_widget(table, table_area);
        }
    }
}
