use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};

use crate::functions::{ConfirmMode, InputMode, LoadingMode};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Tab {
    Contact,
    Chat,
    None,
}

#[non_exhaustive]
#[derive(Debug, Clone, Copy)]
pub enum TuiCommand {
    Quit,
    Other(KeyEvent),
}

pub enum PaletteCommand {
    AddToGroup,
    NewGroup,
}

impl PaletteCommand {
    pub fn try_from(value: &str) -> Result<Self, Box<dyn std::error::Error>> {
        let val = match value {
            "newgroup" => PaletteCommand::NewGroup,
            "addtogroup" => PaletteCommand::AddToGroup,
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
    None,
}

#[must_use]
pub fn get_key_event(cmd: TuiCommand) -> KeyEvent {
    match cmd {
        TuiCommand::Quit => KeyEvent::new(KeyCode::Char('q'), KeyModifiers::NONE),
        TuiCommand::Other(event) => event,
    }
}

#[must_use]
pub fn get_key(cmd: TuiCommand) -> Event {
    Event::Key(get_key_event(cmd))
}

#[must_use]
pub fn get_tuicmd(key: KeyEvent) -> TuiCommand {
    match key.code {
        KeyCode::Char('q') => TuiCommand::Quit,
        _ => TuiCommand::Other(key),
    }
}
