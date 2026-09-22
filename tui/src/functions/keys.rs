use std::time::{Duration, Instant};

use conandatabase::entities::tuichat::TuiChat;
use conanprotocol::comm::enums::ipccmd::IPCCmd;
use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyModifiers};

use crate::{
    App,
    functions::{ConfirmMode, InputMode, LoadingMode},
    matches::{Mode, PaletteCommand, Screen, Tab},
};

pub trait Keys {
    /// Manage Keymappings.
    ///
    /// # Errors
    fn manage_keys(&mut self) -> impl Future<Output = std::io::Result<()>>;
    /// Handles Keys for [`Screen::None`]
    ///
    /// # Errors
    fn handle_none_screen(&mut self, key: KeyEvent) -> impl Future<Output = std::io::Result<()>>;
    /// Handles Keys for [`Screen::InputScreen`]
    ///
    /// # Errors
    fn handle_input_screen(&mut self, key: KeyEvent) -> impl Future<Output = std::io::Result<()>>;
    /// Handles Keys for [`Screen::LoadingScreen`]
    ///
    /// # Errors
    fn handle_loading_screen(&mut self, key: KeyEvent) -> std::io::Result<()>;
    /// Handles Keys for [`Screen::ConfirmScreen`]
    ///
    /// # Errors
    fn handle_confirm_screen(&mut self, key: KeyEvent)
    -> impl Future<Output = std::io::Result<()>>;
    /// Handles Keys for [`Screen::CommandPallete`]
    ///
    /// # Errors
    fn handle_command_palette(
        &mut self,
        key: KeyEvent,
    ) -> impl Future<Output = std::io::Result<()>>;
    /// Toggles tab when called
    fn toggle_tab(&mut self);
    /// Triggers next idx when called
    fn next_idx(&mut self);
    /// Triggers prev index when called
    fn prev_idx(&mut self);
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
                Screen::LoadingScreen { .. } => {
                    self.handle_loading_screen(key)?;
                }
                Screen::ConfirmScreen { .. } => {
                    self.handle_confirm_screen(key).await?;
                }
                Screen::CommandPalette { .. } => {
                    self.handle_command_palette(key).await?;
                }
            }
        }
        Ok(())
    }

    #[allow(clippy::too_many_lines)]
    async fn handle_none_screen(&mut self, key: KeyEvent) -> std::io::Result<()> {
        match key.code {
            KeyCode::Tab => {
                self.toggle_tab();
            }
            KeyCode::Char(ch) if matches!(self.mode, Mode::Insert { .. }) => {
                if let Mode::Insert { ref mut cursor_pos } = self.mode {
                    self.chat_buf.insert(*cursor_pos, ch);
                    *cursor_pos += 1;
                }
            }
            KeyCode::Char('p') if key.modifiers == KeyModifiers::CONTROL => {
                if self.active_screen == Screen::None {
                    self.active_screen = Screen::CommandPalette {
                        options: vec![],
                        text: String::new(),
                        cursor_pos: 0,
                    };
                }
            }
            KeyCode::Char('D' | 'd')
                if matches!(self.tab, Tab::Contact)
                    && key.modifiers.contains(KeyModifiers::SHIFT) =>
            {
                let (is_peer, idx) = self.current_contact();
                let Some(idx) = idx else {
                    self.notification = Some(("No peer selected.".into(), Instant::now()));
                    return Ok(());
                };
                if is_peer
                    && let Some(peer) = self.contacts.get(idx)
                    && !peer.connected
                {
                    self.notification = Some(("Peer is not connected".into(), Instant::now()));
                    return Ok(());
                }
                if self.active_screen == Screen::None {
                    self.active_screen = Screen::ConfirmScreen {
                        prompt: "Are you sure you want to disconnect?".into(),
                        yes_selected: false,
                        mode: ConfirmMode::DisconnectPeer,
                    };
                }
            }
            KeyCode::Char('d') if matches!(self.tab, Tab::Contact) => {
                let (is_peer, idx) = self.current_contact();
                let Some(idx) = idx else {
                    return Ok(());
                };
                if is_peer && idx == 0 {
                    self.notification = Some(("Cannot delete Self".into(), Instant::now()));
                    return Ok(());
                }
                let name = if is_peer {
                    &self.contacts.get(idx).unwrap().name
                } else {
                    &self.groups.get(idx).unwrap().name
                };
                self.active_screen = Screen::ConfirmScreen {
                    prompt: format!("Are you sure you want to delete {name}?"),
                    yes_selected: false,
                    mode: ConfirmMode::DeletePeer,
                };
            }
            KeyCode::Char('r') if matches!(self.tab, Tab::Contact) => {
                let (is_peer, Some(idx)) = self.current_contact() else {
                    return Ok(());
                };
                if is_peer {
                    let Some(peer) = self.contacts.get(idx) else {
                        return Ok(());
                    };
                    if peer.id == 1 {
                        self.notification =
                            Some(("Cannot rename self.".to_string(), Instant::now()));
                        return Ok(());
                    }
                    self.active_screen = Screen::InputScreen {
                        input: peer.name.clone(),
                        cursor_pos: peer.name.len(),
                        prompt: " Rename Peer ".into(),
                        mode: InputMode::RenamePeer,
                    };
                } else {
                    let Some(group) = self.groups.get(idx) else {
                        return Ok(());
                    };
                    self.active_screen = Screen::InputScreen {
                        input: group.name.clone(),
                        cursor_pos: group.name.len(),
                        prompt: " Rename Group ".into(),
                        mode: InputMode::RenameGroup,
                    };
                }
            }
            KeyCode::Char('a') => {
                self.active_screen = Screen::InputScreen {
                    input: String::new(),
                    cursor_pos: 0,
                    prompt: " New Peer ".to_string(),
                    mode: InputMode::NewPeer,
                }
            }
            KeyCode::Char('j') if self.tab == Tab::Contact => {
                self.next_idx();
            }

            KeyCode::Char('k') if self.tab == Tab::Contact => {
                self.prev_idx();
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
                let (is_peer, Some(idx)) = self.current_contact() else {
                    return Ok(());
                };
                match self.tab {
                    Tab::Contact => {
                        if is_peer {
                            // Connecting to a peer
                            let Some(peer) = self.contacts.get(idx) else {
                                return Ok(());
                            };
                            if peer.id == 1 {
                                // Self: we don't need loading screen, just load the chat
                                #[allow(clippy::cast_possible_truncation)]
                                self.send(IPCCmd::ChatList {
                                    peer_id: peer.id,
                                    msg_amount: 50,
                                })
                                .await?;
                                self.toggle_tab();
                                return Ok(());
                            }
                            self.active_screen = Screen::LoadingScreen {
                                loading_text: "Connecting...".into(),
                                mode: LoadingMode::PeerConnect(peer.id),
                            };
                            self.send(IPCCmd::Connect(peer.id)).await?;
                            self.send(IPCCmd::ChatList {
                                #[allow(clippy::cast_possible_truncation)]
                                peer_id: idx as u16,
                                msg_amount: 50,
                            })
                            .await?;
                        } else {
                            let Some(grp) = self.groups.get(idx) else {
                                return Ok(());
                            };
                            self.active_screen = Screen::LoadingScreen {
                                loading_text: format!("Connecting to {}..", grp.name),
                                mode: LoadingMode::GroupConnect(grp.id),
                            };
                            self.send(IPCCmd::GroupConnect(grp.id)).await?;
                        }
                    }
                    Tab::Chat => {
                        let text = self.chat_buf.trim().to_string();
                        if text.is_empty() {
                            return Ok(()); // prevent empty messages
                        }
                        if is_peer {
                            let Some(current_peer) = self.contacts.get(idx) else {
                                eprintln!("Peer not found.");
                                return Ok(());
                            };
                            let current_peer = current_peer.clone();
                            #[allow(clippy::cast_possible_truncation)]
                            self.send(IPCCmd::Text(current_peer.id, text.clone()))
                                .await?;
                            if !current_peer.connected && current_peer.id != 1 {
                                self.notification =
                                    Some(("Contact not connected.".into(), Instant::now()));
                                return Ok(());
                            }
                        } else {
                            let Some(grp) = self.groups.get(idx) else {
                                return Ok(());
                            };
                            #[allow(clippy::cast_possible_truncation)]
                            self.send(IPCCmd::GroupText(grp.id, text.clone())).await?;
                        }
                        let chat = TuiChat::build(&text, "Me");
                        self.chats.push(chat);
                        self.chat_buf = String::new();
                        if let Mode::Insert { ref mut cursor_pos } = self.mode {
                            *cursor_pos = 0;
                        }
                    }
                    Tab::None => unimplemented!(),
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
                    self.next_idx();
                }
                Tab::Chat => {
                    self.chat_scroll = self.chat_scroll.saturating_sub(5);
                }
                _ => unimplemented!(),
            },
            KeyCode::Up => match self.tab {
                Tab::Contact => {
                    self.prev_idx();
                }
                Tab::Chat => {
                    self.chat_scroll = self.chat_scroll.saturating_add(5);
                }
                _ => unimplemented!(),
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
                    let msg = IPCCmd::AddPeer(input.clone(), 80);
                    self.send(msg).await?;
                }
                InputMode::RenamePeer => {
                    let Some(idx) = self.contact_idx.selected() else {
                        return Ok(());
                    };
                    let Some(peer) = self.contacts.get(idx) else {
                        return Ok(());
                    };
                    #[allow(clippy::cast_possible_truncation)]
                    let msg = IPCCmd::RenamePeer(peer.id, input.clone());
                    self.send(msg).await?;
                    self.active_screen = Screen::None;
                }
                InputMode::RenameGroup => {
                    let Some(idx) = self.contact_idx.selected() else {
                        self.notification = Some(("nothing selected".to_string(), Instant::now()));
                        return Ok(());
                    };
                    let new_idx = idx - self.contacts.len() - 1;
                    let Some(group) = self.groups.get(new_idx) else {
                        self.notification = Some((
                            format!("idx: {}, contact len: {}", idx, self.contacts.len())
                                .to_string(),
                            Instant::now(),
                        ));
                        return Ok(());
                    };
                    #[allow(clippy::cast_possible_truncation)]
                    let msg = IPCCmd::RenameGroup(group.id, input.clone());
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

    fn handle_loading_screen(&mut self, key: KeyEvent) -> std::io::Result<()> {
        let Screen::LoadingScreen {
            ref loading_text, ..
        } = self.active_screen
        else {
            return Ok(());
        };
        if key.code == KeyCode::Esc {
            self.notification = Some((loading_text.into(), Instant::now()));
            self.active_screen = Screen::None;
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
            KeyCode::Esc => {
                self.active_screen = Screen::None;
            }
            KeyCode::Left | KeyCode::Right => {
                *yes_selected = !*yes_selected;
            }
            KeyCode::Enter => {
                match mode {
                    ConfirmMode::Exit => {
                        if *yes_selected {
                            self.running = false;
                        }
                    }
                    ConfirmMode::DeletePeer => {
                        if *yes_selected && let Some(idx) = self.contact_idx.selected() {
                            let is_peer = idx < self.contacts.len();
                            let idx = if is_peer {
                                idx
                            } else {
                                idx - self.contacts.len() - 1
                            };
                            let cmd = if is_peer {
                                let Some(peer) = self.contacts.get(idx) else {
                                    return Ok(());
                                };
                                IPCCmd::DeletePeer(peer.id)
                            } else {
                                let Some(group) = self.groups.get(idx) else {
                                    return Ok(());
                                };
                                IPCCmd::DeleteGroup(group.id)
                            };
                            self.send(cmd).await?;
                        }
                    }
                    ConfirmMode::DisconnectPeer => {
                        if *yes_selected
                            && let (true, Some(idx)) = self.current_contact()
                            && let Some(peer) = self.contacts.get(idx)
                        {
                            #[allow(clippy::cast_possible_truncation)]
                            self.send(IPCCmd::Disconnect(peer.id)).await?;
                        }
                    }
                }
                self.active_screen = Screen::None;
            }
            _ => {}
        }
        Ok(())
    }

    async fn handle_command_palette(
        &mut self,
        key: crossterm::event::KeyEvent,
    ) -> std::io::Result<()> {
        let Screen::CommandPalette {
            ref mut text,
            ref mut cursor_pos,
            ..
        } = self.active_screen
        else {
            return Ok(());
        };
        match key.code {
            KeyCode::Char(ch) => {
                text.insert(*cursor_pos, ch);
                *cursor_pos += 1;
            }
            KeyCode::Backspace => {
                if *cursor_pos > 0 {
                    *cursor_pos -= 1;
                    text.remove(*cursor_pos);
                }
            }
            KeyCode::Delete => {
                if (0..text.len()).contains(cursor_pos) {
                    text.remove(*cursor_pos);
                }
            }
            KeyCode::Left => {
                if *cursor_pos > 0 {
                    *cursor_pos -= 1;
                }
            }
            KeyCode::Right => {
                if text.len() > *cursor_pos {
                    *cursor_pos += 1;
                }
            }
            KeyCode::Esc => {
                self.active_screen = Screen::None;
            }
            KeyCode::Enter => match PaletteCommand::try_from(text) {
                Ok(cmd) => {
                    match cmd {
                        PaletteCommand::NewGroup(name) => {
                            self.send(IPCCmd::NewGroup(name)).await?;
                        }
                        PaletteCommand::AddToGroup(grp_name) => {
                            let (true, Some(idx)) = self.current_contact() else {
                                return Ok(());
                            };
                            let Some(curcon) = self.contacts.get(idx) else {
                                return Ok(());
                            };
                            let Some(curgrp) = self.groups.iter().find(|g| g.name == grp_name)
                            else {
                                self.notification =
                                    Some(("No such group with that name".into(), Instant::now()));
                                return Ok(());
                            };
                            self.send(IPCCmd::AddToGroup(curgrp.id, curcon.id)).await?;
                        }
                    }
                    self.active_screen = Screen::None;
                }
                Err(_) => {
                    self.notification = Some(("Invalid Command".to_string(), Instant::now()));
                }
            },
            _ => {}
        }
        Ok(())
    }

    fn toggle_tab(&mut self) {
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
