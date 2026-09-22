use std::{
    error::Error,
    time::{Duration, Instant},
};

use conandatabase::entities::{group::DBGroup, peer::Peer, tuichat::TuiChat};
use conanprotocol::comm::enums::{ipccmd::IPCCmd, ipcres::IPCRes};

use crate::{
    App,
    functions::{ConfirmMode, InputMode, LoadingMode},
    matches::Screen,
};

pub trait ManageIPC {
    /// Manages Overall Interprocess Communication between TUI and Server
    ///
    /// # Errors
    fn manage_ipc(&mut self) -> impl Future<Output = Result<(), Box<dyn Error>>>;
    /// # Errors
    fn server_started(
        &mut self,
        response: bool,
    ) -> impl Future<Output = Result<(), Box<dyn Error>>>;
    /// # Errors
    fn added_peer(&mut self, peer: Peer) -> impl Future<Output = Result<(), Box<dyn Error>>>;
    /// # Errors
    fn connected(
        &mut self,
        peer_id: u16,
        connected: bool,
    ) -> impl Future<Output = Result<(), Box<dyn Error>>>;
    /// # Errors
    fn error(&mut self, text: String) -> Result<(), Box<dyn Error>>;
    /// # Errors
    fn notification(&mut self, text: String) -> Result<(), Box<dyn Error>>;
    /// # Errors
    fn peer_list(&mut self, peers: Vec<Peer>) -> Result<(), Box<dyn Error>>;
    /// # Errors
    fn group_connected(&mut self, msg: String) -> Result<(), Box<dyn Error>>;
    /// # Errors
    fn group_list(&mut self, list: Vec<DBGroup>) -> Result<(), Box<dyn Error>>;
    /// # Errors
    fn text(&mut self, sent_idx: u16, text: String) -> Result<(), Box<dyn Error>>;
    /// # Errors
    fn chat_list(&mut self, peer_id: u16, chats: Vec<TuiChat>) -> Result<(), Box<dyn Error>>;
    /// # Errors
    fn group_chat_list(
        &mut self,
        group_idx: u16,
        chats: Vec<TuiChat>,
    ) -> Result<(), Box<dyn Error>>;
    /// # Errors
    fn renamed_peer(&mut self, idx: u16) -> Result<(), Box<dyn Error>>;
    /// # Errors
    fn deleted_group(&mut self) -> Result<(), Box<dyn Error>>;
    /// # Errors
    fn deleted_peer(&mut self) -> Result<(), Box<dyn Error>>;
}

impl ManageIPC for App {
    async fn manage_ipc(&mut self) -> Result<(), Box<dyn Error>> {
        if let Some(res) = self.try_recv()? {
            match res {
                IPCRes::ServerStarted(response) => {
                    self.server_started(response).await?;
                }
                IPCRes::AddedPeer(peer) => {
                    self.added_peer(peer).await?;
                }
                IPCRes::Connected(peer_id, connected) => {
                    self.connected(peer_id, connected).await?;
                }
                IPCRes::Error(text) => {
                    self.error(text)?;
                }
                IPCRes::Notification(text) => {
                    self.notification(text)?;
                }
                IPCRes::PeerList(peers) => {
                    self.peer_list(peers)?;
                }
                IPCRes::GroupConnected(msg, _) => {
                    self.group_connected(msg)?;
                }
                IPCRes::GroupList(list) => {
                    self.group_list(list)?;
                }
                IPCRes::Text(sent_idx, text) => {
                    self.text(sent_idx, text)?;
                }
                IPCRes::ChatList { peer_id, chats } => {
                    self.chat_list(peer_id, chats)?;
                }
                IPCRes::GroupChatList { group_idx, chats } => {
                    self.group_chat_list(group_idx, chats)?;
                }
                IPCRes::RenamedPeer(idx) => {
                    self.renamed_peer(idx)?;
                }
                IPCRes::DeletedGroup(_) => {
                    self.deleted_group()?;
                }
                IPCRes::DeletedPeer(_) => {
                    self.deleted_peer()?;
                }
                _ => {}
            }
        }
        Ok(())
    }

    async fn server_started(&mut self, response: bool) -> Result<(), Box<dyn Error>> {
        if response {
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
        Ok(())
    }

    async fn added_peer(&mut self, peer: Peer) -> Result<(), Box<dyn Error>> {
        self.notification = Some(("Added Peer.".into(), Instant::now()));
        let cmd = IPCCmd::Connect(peer.id);
        self.active_screen = Screen::LoadingScreen {
            loading_text: "Adding peer...".to_string(),
            mode: LoadingMode::PeerConnect(peer.id),
        };
        self.contacts.push(peer);
        self.send(cmd).await?;
        Ok(())
    }

    async fn connected(&mut self, peer_id: u16, connected: bool) -> Result<(), Box<dyn Error>> {
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
        Ok(())
    }

    fn error(&mut self, text: String) -> Result<(), Box<dyn Error>> {
        self.notification = Some((text, Instant::now()));
        if matches!(self.active_screen, Screen::LoadingScreen { .. }) {
            self.active_screen = Screen::None;
        }
        Ok(())
    }

    fn notification(&mut self, text: String) -> Result<(), Box<dyn Error>> {
        self.notification = Some((text, Instant::now()));
        Ok(())
    }

    fn peer_list(&mut self, peers: Vec<Peer>) -> Result<(), Box<dyn Error>> {
        self.contacts = peers;
        Ok(())
    }

    fn group_connected(&mut self, msg: String) -> Result<(), Box<dyn Error>> {
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
        Ok(())
    }

    fn group_list(&mut self, list: Vec<DBGroup>) -> Result<(), Box<dyn Error>> {
        self.groups = list;
        Ok(())
    }

    fn text(&mut self, sent_idx: u16, text: String) -> Result<(), Box<dyn Error>> {
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
            let new_chat = TuiChat::build(&text, "They");
            self.chats.push(new_chat);
        }
        Ok(())
    }

    fn chat_list(&mut self, peer_id: u16, chats: Vec<TuiChat>) -> Result<(), Box<dyn Error>> {
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
        Ok(())
    }

    fn group_chat_list(
        &mut self,
        group_idx: u16,
        chats: Vec<TuiChat>,
    ) -> Result<(), Box<dyn Error>> {
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
        Ok(())
    }

    fn renamed_peer(&mut self, idx: u16) -> Result<(), Box<dyn Error>> {
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
        Ok(())
    }

    fn deleted_group(&mut self) -> Result<(), Box<dyn Error>> {
        self.notification = Some(("Group deleted.".to_string(), Instant::now()));
        Ok(())
    }

    fn deleted_peer(&mut self) -> Result<(), Box<dyn Error>> {
        self.notification = Some(("Peer deleted.".to_string(), Instant::now()));
        if let Screen::ConfirmScreen { ref mode, .. } = self.active_screen
            && matches!(mode, ConfirmMode::DeletePeer)
        {
            self.active_screen = Screen::None;
        }
        Ok(())
    }
}
