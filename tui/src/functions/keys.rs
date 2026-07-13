use std::{iter::Cycle, time::Duration};

use conanprotocol::{comm::enums::IPCCmd, msg::Mode};
use crossterm::event::{self, Event, KeyCode, KeyModifiers};

use crate::{
    App,
    functions::terminal_control::TerminalControl,
    matches::{Screen, Tab},
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
                Screen::None => match key.code {
                    KeyCode::Tab => {
                        self.next_tab();
                    }
                    KeyCode::Char(ch) if matches!(self.mode, Mode::Insert { .. }) => {
                        if let Mode::Insert { ref mut cursor_pos } = self.mode {
                            self.chat_buf.insert(*cursor_pos, ch);
                            *cursor_pos += 1;
                        }
                    }
                    KeyCode::Char('a') => {
                        self.active_screen = Screen::PeerInputScreen {
                            input: String::new(),
                            cursor_pos: 0,
                        }
                    }
                    KeyCode::Char('i') => {
                        if self.mode == Mode::Normal {
                            self.mode = Mode::Insert { cursor_pos: 0 };
                        }
                    }
                    KeyCode::Char('j') | KeyCode::Down => {
                        if self.tab == Tab::Contact {
                            if let Some(idx) = self.contact_idx.selected()
                                && idx == self.contacts.len() - 1
                            {
                                self.contact_idx.select_first();
                            } else {
                                self.contact_idx.select_next();
                            }
                        }
                    }
                    KeyCode::Char('k') | KeyCode::Up => {
                        if self.tab == Tab::Contact {
                            if let Some(idx) = self.contact_idx.selected()
                                && idx == 0
                            {
                                self.contact_idx.select_last();
                            } else {
                                self.contact_idx.select_previous();
                            }
                        }
                    }
                    KeyCode::Char('q') => {
                        self.running = false;
                    }
                    // KeyCode::Backspace if key.modifiers == KeyModifiers::CONTROL => {
                    //     if let Mode::Insert { ref mut cursor_pos } = self.mode {
                    //         while *cursor_pos > 0 && self.chat_buf.remove(*cursor_pos) != ' ' {
                    //             *cursor_pos -= 1;
                    //         }
                    //     }
                    // }
                    KeyCode::Backspace => {
                        if let Mode::Insert { ref mut cursor_pos } = self.mode {
                            if *cursor_pos > 0 {
                                *cursor_pos -= 1;
                                self.chat_buf.remove(*cursor_pos);
                            }
                        }
                    }
                    KeyCode::Delete => {
                        if let Mode::Insert { ref cursor_pos } = self.mode {
                            if (0..self.chat_buf.len()).contains(cursor_pos) {
                                self.chat_buf.remove(*cursor_pos);
                            }
                        }
                    }
                    KeyCode::Enter => {
                        let id = if let Some(idx) = self.contact_idx.selected()
                            && let Some(target) = self.contacts.get(idx)
                        {
                            Some((target.id, target.address.clone()))
                        } else {
                            None
                        };
                        let Some((id, addr)) = id else {
                            return Ok(());
                        };
                        match self.tab {
                            Tab::Contact => {
                                self.active_screen = Screen::LoadingScreen {
                                    loading_text: "Connecting...".into(),
                                };
                                self.send(IPCCmd::Connect(addr, 80)).await?;
                                self.send(IPCCmd::ChatList {
                                    #[allow(clippy::cast_possible_truncation)]
                                    peer_id: id as u8,
                                    msg_amount: 50,
                                })
                                .await?;
                            }
                            Tab::Chat => match self.mode {
                                Mode::Normal => {
                                    #[allow(clippy::cast_possible_truncation)]
                                    self.send(IPCCmd::Text(id as u8, self.chat_buf.trim().into()))
                                        .await?;
                                    self.chat_buf = String::new();
                                }
                                Mode::Insert { ref mut cursor_pos } => {
                                    self.chat_buf.insert(*cursor_pos, '\n');
                                    *cursor_pos += 1;
                                }
                            },
                            Tab::None => {}
                        }
                    }
                    KeyCode::Left => {
                        if let Mode::Insert { ref mut cursor_pos } = self.mode {
                            if *cursor_pos > 0 {
                                *cursor_pos -= 1;
                            }
                        }
                    }
                    KeyCode::Right => {
                        if let Mode::Insert { ref mut cursor_pos } = self.mode {
                            if self.chat_buf.len() > *cursor_pos {
                                *cursor_pos += 1;
                            }
                        }
                    }
                    KeyCode::Esc => {
                        self.mode = Mode::Normal;
                    }
                    _ => {}
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
