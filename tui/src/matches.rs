use crate::functions::{ConfirmMode, InputMode, LoadingMode};
use crossterm::event::KeyEvent;

#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum Tab {
    Contact,
    Chat,
    None,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Mode {
    Normal,
    Insert { cursor_pos: usize },
}

#[non_exhaustive]
#[derive(Debug, Clone, Copy)]
pub enum TuiCommand {
    Quit,
    Other(KeyEvent),
}

pub enum PaletteCommand {
    AddToGroup(String),
    NewGroup(Option<String>),
}

impl PaletteCommand {
    /// # Errors
    pub fn try_from(value: &str) -> Result<Self, Box<dyn std::error::Error>> {
        let mut values = value.split(' ');
        let Some(cmd) = values.next() else {
            return Err("No Command found.".into());
        };
        let val = match cmd {
            "newgroup" => {
                let name = values.next().map(Into::into);
                PaletteCommand::NewGroup(name)
            }
            "addtogroup" => {
                let Some(name) = values.next() else {
                    return Err("Did not get group name".into());
                };
                PaletteCommand::AddToGroup(name.to_string())
            }
            _ => {
                return Err("No such Enum".into());
            }
        };
        Ok(val)
    }
}

#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Screen {
    InputScreen {
        input: String,
        cursor_pos: usize,
        prompt: String,
        mode: InputMode,
    },
    LoadingScreen {
        loading_text: String,
        mode: LoadingMode,
    },
    ConfirmScreen {
        prompt: String,
        yes_selected: bool,
        mode: ConfirmMode,
    },
    CommandPalette {
        text: String,
        cursor_pos: usize,
        options: Vec<String>,
    },
    GroupDescription(u16),
    None,
}
