pub mod components;
pub mod functions;
pub mod matches;
use std::{
    error::Error,
    io::{Cursor, Stdout},
    process::Command,
    time::{Duration, Instant},
};

use bincode::config;
use conanprotocol::{
    comm::enums::{IPCCmd, IPCRes, encode},
    config::ConanConfig,
    msg::Mode,
};
use database::entities::{chat::Chat, group::DBGroup, peer::Peer};
use ratatui::{
    Frame, Terminal,
    layout::{Constraint, Direction, HorizontalAlignment, Layout},
    prelude::CrosstermBackend,
    text::Line,
    widgets::{Block, Borders, ListState, Padding},
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::UnixStream,
    sync::broadcast,
};

use crate::{
    components::{
        command_pallete::CommandPallete, confirmation_screen::ConfirmScreen, input::InputScreen,
        loading_screen::LoadingScreen, main_component::MainComponents, notification::Notification,
        welcome::WelcomeScreen,
    },
    functions::{ConfirmMode, InputMode, LoadingMode, keys::Keys},
    matches::{Screen, Tab},
};

pub struct App {
    /// Current Tab
    pub tab: Tab,
    /// Current mode [`Mode`]
    pub mode: Mode,
    /// Notification Feature for alerts and errors
    pub notification: Option<(String, Instant)>,
    /// Stream for Daemon to Client
    pub stream: UnixStream,
    /// Current active screen
    pub active_screen: Screen,
    /// Whether app running
    pub running: bool,
    /// List of all the contacts
    pub contacts: Vec<Peer>,
    /// List of all the groups
    pub groups: Vec<DBGroup>,
    /// Current contact
    pub contact_idx: ListState,
    /// All the Chats for the current contact/group
    pub chats: Vec<Chat>,
    /// Chat Buffer for writing messages
    pub chat_buf: String,
    /// Chat Scroll Option to scroll texts
    pub chat_scroll: usize,
    /// Sender for [`IPCCmd`]
    pub sender: broadcast::Sender<IPCCmd>,
    /// Receiver for [`IPCCmd`]
    pub receiver: broadcast::Receiver<IPCCmd>,
}

