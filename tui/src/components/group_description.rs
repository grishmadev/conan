use ratatui::{
    Frame,
    layout::Constraint,
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
        let block = Block::default()
            .title(" Group Description ")
            .borders(Borders::ALL);
        let table_area = block.inner(box_area);
        let (Some(group), Some(members)) =
            (self.groups.get(idx as usize), self.group_members.as_ref())
        else {
            return;
        };
        let mut rows = Vec::with_capacity(members.len());
        for (member, is_admin) in members {
            let privilege = if *is_admin { "Admin" } else { "Member" };
            rows.push(Row::new([member.name.clone(), privilege.into()]));
        }
        let table = Table::default()
            .header(Row::new(vec![Cell::from("Name"), Cell::from("Privilege")]))
            .rows(rows);
        f.render_widget(Clear, box_area);
        f.render_widget(block, box_area);
        f.render_widget(table, table_area);
    }
}
