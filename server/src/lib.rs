use conan_extras::generate_name;
use conandatabase::entities::{
    chat::{Chat, ChatData},
    group::{ConnectionGroup, DBGroup},
    group_chat::ConnectionGroupChat,
    peer::{Peer, PeerData},
};
use conanprotocol::{
    comm::enums::{
        error::ConanError,
        ipccmd::IPCCmd,
        ipcres::IPCRes,
        msg::{Msg, SlaveCmd},
    },
    entities::manager::Manager,
    mls::ConanGroup,
};
use openmls::group::MlsGroup;
use std::{
    error::Error,
    sync::{Arc, atomic::Ordering},
};
use tor_llcrypto::pk::ed25519::ExpandedKeypair;

pub mod functions;
pub struct ServerHandler {
    pub manager: Manager,
    pub signing_key: ExpandedKeypair,
}

impl ServerHandler {
    /// Setup Manager. Return Command Handler
    /// # Errors
    pub fn setup(
        mut manager: Manager,
        signing_key: ExpandedKeypair,
    ) -> Result<Self, Box<dyn Error>> {
        println!("Starting Manager..");
        manager.init_server()?;
        println!("Manager Started. Establishing Message Routes..");
        manager.setup_slave_communication()?;
        println!("All Set.");
        Ok(Self {
            manager,
            signing_key,
        })
    }

    /// Handle incoming [`IPCCmd`] commands
    /// # Errors
    pub fn handle_commands(&mut self, cmd: IPCCmd) -> Result<(), Box<dyn Error>> {
        match cmd {
            IPCCmd::Tick => {
                self.tick()?;
            }
            IPCCmd::StartServer => {
                self.start_server()?;
            }
            IPCCmd::AddPeer(addr, _) => {
                self.add_peer(&addr)?;
            }
            IPCCmd::Connect(peer_id) => {
                self.connect(peer_id)?;
            }
            IPCCmd::Text(idx, text) => {
                self.text(idx, text)?;
            }
            IPCCmd::Disconnect(idx) => {
                self.disconnect(idx)?;
            }
            IPCCmd::PeerList => {
                self.peer_list()?;
            }
            IPCCmd::DeletePeer(idx) => {
                self.delete_peer(idx)?;
            }
            IPCCmd::DeleteGroup(idx) => {
                self.delete_group(idx)?;
            }
            IPCCmd::RenamePeer(idx, new_name) => {
                self.rename_peer(idx, new_name)?;
            }
            IPCCmd::ChatList {
                peer_id,
                msg_amount,
            } => {
                self.chat_list(peer_id, msg_amount)?;
            }
            IPCCmd::NewGroup(name) => {
                self.new_group(name)?;
            }
            IPCCmd::AddToGroup(group_idx, peer_idx) => {
                self.add_to_group(group_idx, peer_idx)?;
            }
            IPCCmd::RemoveFromGroup(group_idx, peer_idx) => {
                self.remove_from_group(group_idx, peer_idx)?;
            }
            IPCCmd::RenameGroup(idx, name) => {
                self.rename_group(idx, &name)?;
            }
            IPCCmd::GroupConnect(idx) => {
                self.group_connect(idx)?;
            }
            IPCCmd::InitiateGroup(grp_id) => {
                self.initiate_group(&grp_id)?;
            }
            IPCCmd::GroupList => {
                self.group_list()?;
            }
            IPCCmd::GroupChatList {
                group_idx,
                msg_amount,
            } => {
                self.group_peer_list(group_idx, msg_amount)?;
            }
            IPCCmd::GroupText(grp_idx, text) => {
                self.group_text(grp_idx, &text)?;
            }
            // TODO: Add a trigger in TUI side
            IPCCmd::Promote(group_idx, peer_idx) => {
                self.promote(group_idx, peer_idx)?;
            }

            // TODO: Add a trigger in TUI side
            IPCCmd::Demote(group_idx, peer_idx) => {
                self.demote(group_idx, peer_idx)?;
            }
            _ => unimplemented!(),
        }
        Ok(())
    }

    fn tick(&self) -> Result<(), Box<dyn Error>> {
        self.manager.msg_sender.send(IPCRes::Tock)?;
        Ok(())
    }

    fn start_server(&self) -> Result<(), Box<dyn Error>> {
        let started = self.manager.server_ready.load(Ordering::SeqCst);
        self.manager
            .msg_sender
            .send(IPCRes::ServerStarted(started))?;
        Ok(())
    }

