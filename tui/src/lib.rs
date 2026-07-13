pub mod components;
pub mod functions;
pub mod matches;
use std::{
    error::Error,
    io::{Cursor, Stdout},
    time::{Duration, Instant},
};

use bincode::config;
use conanprotocol::{
    comm::enums::{IPCCmd, IPCRes, encode},
    entities::database::{chat::Chat, peer::Peer},
    msg::Mode,
};
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
        confirmation_screen::ConfirmScreen, loading_screen::LoadingScreen,
        main_component::MainComponents, new_peer::NewPeer, notification::Notification,
        welcome::WelcomeScreen,
    },
    functions::keys::Keys,
    matches::{Screen, Tab},
};

pub struct App {
    pub tab: Tab,
    pub mode: Mode,
    pub notification: Option<(String, Instant)>,
    pub stream: UnixStream,
    pub active_screen: Screen,
    pub running: bool,
    pub time: Instant,
    pub contacts: Vec<Peer>,
    pub contact_idx: ListState,
    pub chats: Vec<Chat>,
    pub chat_buf: String,
    pub sender: broadcast::Sender<IPCCmd>,
    pub receiver: broadcast::Receiver<IPCCmd>,
}

impl App {
    /// # Errors
    pub async fn create(socket_path: &str) -> std::io::Result<Self> {
        let stream = UnixStream::connect(socket_path).await?;
        let time = Instant::now();
        let (sender, receiver) = tokio::sync::broadcast::channel::<IPCCmd>(100);
        Ok(Self {
            tab: Tab::None,
            mode: Mode::Normal,
            notification: None,
            stream,
            contacts: vec![],
            contact_idx: ListState::default(),
            chats: vec![],
            chat_buf: String::new(),
            active_screen: Screen::None,
            running: true,
            time,
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
                _ = sender.send(IPCCmd::Tick);
                tokio::time::sleep(Duration::from_secs(1)).await;
            }
        });
        while self.running {
            terminal.draw(|f| {
                self.set_layout(f, userid);
                self.render(f);
            })?;
            if let Ok(s) = self.receiver.try_recv() {
                match s {
                    IPCCmd::Tick => {
                        self.send(IPCCmd::PeerList).await?;
                    }
                    _ => {}
                }
            }
            self.manage_keys().await?;
            self.manage_ipc()?;
        }
        Ok(())
    }

    /// # Errors
    pub fn manage_ipc(&mut self) -> Result<(), Box<dyn Error>> {
        if let Some(res) = self.try_recv()? {
            match res {
                IPCRes::ServerStarted => {
                    if let Screen::LoadingScreen { .. } = self.active_screen {
                        self.active_screen = Screen::None;
                        self.notification = Some(("Server Started.".to_string(), Instant::now()));
                    }
                }
                IPCRes::Connected(_, _) => {
                    if let Screen::LoadingScreen { .. } = self.active_screen {
                        self.active_screen = Screen::None;
                        self.notification =
                            Some(("Connected to Peer.".to_string(), Instant::now()));
                    }
                }
                IPCRes::Error(text) => {
                    self.notification = Some((text, Instant::now()));
                    if matches!(self.active_screen, Screen::LoadingScreen { .. }) {
                        self.active_screen = Screen::None;
                    }
                }
                IPCRes::Notification(text) => {
                    self.notification = Some((text, Instant::now()));
                }
                IPCRes::PeerList(peers) => {
                    self.contacts = peers;
                }
                _ => {}
            }
        }
        Ok(())
    }

    fn set_layout(&mut self, f: &mut Frame<'_>, userid: &str) {
        let area = f.area();
        let main_block = Block::new()
            .title(Line::from(format!(" Conan - {userid} ")).alignment(HorizontalAlignment::Center))
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
            Screen::PeerInputScreen {
                ref input,
                ref cursor_pos,
            } => {
                self.render_new_peer_block(f, input, cursor_pos);
            }
            Screen::LoadingScreen { ref loading_text } => {
                self.render_loading_screen(f, loading_text);
            }
            Screen::ConfirmScreen {
                ref prompt,
                ref options,
                ref idx,
            } => {
                self.render_confirmation(f, prompt, options, idx);
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
}
