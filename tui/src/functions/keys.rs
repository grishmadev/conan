use std::time::{Duration, Instant};

use conanprotocol::{comm::enums::IPCCmd, entities::database::chat::Chat, msg::Mode};
use crossterm::event::{self, Event, KeyCode, KeyEvent};

use crate::{
    App,
    functions::{ConfirmMode, InputMode, LoadingMode, terminal_control::TerminalControl},
    matches::{Screen, Tab},
};

pub trait Keys {
    fn manage_keys(&mut self) -> impl Future<Output = std::io::Result<()>>;
    fn handle_none_screen(&mut self, key: KeyEvent) -> impl Future<Output = std::io::Result<()>>;
    fn handle_input_screen(&mut self, key: KeyEvent) -> impl Future<Output = std::io::Result<()>>;
    fn handle_confirm_screen(&mut self, key: KeyEvent)
    -> impl Future<Output = std::io::Result<()>>;
}

impl Keys for App {
    async fn manage_keys(&mut self) -> std::io::Result<()> {
        if event::poll(Duration::from_millis(10))?
            && let Event::Key(key) = event::read()?
        {
            if key.code == KeyCode::Char('c')
                && key
                    .modifiers
                    .contains(crossterm::event::KeyModifiers::CONTROL)
            {
                self.running = false;
                return Ok(());
            }
            match self.active_screen {
                Screen::None => {
                    self.handle_none_screen(key).await?;
                }
                Screen::InputScreen { .. } => {
                    self.handle_input_screen(key).await?;
                }
                Screen::LoadingScreen { .. } => {}
                Screen::ConfirmScreen { .. } => {
                    self.handle_confirm_screen(key).await?;
                }
            }
        }
        Ok(())
    }

    async fn handle_none_screen(&mut self, key: KeyEvent) -> std::io::Result<()> {
        match key.code {
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
                if self.contact_idx.selected() == Some(0) {
                    self.notification = Some(("Cannot delete Self".into(), Instant::now()));
                    return Ok(());
                }
                let Some(conn) = self.current_contact() else {
                    return Ok(());
                };
                self.active_screen = Screen::ConfirmScreen {
                    prompt: format!("Are you sure you want to delete {}?", conn.name),
                    yes_selected: false,
                    mode: ConfirmMode::DeletePeer,
                };
            }
            KeyCode::Char('r') if matches!(self.tab, Tab::Contact) => {
                if let Some(peer) = self.current_contact()
                    && peer.id == 1
                {
                    self.notification = Some(("Cannot rename self.".to_string(), Instant::now()));
                    return Ok(());
                }
                let Some(conn) = self.current_contact() else {
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
            KeyCode::Char('j') => {
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
            KeyCode::Char('k') => {
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
            KeyCode::Backspace => {
                if let Mode::Insert { ref mut cursor_pos } = self.mode
                    && *cursor_pos > 0
                {
                    *cursor_pos -= 1;
                    self.chat_buf.remove(*cursor_pos);
                }
            }
            KeyCode::Delete => {
                if let Mode::Insert { ref cursor_pos } = self.mode
                    && (0..self.chat_buf.len()).contains(cursor_pos)
                {
                    self.chat_buf.remove(*cursor_pos);
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
                            // Self: we don't need loading screen, just load the chat
                            #[allow(clippy::cast_possible_truncation)]
                            self.send(IPCCmd::ChatList {
                                peer_id: id as u8,
                                msg_amount: 50,
                            })
                            .await?;
                            self.next_tab();
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
                    Tab::Chat => {
                        if self.chat_buf.trim().is_empty() {
                            return Ok(()); // prevent empty messages
                        }
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
                        if !current_peer.connected && current_peer.id != 1 {
                            self.notification =
                                Some(("Contact not connected.".into(), Instant::now()));
                            return Ok(());
                        }
                        let chat = Chat::chat_to_send(&self.chat_buf, current_peer.id);
                        self.chats.push(chat);
                        self.chat_buf = String::new();
                        if let Mode::Insert { ref mut cursor_pos } = self.mode {
                            *cursor_pos = 0;
                        }
                    }
                    Tab::None => {}
                }
            }
            KeyCode::Left => {
                if let Mode::Insert { ref mut cursor_pos } = self.mode
                    && *cursor_pos > 0
                {
                    *cursor_pos -= 1;
                }
            }
            KeyCode::Right => {
                if let Mode::Insert { ref mut cursor_pos } = self.mode
                    && self.chat_buf.len() > *cursor_pos
                {
                    *cursor_pos += 1;
                }
            }
            KeyCode::Esc => {
                // Return to contact list from chat
                if self.tab == Tab::Chat {
                    self.tab = Tab::Contact;
                    self.mode = Mode::Normal;
                }
            }
            KeyCode::Down => match self.tab {
                Tab::Contact => {
                    if let Some(idx) = self.contact_idx.selected()
                        && idx == self.contacts.len() - 1
                    {
                        self.contact_idx.select_first();
                    } else {
                        self.contact_idx.select_next();
                    }
                }
                Tab::Chat => {
                    self.chat_scroll = self.chat_scroll.saturating_sub(5);
                }
                _ => {}
            },
            KeyCode::Up => match self.tab {
                Tab::Contact => {
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
                Tab::Chat => {
                    self.chat_scroll = self.chat_scroll.saturating_add(5);
                }
                _ => {}
            },
            _ => {}
        }
        Ok(())
    }

    async fn handle_input_screen(&mut self, key: KeyEvent) -> std::io::Result<()> {
        let Screen::InputScreen {
            ref mut input,
            ref mut cursor_pos,
            ref mut mode,
            ..
        } = self.active_screen
        else {
            return Ok(());
        };
        match key.code {
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
                    #[allow(clippy::cast_possible_truncation)]
                    let msg = IPCCmd::RenamePeer(peer.id as u8, input.clone());
                    self.send(msg).await?;
                    self.active_screen = Screen::None;
                }
            },
            KeyCode::Esc => {
                self.active_screen = Screen::None;
            }
            _ => {}
        }
        Ok(())
    }

    async fn handle_confirm_screen(&mut self, key: KeyEvent) -> std::io::Result<()> {
        let Screen::ConfirmScreen {
            ref mut yes_selected,
            ref mut mode,
            ..
        } = self.active_screen
        else {
            return Ok(());
        };
        match key.code {
            KeyCode::Left | KeyCode::Right => {
                *yes_selected = !*yes_selected;
            }
            KeyCode::Enter => match mode {
                ConfirmMode::Exit => {
                    if *yes_selected {
                        crossterm::terminal::disable_raw_mode()?;
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
        }
        Ok(())
    }
}
