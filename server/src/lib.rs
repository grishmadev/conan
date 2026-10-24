use conan_extras::{generate_name, logs::Log};
use conandatabase::entities::{
    chat::{Chat, ChatData},
    group::{ConnectionGroup, DBGroup},
    group_chat::ConnectionGroupChat,
    member::GroupMember,
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

/// Command handler of the Conan server.
///
/// Dispatches the [`IPCCmd`]s received from the TUI to the [`Manager`] and
/// relays the outcome back to the TUI using [`IPCRes`] messages.
pub struct ServerHandler {
    /// Manager holding the peer, group and database state.
    pub manager: Manager,
    /// Identity key used for signing the MLS group messages.
    pub signing_key: ExpandedKeypair,
}

impl ServerHandler {
    /// Sets up the [`Manager`] and returns a ready to use command handler.
    ///
    /// Initializes the server, establishes the message routes and sets up the
    /// communication pipeline used by the connected peers.
    ///
    /// # Errors
    ///
    /// Returns an error if the manager cannot be initialized or if the peer
    /// communication pipeline cannot be established.
    ///
    /// # Panics
    ///
    /// Panics if the given manager has already been initialized.
    pub fn setup(
        mut manager: Manager,
        signing_key: ExpandedKeypair,
    ) -> Result<Self, Box<dyn Error>> {
        manager.init_server()?;
        Log::info("Manager Started. Establishing Message Routes..");
        manager.setup_slave_communication()?;
        Log::info("All Set.");
        Ok(Self {
            manager,
            signing_key,
        })
    }

    /// Handles an incoming [`IPCCmd`] and dispatches it to the matching action.
    ///
    /// Each command is forwarded to its dedicated handler which performs the
    /// requested action and replies with the appropriate [`IPCRes`].
    ///
    /// # Errors
    ///
    /// Returns an error if the dispatched action cannot be completed.
    ///
    /// # Panics
    ///
    /// Panics if an unsupported command is received.
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
            IPCCmd::GroupInfo {
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

    /// Answers a [`IPCCmd::Tick`] with an [`IPCRes::Tock`].
    ///
    /// Used by the TUI to keep the connection with the server alive.
    ///
    /// # Errors
    ///
    /// Returns an error if the response cannot be sent to the TUI.
    fn tick(&self) -> Result<(), Box<dyn Error>> {
        self.manager.msg_sender.send(IPCRes::Tock)?;
        Ok(())
    }

    /// Reports the current readiness state of the server.
    ///
    /// Replies with an [`IPCRes::ServerStarted`] carrying the value of the
    /// `server_ready` flag of the [`Manager`].
    ///
    /// # Errors
    ///
    /// Returns an error if the response cannot be sent to the TUI.
    fn start_server(&self) -> Result<(), Box<dyn Error>> {
        let started = self.manager.server_ready.load(Ordering::SeqCst);
        self.manager
            .msg_sender
            .send(IPCRes::ServerStarted(started))?;
        Ok(())
    }

    /// Adds a new peer to the local database.
    ///
    /// A random name is generated for the peer, which is then stored as a
    /// friend and the resulting [`Peer`] is reported back to the TUI with an
    /// [`IPCRes::AddedPeer`].
    ///
    /// # Errors
    ///
    /// Returns an error if the peer cannot be inserted into the database or if
    /// the response cannot be sent to the TUI.
    fn add_peer(&self, addr: &str) -> Result<(), Box<dyn Error>> {
        let name = generate_name(3..8);
        let peer = Peer::build(&name, addr, true);
        let peer = self.manager.dbconn.insert_peer(peer)?;
        self.manager.msg_sender.send(IPCRes::AddedPeer(peer))?;
        Ok(())
    }

    /// Connects to the peer with the given `peer_id`.
    ///
    /// If the peer is already connected, the connection is confirmed and the
    /// peer is removed from the waitlist. If a connection attempt is already in
    /// progress, the pending state is reported. Otherwise the peer is dialed
    /// and added to the waitlist until the connection is established.
    ///
    /// # Errors
    ///
    /// Returns an error if the peer cannot be found in the database, if the
    /// dialer cannot be started or if the response cannot be sent to the TUI.
    ///
    /// # Panics
    ///
    /// Panics if the peer is not present in the database or if the list of the
    /// active peers is locked.
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

    /// Sends a text message to the peer with the given `idx`.
    ///
    /// If `idx` is 1, the text is a note to self and is only stored locally.
    /// Otherwise the text is stored in the database and forwarded to the peer
    /// with the given `idx`.
    ///
    /// # Errors
    ///
    /// Returns an error if the message cannot be stored in the database or if
    /// it cannot be delivered to the target peer.
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
                Log::err("Cannot find target peer.");
                return Ok(());
            };
            let chat = Chat::chat_to_send(&text, idx);
            manager.dbconn.insert_chat(chat)?;
            let msg = Msg::Text(text);
            target.command_sender.send(SlaveCmd::Msg(msg))?;
        }
        Ok(())
    }

    /// Disconnects from the peer with the given `idx`.
    ///
    /// A `idx` of 1 refers to self and is ignored. For every other peer a
    /// [`SlaveCmd::Shutdown`] is sent to the peer to close the connection.
    ///
    /// # Errors
    ///
    /// Returns an error if the peer list is locked or if the shutdown command
    /// cannot be delivered to the target peer.
    fn disconnect(&mut self, idx: u16) -> Result<(), Box<dyn Error>> {
        if idx == 1 {
            return Ok(());
        }
        if let Ok(mut peers) = self.manager.peers.write() {
            let Some(target) = peers.get_mut(&idx) else {
                Log::err("Cannot find target peer to disconnect.");
                return Ok(());
            };
            target.command_sender.send(SlaveCmd::Shutdown)?;
        } else {
            return Err(ConanError::Locked.into());
        }
        Ok(())
    }

    /// Sends the list of all known peers to the TUI.
    ///
    /// Only the friends are listed and the `connected` flag of every peer is
    /// refreshed from the currently active peers before the list is reported
    /// back with an [`IPCRes::PeerList`].
    ///
    /// # Errors
    ///
    /// Returns an error if the peers cannot be listed from the database or if
    /// the response cannot be sent to the TUI.
    ///
    /// # Panics
    ///
    /// Panics if the list of the active peers is locked.
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

    /// Deletes the peer with the given `idx` from the local database.
    ///
    /// The deletion is confirmed to the TUI with an [`IPCRes::DeletedPeer`].
    ///
    /// # Errors
    ///
    /// Returns an error if the peer cannot be deleted from the database or if
    /// the response cannot be sent to the TUI.
    fn delete_peer(&self, idx: u16) -> Result<(), Box<dyn Error>> {
        self.manager.dbconn.delete_peer(idx)?;
        self.manager.msg_sender.send(IPCRes::DeletedPeer(idx))?;
        Ok(())
    }

    /// Deletes the group with the given `idx`.
    ///
    /// The group is removed from the database and, if it is loaded in memory,
    /// the MLS group is deleted from the storage as well. The deletion is
    /// confirmed to the TUI with an [`IPCRes::DeletedGroup`].
    ///
    /// # Errors
    ///
    /// Returns an error if the group cannot be deleted from the database, if
    /// the MLS group cannot be deleted from the storage or if the response
    /// cannot be sent to the TUI.
    ///
    /// # Panics
    ///
    /// Panics if the list of the active groups is locked.
    fn delete_group(&self, idx: u16) -> Result<(), Box<dyn Error>> {
        #[allow(clippy::cast_possible_truncation)]
        self.manager.dbconn.delete_group(idx)?;
        if let Some(mut group) = self.manager.groups.write().unwrap().remove(&idx) {
            group.delete(self.manager.provider.storage())?;
        }
        self.manager.msg_sender.send(IPCRes::DeletedGroup(idx))?;
        Ok(())
    }

    /// Renames the peer with the given `idx`.
    ///
    /// The new name is stored in the database and the rename is confirmed to
    /// the TUI with an [`IPCRes::RenamedPeer`].
    ///
    /// # Errors
    ///
    /// Returns an error if the peer cannot be renamed in the database or if the
    /// response cannot be sent to the TUI.
    fn rename_peer(&self, idx: u16, name: String) -> Result<(), Box<dyn Error>> {
        self.manager.dbconn.rename_peer(idx, name)?;
        self.manager.msg_sender.send(IPCRes::RenamedPeer(idx))?;
        Ok(())
    }

    /// Sends the chat history with the peer `peer_id` to the TUI.
    ///
    /// At most `msg_amount` of the most recent messages are fetched from the
    /// database and reported back with an [`IPCRes::ChatList`].
    ///
    /// # Errors
    ///
    /// Returns an error if the chats cannot be fetched from the database or if
    /// the response cannot be sent to the TUI.
    fn chat_list(&self, peer_id: u16, msg_amount: u8) -> Result<(), Box<dyn Error>> {
        let chats = self.manager.dbconn.list_tuichat_from(peer_id, msg_amount)?;
        self.manager
            .msg_sender
            .send(IPCRes::ChatList { peer_id, chats })?;
        Ok(())
    }

    /// Creates a new MLS group.
    ///
    /// The group is built with the identity key of the server and the address
    /// of self as the initial member. If no `name` is provided, a random one
    /// is generated. The group is then stored in the local database.
    ///
    /// # Errors
    ///
    /// Returns an error if the self peer cannot be found in the database, if
    /// the MLS group cannot be built or if the group cannot be stored in the
    /// database.
    fn new_group(&self, name: Option<String>) -> Result<(), Box<dyn Error>> {
        let manager = &self.manager;
        let me = manager
            .dbconn
            .get_peer_from_id(1)?
            .ok_or("Cannot find self link")?;
        let new_group = MlsGroup::build(&self.signing_key, &me.address)?;
        let name = if let Some(name) = name {
            name
        } else {
            generate_name(3..8)
        };
        let dbgroup = DBGroup::new(new_group.group_id().to_vec(), name);
        let dbgroup = manager.dbconn.insert_group(dbgroup)?;
        // adding self as admin because self created group
        // and is the first member
        let me = manager
            .dbconn
            .get_peer_from_id(1)?
            .ok_or(ConanError::NotFound("Self".to_string()))?;
        manager.dbconn.insert_member(dbgroup.id, me, true)?;
        Ok(())
    }

    /// Adds the peer `peer_idx` to the group `group_idx`.
    ///
    /// The peer is asked to join the group and the group is loaded in memory
    /// so that it can be used right away. If the peer is not connected, an
    /// [`IPCRes::Error`] is reported back to the TUI.
    ///
    /// # Errors
    ///
    /// Returns an error if the peer cannot be invited to the group.
    ///
    /// # Panics
    ///
    /// Panics if the list of the active peers is locked.
    fn add_to_group(&self, group_idx: u16, peer_idx: u16) -> Result<(), Box<dyn Error>> {
        if !self.manager.peers.read().unwrap().contains_key(&peer_idx) {
            self.manager
                .msg_sender
                .send(IPCRes::Error("Peer not conneced.".into()))?;
            return Ok(());
        }
        self.manager.make_peer_join_group(peer_idx, group_idx)?;
        Ok(())
    }

    /// Removes the peer `peer_idx` from the group `group_idx`.
    ///
    /// The removal is committed to the MLS group and broadcast to all the
    /// members of the group.
    ///
    /// # Errors
    ///
    /// Returns an error if the member cannot be removed from the group.
    ///
    /// # Panics
    ///
    /// Panics if the group is not loaded in memory.
    fn remove_from_group(&self, group_idx: u16, peer_idx: u16) -> Result<(), Box<dyn Error>> {
        if let Ok(mut groups) = self.manager.groups.write() {
            let target_grp = groups.get_mut(&group_idx).unwrap();
            self.manager.remove_member(target_grp, peer_idx)?;
        }
        Ok(())
    }

    /// Renames the group with the given `idx`.
    ///
    /// The new name is stored in the database and the rename is confirmed to
    /// the TUI with an [`IPCRes::RenamedGroup`].
    ///
    /// # Errors
    ///
    /// Returns an error if the group cannot be renamed in the database or if
    /// the response cannot be sent to the TUI.
    fn rename_group(&self, idx: u16, name: &str) -> Result<(), Box<dyn Error>> {
        self.manager.dbconn.rename_group(idx, name)?;
        self.manager.msg_sender.send(IPCRes::RenamedGroup(idx))?;
        Ok(())
    }

    /// Connects to the group with the given `idx`.
    ///
    /// The MLS group is loaded from the storage, a connection is established
    /// with every member of the group and the group is kept in memory for
    /// later use.
    ///
    /// # Errors
    ///
    /// Returns an error if the group cannot be loaded or if the connection to
    /// the members of the group cannot be established.
    ///
    /// # Panics
    ///
    /// Panics if the list of the active groups is locked.
    fn group_connect(&mut self, idx: u16) -> Result<(), Box<dyn Error>> {
        let mut group = self.manager.get_mls_group_from_idx(idx)?;
        self.manager.connect_to_group(&mut group)?;
        self.manager.groups.write().unwrap().insert(idx, group);
        Ok(())
    }

    /// Initiates the group identified by the MLS group id `grp_id`.
    ///
    /// The group is looked up in the database, loaded from the storage and
    /// connected to before it is kept in memory for later use.
    ///
    /// # Errors
    ///
    /// Returns an error if the group cannot be found in the database, if it
    /// cannot be loaded or if the connection to the members of the group
    /// cannot be established.
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

    /// Sends the list of all known groups to the TUI.
    ///
    /// The `connected` flag of every group is refreshed from the groups
    /// currently loaded in memory before the list is reported back with an
    /// [`IPCRes::GroupList`].
    ///
    /// # Errors
    ///
    /// Returns an error if the groups cannot be listed from the database or if
    /// the response cannot be sent to the TUI.
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

    /// Sends the details of the group `group_idx` to the TUI.
    ///
    /// At most `msg_amount` of the most recent messages and the list of the
    /// members of the group are fetched and reported back with an
    /// [`IPCRes::GroupInfo`].
    ///
    /// # Errors
    ///
    /// Returns an error if the chats or the members cannot be fetched from the
    /// database or if the response cannot be sent to the TUI.
    fn group_peer_list(&self, group_idx: u16, msg_amount: u8) -> Result<(), Box<dyn Error>> {
        let chats = self
            .manager
            .dbconn
            .get_tuichats_by_group_idx(group_idx, msg_amount)?;
        let members = self.manager.dbconn.list_members(group_idx)?;
        self.manager.msg_sender.send(IPCRes::GroupInfo {
            group_idx,
            chats,
            members,
        })?;
        Ok(())
    }

    /// Sends a text message to the group `grp_idx`.
    ///
    /// The message is encrypted for the group and sent to all of its members.
    /// If the group is not loaded in memory, the message is silently dropped.
    ///
    /// # Errors
    ///
    /// Returns an error if the message cannot be encrypted for the group or if
    /// it cannot be delivered to the members of the group.
    ///
    /// # Panics
    ///
    /// Panics if the list of the active groups is locked.
    fn group_text(&self, grp_idx: u16, text: &str) -> Result<(), Box<dyn Error>> {
        let mut groups = self.manager.groups.write().unwrap();
        let Some(group) = groups.get_mut(&grp_idx) else {
            return Ok(());
        };
        self.manager.send_text(group, text)?;
        Ok(())
    }

    /// Promotes the peer `peer_idx` to an admin of the group `group_idx`.
    ///
    /// The promotion is committed to the MLS group and broadcast to all the
    /// members of the group.
    ///
    /// # Errors
    ///
    /// Returns an error if the group is not loaded in memory or if the member
    /// cannot be promoted.
    fn promote(&self, group_idx: u16, peer_idx: u16) -> Result<(), Box<dyn Error>> {
        if let Ok(mut groups) = self.manager.groups.write() {
            let group = groups
                .get_mut(&group_idx)
                .ok_or(ConanError::NotFound("Group".into()))?;
            self.manager.promote_member(group, peer_idx)?;
            return Ok(());
        }
        Err(ConanError::Locked.into())
    }

    /// Demotes the admin `peer_idx` to a normal member of the group `group_idx`.
    ///
    /// The demotion is committed to the MLS group and broadcast to all the
    /// members of the group.
    ///
    /// # Errors
    ///
    /// Returns an error if the group is not loaded in memory or if the admin
    /// cannot be demoted.
    fn demote(&self, group_idx: u16, peer_idx: u16) -> Result<(), Box<dyn Error>> {
        if let Ok(mut groups) = self.manager.groups.write() {
            let group = groups
                .get_mut(&group_idx)
                .ok_or(ConanError::NotFound("Group".into()))?;
            self.manager.demote_member(group, peer_idx)?;
            return Ok(());
        }
        Err(ConanError::Locked.into())
    }
}
