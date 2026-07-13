use crate::{App, matches::Tab};

pub trait TerminalControl {
    fn next_tab(&mut self);
}

impl TerminalControl for App {
    fn next_tab(&mut self) {
        let new_tab = match self.tab {
            Tab::Contact => Tab::Chat,
            _ => Tab::Contact,
        };
        self.tab = new_tab;
    }
}