impl App {
    /// # Errors
    pub async fn create(config: ConanConfig) -> std::io::Result<Self> {
        let socket_path = &config.socket_path;
        let stream = if let Ok(s) = UnixStream::connect(socket_path).await {
            s
        } else {
            eprintln!("Server not started. Starting.");
            let server_bin = std::env::current_exe()
                .ok()
                .and_then(|p| p.parent().map(|d| d.join("conan-server")))
                .filter(|p| p.exists())
                .unwrap_or_else(|| "conan-server".into());
            Command::new(server_bin)
                .args([
                    "-s",
                    &config.socket_path,
                    "-k",
                    &config.arti_key_store,
                    "-C",
                    &config.cache_path,
                    "-d",
                    &config.db_path,
                ])
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .spawn()?;
            loop {
                tokio::time::sleep(Duration::from_millis(500)).await;
                if let Ok(s) = UnixStream::connect(socket_path).await {
                    break s;
                }
            }
        };
        let (sender, receiver) = tokio::sync::broadcast::channel::<IPCCmd>(100);
        Ok(Self {
            tab: Tab::None,
            mode: Mode::Normal,
            notification: None,
            stream,
            contacts: vec![],
            groups: vec![],
            contact_idx: ListState::default(),
            chats: vec![],
            chat_buf: String::new(),
            chat_scroll: 0,
            active_screen: Screen::None,
            running: true,
            sender,
            receiver,
        })
    }
    /// # Errors
    pub async fn manage_terminal(
        &mut self,
        terminal: &mut Terminal<CrosstermBackend<Stdout>>,
        userid: &str,
    ) -> Result<(), Box<dyn Error>> {
        let now = Instant::now();
        let timer = Duration::from_secs(1);
        terminal.clear()?;
        loop {
            if now.elapsed() > timer {
                break;
            }
            terminal.draw(|f| self.render_welcome(f))?;
        }
        let sender = self.sender.clone();
        tokio::spawn(async move {
            loop {
                _ = sender.send(IPCCmd::PeerList);
                tokio::time::sleep(Duration::from_millis(500)).await;
                _ = sender.send(IPCCmd::GroupList);
                tokio::time::sleep(Duration::from_millis(500)).await;
            }
        });
        let sender = self.sender.clone();
        tokio::spawn(async move {
            loop {
                _ = sender.send(IPCCmd::PingChat);
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
        });
        self.active_screen = Screen::LoadingScreen {
            loading_text: "Starting Server...".into(),
            mode: LoadingMode::StartServer,
        };
        self.send(IPCCmd::StartServer).await?;
        while self.running {
            terminal.draw(|f| {
                self.set_layout(f, userid);
                self.render(f);
            })?;
            if let Ok(s) = self.receiver.try_recv() {
                match s {
                    IPCCmd::PingChat => {
                        self.update_chats().await?;
                    }
                    cmd => {
                        self.send(cmd).await?;
                    }
                }
            }
            self.manage_keys().await?;
            self.manage_ipc().await?;
        }
        crossterm::terminal::disable_raw_mode()?;
        terminal.clear()?;
        Ok(())
    }

    /// # Errors
    pub async fn manage_ipc(&mut self) -> Result<(), Box<dyn Error>> {
        if let Some(res) = self.try_recv()? {
            match res {
                IPCRes::ServerStarted(ans) => {
                    if ans {
                        self.notification = Some(("Server Started.".into(), Instant::now()));
                        if matches!(
                            self.active_screen,
                            Screen::LoadingScreen {
                                mode: LoadingMode::StartServer,
                                ..
                            }
                        ) {
                            self.active_screen = Screen::None;
                        }
                    } else {
                        tokio::time::sleep(Duration::from_millis(500)).await;
                        self.send(IPCCmd::StartServer).await?;
                    }
                }

                IPCRes::AddedPeer(peer) => {
                    self.notification = Some(("Added Peer.".into(), Instant::now()));
                    let cmd = IPCCmd::Connect(peer.id);
                    self.active_screen = Screen::LoadingScreen {
                        loading_text: "Adding peer...".to_string(),
                        mode: LoadingMode::PeerConnect(peer.id),
                    };
                    self.contacts.push(peer);
                    self.send(cmd).await?;
                }

                IPCRes::Connected(peer_id, connected) => {
                    if connected {
                        if matches!(
                            self.active_screen,
                            Screen::LoadingScreen {
                                mode: LoadingMode::PeerConnect(_),
                                ..
                            }
                        ) {
                            self.active_screen = Screen::None;
                        }
                        self.notification = Some(("Connected.".to_string(), Instant::now()));
                    } else {
                        tokio::time::sleep(Duration::from_millis(500)).await;
                        self.send(IPCCmd::Connect(peer_id)).await?;
                    }
                }

                IPCRes::Error(text) => {
                    self.notification = Some((text, Instant::now()));
                    if matches!(self.active_screen, Screen::LoadingScreen { .. }) {
                        self.active_screen = Screen::None;
                    }
                }

                IPCRes::Notification(text) => {
                    self.notification = Some((text.clone(), Instant::now()));
                }

                IPCRes::PeerList(peers) => {
                    self.contacts = peers;
                }

                IPCRes::GroupConnected(msg, _) => {
                    self.notification = Some((msg, Instant::now()));
                    if matches!(
                        self.active_screen,
                        Screen::LoadingScreen {
                            mode: LoadingMode::GroupConnect(_),
                            ..
                        }
                    ) {
                        self.active_screen = Screen::None;
                    }
                }

                IPCRes::GroupList(list) => {
                    self.groups = list;
                }

                IPCRes::Text(sent_idx, text) => {
                    let (true, Some(idx)) = self.current_contact() else {
                        return Ok(());
                    };
                    if idx != usize::from(sent_idx) {
                        return Ok(());
                    }
                    let Some(cur_cont) = self.contacts.get(idx) else {
                        return Ok(());
                    };
                    #[allow(clippy::cast_possible_truncation)]
                    let idx = idx as u16;
                    if cur_cont.id.eq(&idx) {
                        let new_chat = Chat::chat_to_rec(&text, idx);
                        self.chats.push(new_chat);
                    }
                }

                IPCRes::ChatList { peer_id, chats } => {
                    let (true, Some(idx)) = self.current_contact() else {
                        return Ok(());
                    };
                    let Some(cur_cont) = self.contacts.get(idx) else {
                        return Ok(());
                    };
                    if cur_cont.id != peer_id {
                        return Ok(());
                    }
                    self.chats = chats;
                }

                IPCRes::GroupChatList { group_idx, chats } => {
                    let (false, Some(idx)) = self.current_contact() else {
                        return Ok(());
                    };
                    let Some(grp) = self.groups.get(idx) else {
                        return Ok(());
                    };
                    if grp.id != group_idx {
                        return Ok(());
                    }
                    self.chats = chats;
                }

                IPCRes::RenamedPeer(idx) => {
                    if let Some(target) = self.contacts.get(idx as usize) {
                        self.notification = Some((
                            format!("Peer name changed to {}", target.name),
                            Instant::now(),
                        ));
                        if let Screen::InputScreen { ref mode, .. } = self.active_screen
                            && matches!(mode, InputMode::RenamePeer)
                        {
                            self.active_screen = Screen::None;
                        }
                    }
                }

                IPCRes::DeletedGroup(_) => {
                    self.notification = Some(("Group deleted.".to_string(), Instant::now()));
                }

                IPCRes::DeletedPeer(_) => {
                    self.notification = Some(("Peer deleted.".to_string(), Instant::now()));
                    if let Screen::ConfirmScreen { ref mode, .. } = self.active_screen
                        && matches!(mode, ConfirmMode::DeletePeer)
                    {
                        self.active_screen = Screen::None;
                    }
                }
                _ => {}
            }
        }
        Ok(())
    }

    fn set_layout(&mut self, f: &mut Frame<'_>, userid: &str) {
        let area = f.area();
        let self_addr = if let Some(c) = self.contacts.first() {
            c.address.clone()
        } else {
            String::new()
        };
        let main_block = Block::new()
            .title_top(
                Line::from(format!(" Conan - {userid} ")).alignment(HorizontalAlignment::Center),
            )
            .title_bottom(Line::from(self_addr).alignment(HorizontalAlignment::Right))
            .borders(Borders::NONE)
            .padding(Padding::new(1, 1, 0, 0));

        let inner_block = main_block.inner(area);

        let chunks = Layout::default()
            .constraints([Constraint::Min(30), Constraint::Percentage(100)])
            .spacing(1)
            .margin(1)
            .direction(Direction::Horizontal)
            .split(inner_block);
        let selected = matches!(self.tab, Tab::Contact);
        self.render_contact_list(f, chunks[0], selected);

        let chat_block = Layout::default()
            .constraints([Constraint::Percentage(100), Constraint::Min(3)])
            .direction(Direction::Vertical)
            .split(chunks[1]);

        // right chunk
        let selected = matches!(self.tab, Tab::Chat);

        if self.contact_idx.selected().is_some() {
            self.render_chats(f, selected, chat_block[0]);
            self.render_chat_bar(f, true, chat_block[1]);
        } else {
            self.render_chats(f, selected, chunks[1]);
        }

        f.render_widget(main_block, area);
    }

    fn render(&mut self, f: &mut Frame<'_>) {
        if let Some((text, time)) = self.notification.as_ref() {
            if time.elapsed() < Duration::from_secs(2) {
                self.render_notification(f, text);
            } else {
                self.notification = None;
            }
        }
        match self.active_screen {
            Screen::None => {}
            Screen::InputScreen {
                ref input,
                ref cursor_pos,
                ref prompt,
                ..
            } => {
                self.render_input_block(f, input, prompt, cursor_pos);
            }
            Screen::LoadingScreen {
                ref loading_text, ..
            } => {
                self.render_loading_screen(f, loading_text);
            }
            Screen::ConfirmScreen {
                ref prompt,
                ref yes_selected,
                ..
            } => {
                self.render_confirmation(f, prompt, yes_selected);
            }
            Screen::CommandPalette {
                ref text,
                ref cursor_pos,
                ..
            } => {
                self.render_command_palette(f, text, cursor_pos);
            }
        }
    }

    /// # Errors
    async fn send(&mut self, cmd: IPCCmd) -> std::io::Result<()> {
        let bytes = encode(cmd);
        self.stream.write_all(&bytes).await?;
        self.stream.flush().await?;
        Ok(())
    }

    fn try_recv(&mut self) -> Result<Option<IPCRes>, Box<dyn Error>> {
        let mut buf = [0u8; 4096];
        if let Ok(n) = self.stream.try_read(&mut buf) {
            let (res, _) = bincode::decode_from_slice(&buf[..n], config::standard())?;
            return Ok(Some(res));
        }
        Ok(None)
    }

    async fn _recv(&mut self) -> Result<IPCRes, Box<dyn Error>> {
        let mut cursor = Cursor::new(Vec::<u8>::new());
        self.stream.read_exact(cursor.get_mut()).await?;
        let (res, _) = bincode::decode_from_slice(cursor.get_ref(), config::standard())?;
        Ok(res)
    }

    /// Fetches current contact in terminal
    fn current_contact(&self) -> (bool, Option<usize>) {
        let Some(idx) = self.contact_idx.selected() else {
            return (false, None);
        };
        // Selected `Groups`
        if idx == self.contacts.len() {
            return (false, None);
        }
        let is_peer = idx < self.contacts.len();
        let idx = if is_peer {
            idx
        } else {
            idx - self.contacts.len() - 1
        };
        (is_peer, Some(idx))
    }

    /// Updates chats on screen by calling database via socket
    /// # Errors
    pub async fn update_chats(&mut self) -> Result<(), Box<dyn Error>> {
        let (is_peer, idx) = self.current_contact();
        // Something is selected
        if let Some(cur_idx) = idx {
            if is_peer && let Some(peer) = self.contacts.get(cur_idx) {
                #[allow(clippy::cast_possible_truncation)]
                self.send(IPCCmd::ChatList {
                    peer_id: peer.id,
                    msg_amount: 50,
                })
                .await?;
            } else if let Some(grp) = self.groups.get(cur_idx) {
                self.send(IPCCmd::GroupChatList {
                    group_idx: grp.id,
                    msg_amount: 50,
                })
                .await?;
            }
        } else {
            // nothing is selected
            self.chats.clear();
        }
        Ok(())
    }
}
