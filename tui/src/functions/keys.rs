use std::time::{Duration, Instant};

use conanprotocol::{comm::enums::IPCCmd, entities::database::chat::Chat, msg::Mode};
use crossterm::event::{self, Event, KeyCode};

use crate::{
    App,
    functions::{ConfirmMode, InputMode, LoadingMode, terminal_control::TerminalControl},
    matches::{Screen, Tab},
};

pub trait Keys {
    fn manage_keys(&mut self) -> impl Future<Output = std::io::Result<()>>;
}

impl Keys for App {
    async fn manage_keys(&mut self) -> std::io::Result<()> {
        if event::poll(Duration::from_millis(10))?
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
                    KeyCode::Char('d') if matches!(self.tab, Tab::Contact) => {
                        let Ok(Some(conn)) = self.current_contact() else {
                            return Ok(());
                        };
                        self.active_screen = Screen::ConfirmScreen {
                            prompt: format!("Are you sure you want to delete {}?", conn.name),
                            yes_selected: false,
                            mode: ConfirmMode::DeletePeer,
                        };
                    }
                    KeyCode::Char('r') if matches!(self.tab, Tab::Contact) => {
                        if let Ok(Some(peer)) = self.current_contact()
                            && peer.id == 1
                        {
                            self.notification =
                                Some(("Cannot rename self.".to_string(), Instant::now()));
                            return Ok(());
                        }
                        let Ok(Some(conn)) = self.current_contact() else {
                            return Ok(());
                        };
                        self.active_screen = Screen::InputScreen {
                            input: conn.name.clone(),
                            cursor_pos: conn.name.len(),
                            prompt: " Rename Peer ".into(),
                            mode: InputMode::RenamePeer,
                        };
                    }
                    KeyCode::Char('a') => {
                        self.active_screen = Screen::InputScreen {
                            input: String::new(),
                            cursor_pos: 0,
                            prompt: " New Peer ".to_string(),
                            mode: InputMode::NewPeer,
                        }
                    }
                    KeyCode::Char('i') => {
                        if self.mode == Mode::Normal && self.tab == Tab::Chat {
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
                                if let Some(idx) = self.contact_idx.selected_mut() {
                                    *idx = self.contacts.len() - 1;
                                }
                            } else {
                                self.contact_idx.select_previous();
                            }
                        }
                    }
                    KeyCode::Char('q') => {
                        self.active_screen = Screen::ConfirmScreen {
                            prompt: "Are you sure you want to quit?".into(),
                            yes_selected: false,
                            mode: ConfirmMode::Exit,
                        };
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
                        let target = if let Some(idx) = self.contact_idx.selected()
                            && let Some(target) = self.contacts.get(idx)
                        {
                            Some((target.id, target.address.clone()))
                        } else {
                            None
                        };
                        let Some((id, addr)) = target else {
                            return Ok(());
                        };
                        match self.tab {
                            Tab::Contact => {
                                if self.contact_idx.selected() == Some(0) {
                                    self.notification =
                                        Some(("Cannot connect to self.".into(), Instant::now()));
                                    return Ok(());
                                }
                                self.active_screen = Screen::LoadingScreen {
                                    loading_text: "Connecting...".into(),
                                    mode: LoadingMode::NewPeer,
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
                                    let Some(selected) = self.contact_idx.selected() else {
                                        println!("No chat selected.");
                                        return Ok(());
                                    };
                                    let Some(current_peer) = self.contacts.get(selected) else {
                                        println!("Peer not found.");
                                        return Ok(());
                                    };
                                    let chat = Chat::chat_to_send(&self.chat_buf, current_peer.id);
                                    self.chats.push(chat);
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
                Screen::InputScreen {
                    ref mut input,
                    ref mut cursor_pos,
                    ref mut mode,
                    ..
                } => match key.code {
                    KeyCode::Char(ch) => {
                        input.insert(*cursor_pos, ch);
                        *cursor_pos += 1;
                    }
                    KeyCode::Backspace => {
                        if *cursor_pos > 0 {
                            *cursor_pos -= 1;
                            input.remove(*cursor_pos);
                        }
                    }
                    KeyCode::Delete => {
                        if (0..input.len()).contains(cursor_pos) {
                            input.remove(*cursor_pos);
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
                    KeyCode::Enter => match mode {
                        InputMode::NewPeer => {
                            let msg = IPCCmd::Connect(input.clone(), 80);
                            self.send(msg).await?;
                            self.active_screen = Screen::LoadingScreen {
                                loading_text: "Adding peer...".to_string(),
                                mode: LoadingMode::NewPeer,
                            };
                        }
                        InputMode::RenamePeer => {
                            let Some(idx) = self.contact_idx.selected() else {
                                return Ok(());
                            };
                            let Some(peer) = self.contacts.get(idx) else {
                                return Ok(());
                            };
                            let msg = IPCCmd::RenamePeer(peer.id as u8, input.clone());
                            self.send(msg).await?;
                            self.active_screen = Screen::None;
                        }
                    },
                    KeyCode::Esc => {
                        self.active_screen = Screen::None;
                    }
                    _ => {}
                },
                Screen::LoadingScreen { .. } => {}
                Screen::ConfirmScreen {
                    ref mut yes_selected,
                    ref mut mode,
                    ..
                } => match key.code {
                    KeyCode::Left | KeyCode::Right | KeyCode::Char('l') | KeyCode::Char('h') => {
                        *yes_selected = !*yes_selected;
                    }
                    KeyCode::Enter => match mode {
                        ConfirmMode::Exit => {
                            if *yes_selected {
                                self.running = false;
                            }
                            self.active_screen = Screen::None;
                        }
                        ConfirmMode::DeletePeer => {
                            if *yes_selected {
                                let Some(idx) = self.contact_idx.selected() else {
                                    return Ok(());
                                };
                                let Some(peer) = self.contacts.get(idx) else {
                                    return Ok(());
                                };
                                let cmd = IPCCmd::DeletePeer(peer.id);
                                self.send(cmd).await?;
                            }
                            self.active_screen = Screen::None;
                        }
                    },
                    _ => {}
                },
            }
        }
        Ok(())
    }
}
