use std::time::Duration;

use conanprotocol::comm::enums::IPCCmd;
use crossterm::event::{self, Event, KeyCode};

use crate::{
    App, TerminalControl,
    matches::{Screen, TuiCommand, get_tuicmd},
};

pub trait Keys {
    fn manage_keys(&mut self) -> impl Future<Output = std::io::Result<()>>;
}

impl Keys for App {
    async fn manage_keys(&mut self) -> std::io::Result<()> {
        if event::poll(Duration::from_millis(100))?
            && let Event::Key(key) = event::read()?
        {
            match self.active_screen {
                Screen::None => match get_tuicmd(key) {
                    TuiCommand::Quit => {
                        self.running = false;
                    }
                    TuiCommand::Other(key) => match key.code {
                        KeyCode::Tab => {
                            self.next_tab();
                        }
                        KeyCode::Char('a') => {
                            self.active_screen = Screen::PeerInputScreen {
                                input: String::new(),
                                cursor_pos: 0,
                            }
                        }
                        _ => {}
                    },
                },
                Screen::PeerInputScreen {
                    ref mut input,
                    ref mut cursor_pos,
                } => match key.code {
                    KeyCode::Char(ch) => {
                        input.insert(*cursor_pos, ch);
                        self.active_screen = Screen::PeerInputScreen {
                            input: input.clone(),
                            cursor_pos: *cursor_pos + 1,
                        }
                    }
                    KeyCode::Backspace => {
                        if *cursor_pos > 0 {
                            *cursor_pos -= 1;
                            input.remove(*cursor_pos);
                        }
                        self.active_screen = Screen::PeerInputScreen {
                            input: input.clone(),
                            cursor_pos: *cursor_pos,
                        }
                    }
                    KeyCode::Delete => {
                        if (0..input.len()).contains(cursor_pos) {
                            input.remove(*cursor_pos);
                        }
                        self.active_screen = Screen::PeerInputScreen {
                            input: input.clone(),
                            cursor_pos: *cursor_pos,
                        }
                    }
                    KeyCode::Left => {
                        if *cursor_pos > 0 {
                            *cursor_pos -= 1;
                        }
                    }
                    KeyCode::Right => {
                        if input.len() > *cursor_pos {
                            *cursor_pos += 1;
                        }
                    }
                    KeyCode::Enter => {
                        let msg = IPCCmd::Connect(input.clone(), 80);
                        self.send(msg).await?;
                        self.active_screen = Screen::LoadingScreen {
                            loading_text: "Adding peer...".to_string(),
                        };
                    }
                    KeyCode::Esc => {
                        self.active_screen = Screen::None;
                    }
                    _ => {}
                },
                Screen::LoadingScreen { .. } => {}
                Screen::ConfirmScreen {
                    ref options,
                    ref mut idx,
                    ..
                } => match key.code {
                    KeyCode::Left => {
                        if *idx == 0 {
                            *idx = options.len() - 1;
                        } else {
                            *idx -= 1;
                        }
                    }
                    KeyCode::Right => {
                        if *idx == options.len() - 1 {
                            *idx = 0;
                        } else {
                            *idx += 1;
                        }
                    }
                    _ => {}
                },
            }
        }
        Ok(())
    }
}