    fn add_peer(&self, addr: &str) -> Result<(), Box<dyn Error>> {
        let name = generate_name(3..8);
        let peer = Peer::build(&name, addr, true);
        let peer = self.manager.dbconn.insert_peer(peer)?;
        self.manager.msg_sender.send(IPCRes::AddedPeer(peer))?;
        Ok(())
    }

    fn connect(&mut self, peer_id: u16) -> Result<(), Box<dyn Error>> {
        let present_in_peers = self.manager.peers.read().unwrap().contains_key(&peer_id);
        let present_in_waitlist = self.manager.waitlist.contains(&peer_id);

        if present_in_peers {
            let res = IPCRes::Connected(peer_id, true);
            self.manager.msg_sender.send(res)?;
            self.manager.waitlist.remove(&peer_id);
        } else if present_in_waitlist {
            let res = IPCRes::Connected(peer_id, false);
            self.manager.msg_sender.send(res)?;
        } else {
            let dbpeer = self.manager.dbconn.get_peer_from_id(peer_id)?.unwrap();
            if let Err(e) = self.manager.connect_as_dialer(dbpeer.address) {
                return Err(format!("Cannot connect as Dialer:\n{e}").into());
            }
            self.manager
                .msg_sender
                .send(IPCRes::Connected(peer_id, false))?;
            self.manager.waitlist.insert(peer_id);
        }
        Ok(())
    }

    fn text(&self, idx: u16, text: String) -> Result<(), Box<dyn Error>> {
        let manager = &self.manager;
        if idx == 1 {
            let chat = Chat::chat_to_send(&text, 1);
            manager.dbconn.insert_chat(chat)?;
            manager.msg_sender.send(IPCRes::Text(idx, text))?;
            return Ok(());
        }
        let peers = Arc::clone(&manager.peers);
        if let Ok(mut peers) = peers.write() {
            let Some(target) = peers.get_mut(&idx) else {
                println!("Cannot find target peer.");
                return Ok(());
            };
            let chat = Chat::chat_to_send(&text, idx);
            manager.dbconn.insert_chat(chat)?;
            let msg = Msg::Text(text);
            target.command_sender.send(SlaveCmd::Msg(msg))?;
        }
        Ok(())
    }

    fn disconnect(&mut self, idx: u16) -> Result<(), Box<dyn Error>> {
        if idx == 1 {
            return Ok(());
        }
        if let Ok(mut peers) = self.manager.peers.write() {
            let Some(target) = peers.get_mut(&idx) else {
                eprintln!("Cannot find target peer to disconnect.");
                return Ok(());
            };
            target.command_sender.send(SlaveCmd::Shutdown)?;
        } else {
            return Err(ConanError::Locked.into());
        }
        Ok(())
    }

    fn peer_list(&self) -> Result<(), Box<dyn Error>> {
        let manager = &self.manager;
        let mut peers = manager.dbconn.list_all_peers(true)?;
        if let Ok(mem_slaves) = Arc::clone(&manager.peers).read() {
            let iter = peers.iter_mut();
            #[allow(clippy::cast_possible_truncation)]
            for p in iter {
                p.connected = mem_slaves.contains_key(&p.id);
            }
        }
        manager.msg_sender.send(IPCRes::PeerList(peers))?;
        Ok(())
    }

    fn delete_peer(&self, idx: u16) -> Result<(), Box<dyn Error>> {
        self.manager.dbconn.delete_peer(idx)?;
        self.manager.msg_sender.send(IPCRes::DeletedPeer(idx))?;
        Ok(())
    }

    fn delete_group(&self, idx: u16) -> Result<(), Box<dyn Error>> {
        #[allow(clippy::cast_possible_truncation)]
        self.manager.dbconn.delete_group(idx)?;
        self.manager.groups.write().unwrap().remove(&idx);
        self.manager.msg_sender.send(IPCRes::DeletedGroup(idx))?;
        Ok(())
    }

    fn rename_peer(&self, idx: u16, name: String) -> Result<(), Box<dyn Error>> {
        self.manager.dbconn.rename_peer(idx, name)?;
        self.manager.msg_sender.send(IPCRes::RenamedPeer(idx))?;
        Ok(())
    }

    fn chat_list(&self, peer_id: u16, msg_amount: u8) -> Result<(), Box<dyn Error>> {
        let chats = self.manager.dbconn.list_tuichat_from(peer_id, msg_amount)?;
        self.manager
            .msg_sender
            .send(IPCRes::ChatList { peer_id, chats })?;
        Ok(())
    }

