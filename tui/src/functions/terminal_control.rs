use crate::{App, Mode, matches::Tab};

pub trait TerminalControl {
    fn next_tab(&mut self);
}

impl TerminalControl for App {
    // fn next_tab(&mut self) {
    //     let new_tab = match self.tab {
    //         Tab::Contact => Tab::Chat,
    //         _ => Tab::Contact,
    //     };
    //     self.tab = new_tab;
    // }
    fn next_tab(&mut self) {
        match self.tab {
            Tab::None | Tab::Contact => {
                self.tab = Tab::Chat;
                self.mode = Mode::Insert {
                    cursor_pos: self.chat_buf.len(),
                };
            }
            Tab::Chat => {
                self.tab = Tab::Contact;
                self.mode = Mode::Normal
            }
        }
    }
}
