use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};

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

#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Screen {
    PeerInputScreen {
        input: String,
        cursor_pos: usize,
    },
    LoadingScreen {
        loading_text: String,
    },
    ConfirmScreen {
        prompt: String,
        options: Vec<String>,
        idx: usize,
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