    fn new_group(&self, name: Option<String>) -> Result<(), Box<dyn Error>> {
        let manager = &self.manager;
        let self_link = manager
            .dbconn
            .get_peer_from_id(1)?
            .ok_or("Cannot find self link")?
            .address;
        let new_group = MlsGroup::build(&self.signing_key, &self_link)?;
        let name = if let Some(name) = name {
            name
        } else {
            generate_name(3..8)
        };
        let dbgroup = DBGroup::new(new_group.group_id().to_vec(), name);
        manager.dbconn.insert_group(dbgroup)?;
        Ok(())
    }

    fn add_to_group(&self, group_idx: u16, peer_idx: u16) -> Result<(), Box<dyn Error>> {
        self.manager.make_peer_join_group(peer_idx, group_idx)?;
        Ok(())
    }

    fn remove_from_group(&self, group_idx: u16, peer_idx: u16) -> Result<(), Box<dyn Error>> {
        if let Ok(mut groups) = self.manager.groups.write() {
            let target_grp = groups.get_mut(&group_idx).unwrap();
            self.manager.remove_member(target_grp, peer_idx)?;
        }
        Ok(())
    }

    fn rename_group(&self, idx: u16, name: &str) -> Result<(), Box<dyn Error>> {
        self.manager.dbconn.rename_group(idx, name)?;
        self.manager.msg_sender.send(IPCRes::RenamedGroup(idx))?;
        Ok(())
    }

    fn group_connect(&mut self, idx: u16) -> Result<(), Box<dyn Error>> {
        let mut group = self.manager.get_mls_group_from_idx(idx)?;
        self.manager.connect_to_group(&mut group)?;
        self.manager.groups.write().unwrap().insert(idx, group);
        Ok(())
    }

    fn initiate_group(&mut self, grp_id: &[u8]) -> Result<(), Box<dyn Error>> {
        let dbgroup = self.manager.dbconn.get_group_by_group_id(grp_id)?;
        let mut mlsgrp = self.manager.get_mls_group_from_idx(dbgroup.id)?;
        self.manager.connect_to_group(&mut mlsgrp)?;
        if let Ok(mut groups) = self.manager.groups.write() {
            groups.insert(dbgroup.id, mlsgrp);
        } else {
            return Err(ConanError::ParseError.into());
        }
        Ok(())
    }

    fn group_list(&self) -> Result<(), Box<dyn Error>> {
        let manager = &self.manager;
        let mut groups = manager.dbconn.list_groups()?;
        if let Ok(grouplist) = Arc::clone(&manager.groups).read() {
            let groups = groups.iter_mut();
            for g in groups {
                if grouplist.contains_key(&g.id) {
                    g.connected = true;
                }
            }
        }
        manager.msg_sender.send(IPCRes::GroupList(groups))?;
        Ok(())
    }

    fn group_peer_list(&self, group_idx: u16, msg_amount: u8) -> Result<(), Box<dyn Error>> {
        let chats = self
            .manager
            .dbconn
            .get_tuichats_by_group_idx(group_idx, msg_amount)?;
        self.manager
            .msg_sender
            .send(IPCRes::GroupChatList { group_idx, chats })?;
        Ok(())
    }

    fn group_text(&self, grp_idx: u16, text: &str) -> Result<(), Box<dyn Error>> {
        let mut groups = self.manager.groups.write().unwrap();
        let Some(group) = groups.get_mut(&grp_idx) else {
            return Ok(());
        };
        self.manager.send_text(group, text)?;
        Ok(())
    }

    fn promote(&self, group_idx: u16, peer_idx: u16) -> Result<(), Box<dyn Error>> {
        if let Ok(mut groups) = self.manager.groups.write() {
            let group = groups.get_mut(&group_idx).ok_or(ConanError::NotFound)?;
            self.manager.promote_member(group, peer_idx)?;
            return Ok(());
        }
        Err(ConanError::Locked.into())
    }

    fn demote(&self, group_idx: u16, peer_idx: u16) -> Result<(), Box<dyn Error>> {
        if let Ok(mut groups) = self.manager.groups.write() {
            let group = groups.get_mut(&group_idx).ok_or(ConanError::NotFound)?;
            self.manager.demote_member(group, peer_idx)?;
            return Ok(());
        }
        Err(ConanError::Locked.into())
    }
}
