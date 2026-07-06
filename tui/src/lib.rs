pub mod components;
pub mod functions;
pub mod matches;
use std::{
    error::Error,
    io::{Cursor, Stdout},
    time::{Duration, Instant},
};

use bincode::config;
use conanprotocol::comm::enums::{IPCCmd, IPCRes, encode};
use ratatui::{
    Frame, Terminal,
    layout::{Constraint, Direction, HorizontalAlignment, Layout},
    prelude::CrosstermBackend,
    text::Line,
    widgets::{Block, Borders, Padding},
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::UnixStream,
    sync::broadcast::{Receiver, Sender},
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
    pub notification: Option<(String, Instant)>,
    /// Send commands to Daemon
    pub cmd_sx: Sender<IPCCmd>,

    /// Receive responses from Daemon
    pub res_rx: Receiver<IPCRes>,
    cmd_rx: Receiver<IPCCmd>,
    res_sx: Sender<IPCRes>,
    pub stream: UnixStream,
    pub active_screen: Screen,
    pub running: bool,
}

impl App {
    /// # Errors
    pub async fn create(socket_path: &str) -> std::io::Result<Self> {
        let stream = UnixStream::connect(socket_path).await?;
        let (cmd_sx, cmd_rx) = tokio::sync::broadcast::channel::<IPCCmd>(100);
        let (res_sx, res_rx) = tokio::sync::broadcast::channel::<IPCRes>(100);
        Ok(Self {
            tab: Tab::None,
            notification: None,
            stream,
            cmd_sx,
            res_sx,
            cmd_rx,
            res_rx,
            active_screen: Screen::None,
            running: true,
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
        while self.running {
            terminal.draw(|f| {
                self.set_layout(f, userid);
                self.render(f);
            })?;
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
                _ => {}
            }
        }
        Ok(())
    }

    fn set_layout(&self, f: &mut Frame<'_>, userid: &str) {
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
        let list = ["John Doe", "Jennie"].to_vec();
        let selected = matches!(self.tab, Tab::Contact { .. });

        // left chunk
        self.render_contact_list(f, &list, 0, chunks[0], selected);
        // right chunk
        self.render_chats(f, true, chunks[1]);

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

    async fn recv(&mut self) -> Result<IPCRes, Box<dyn Error>> {
        let mut cursor = Cursor::new(Vec::<u8>::new());
        self.stream.read_exact(cursor.get_mut()).await?;
        let (res, _) = bincode::decode_from_slice(cursor.get_ref(), config::standard())?;
        Ok(res)
    }
}

pub trait TerminalControl {
    fn next_tab(&mut self);
}

impl TerminalControl for App {
    fn next_tab(&mut self) {
        let new_tab = match self.tab {
            Tab::Contact { .. } => Tab::Chat,
            _ => Tab::Contact {
                list: vec!["hello".into(), "world".into()],
                active: Some(1),
            },
        };
        self.tab = new_tab;
    }
}
