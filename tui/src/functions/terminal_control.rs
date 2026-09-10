use crate::{App, Mode, matches::Tab};

pub trait TerminalControl {
    fn next_tab(&mut self);
    fn next_idx(&mut self);
    fn prev_idx(&mut self);
}

impl TerminalControl for App {
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
                self.mode = Mode::Normal;
            }
        }
    }
    fn next_idx(&mut self) {
        let c_len = self.contacts.len();
        let g_len = self.groups.len();

        if c_len == 0 && g_len == 0 {
            return;
        }

        let next_idx = match self.contact_idx.selected() {
            Some(idx) => {
                if g_len == 0 {
                    if idx + 1 < c_len { idx + 1 } else { 0 }
                } else if c_len == 0 {
                    if idx < g_len { idx + 1 } else { 1 }
                } else {
                    if idx < c_len - 1 {
                        // Move down contacts
                        idx + 1
                    } else if idx == c_len - 1 {
                        // Skip Group header at `c_len`
                        c_len + 1
                    } else if idx < c_len + g_len {
                        // Move down groups
                        idx + 1
                    } else {
                        // Wrap back to top contact
                        0
                    }
                }
            }
            None => 0,
        };
        self.contact_idx.select(Some(next_idx));
    }

    fn prev_idx(&mut self) {
        let c_len = self.contacts.len();
        let g_len = self.groups.len();

        if c_len == 0 && g_len == 0 {
            return;
        }

        let prev_idx = match self.contact_idx.selected() {
            Some(idx) => {
                if g_len == 0 {
                    if idx > 0 {
                        idx - 1
                    } else {
                        c_len.saturating_sub(1)
                    }
                } else if c_len == 0 {
                    if idx > 1 { idx - 1 } else { g_len }
                } else {
                    if idx == 0 {
                        // Wrap to bottom group
                        c_len + g_len
                    } else if idx == c_len + 1 {
                        // Skip Group header moving up
                        c_len - 1
                    } else {
                        // Move up normally
                        idx - 1
                    }
                }
            }
            None => 0,
        };
        self.contact_idx.select(Some(prev_idx));
    }
}
