use crate::{
    comm::{
        enums::{IPCCmd, IPCRes, from_bytes, to_bytes},
        error::ConanError,
        notification::ConanNotif,
    },
    entities::slave::Slave,
    extras::mls_provider::ConanMlsProvider,
    mls::{ConanGroup, ConanGroupError},
    msg::{Internal, Msg, SlaveCmd},
};
use database::{
    FromConnection,
    entities::{
        chat::{Chat, ChatData},
        group::{ConnectionGroup, DBGroup},
        group_chat::{ConnectionGroupChat, GroupChat},
        member::GroupMember,
        peer::PeerData,
    },
    rusqlite::Connection,
};
use extras::{codec::JsonCodec, generate_name};
use openmls::{
    group::{GroupId, MlsGroup},
    prelude::{
        DeserializeBytes, KeyPackage, MlsMessageBodyOut, MlsMessageIn, ProcessedMessageContent,
        Welcome, tls_codec::Serialize,
    },
};
use openmls_basic_credential::SignatureKeyPair;
use openmls_sqlite_storage::{Codec, SqliteStorageProvider};
use std::{
    collections::HashMap,
    error::Error,
    sync::{Arc, RwLock},
};
use tokio::sync::broadcast;
use tor_llcrypto::pk::ed25519::ExpandedKeypair;
pub struct CommandHandler<C = JsonCodec>
where
    C: Codec,
{
    peers: Arc<RwLock<HashMap<u16, Slave>>>,
    groups: Arc<RwLock<HashMap<u16, MlsGroup>>>,
    dbconn: Connection,
    msg_sen: broadcast::Sender<IPCRes>,
    openmls_store: SqliteStorageProvider<C, Connection>,
    /// here `u8` refers to peers associated with the given group id before joining
    invitation_memory: Arc<RwLock<HashMap<u16, Vec<u8>>>>,
    identity_key: ExpandedKeypair,
    provider: ConanMlsProvider,
    signer: SignatureKeyPair,
    asst_sndr: std::sync::mpsc::Sender<Internal>,
}
impl CommandHandler {
    pub fn new(
        peers: Arc<RwLock<HashMap<u16, Slave>>>,
        groups: Arc<RwLock<HashMap<u16, MlsGroup>>>,
        invitation_memory: Arc<RwLock<HashMap<u16, Vec<u8>>>>,
        dbconn: Connection,
        msg_sen: broadcast::Sender<IPCRes>,
        expanded_key: ExpandedKeypair,
        provider: ConanMlsProvider,
        sndr: std::sync::mpsc::Sender<Internal>,
    ) -> Self {
        let openmls_store = SqliteStorageProvider::from_db(&dbconn).unwrap();
        let (signer, _, _) = MlsGroup::signer_from_expanded_key(&expanded_key);
        Self {
            peers,
            groups,
            dbconn,
            msg_sen,
            openmls_store,
            invitation_memory,
            identity_key: expanded_key,
            provider,
            signer,
            asst_sndr: sndr,
        }
    }
    pub fn handle_msg_text(&self, idx: u16, text: String) -> Result<(), Box<dyn Error>> {
        let chat = Chat::chat_to_rec(&text, idx);
        for _ in 0..3 {
            if let Err(e) = self.dbconn.insert_chat(chat.clone()) {
                println!("Error inserting chat: {e}");
            } else {
                break;
            }
        }
        self.msg_sen.send(IPCRes::Text(idx, text.clone()))?;
        let Ok(Some(target)) = self.dbconn.get_peer_from_id(idx) else {
            _ = self
                .msg_sen
                .send(IPCRes::Error("Cannot find peer in database.".into()));
            return Err("Did not find connection.".into());
        };
        ConanNotif::Text(target.name, text).notify()?;
        Ok(())
    }

    pub fn handle_msg_verified(&self) -> Result<(), Box<dyn Error>> {
        self.msg_sen
            .send(IPCRes::Notification("Verified.".into()))?;
        ConanNotif::Sys("Peer Verified".into()).notify()?;
        Ok(())
    }

    pub fn handle_msg_convert(&self, peer_idx: u16) -> Result<(), Box<dyn Error>> {
        println!("got convert to group");
        let (signer, _, _) = MlsGroup::signer_from_expanded_key(&self.identity_key);
        let peers = Arc::clone(&self.peers);
        let provider = ConanMlsProvider::new(&self.dbconn)?;
        let self_link = self.dbconn.get_peer_from_id(1)?.unwrap().address;
        tokio::spawn(async move {
            let key_package = MlsGroup::key_package_bundle(&signer, &provider, &self_link).unwrap();
            let Ok(mut peers) = peers.write() else {
                println!("Cannot write to peers.");
                return;
            };
            let Some(peer) = peers.get_mut(&peer_idx) else {
                println!("Cannot get selected peer.");
                return;
            };
            let key_package_ser = to_bytes(key_package.key_package());
            peer.command_sender
                .send(SlaveCmd::Msg(Msg::KeyPackage(key_package_ser)))
                .unwrap();
            println!("peer idx: {peer_idx}");
        });
        Ok(())
    }

    pub fn handle_msg_keypackage(&self, peer_idx: u16, data: &[u8]) -> Result<(), Box<dyn Error>> {
        println!("got keypackage");
        let key_package = from_bytes::<KeyPackage>(data)?;
        {
            let groups = self.groups.read().unwrap();
            for (idx, _) in groups.iter() {
                println!("group_idx: {idx}");
            }
        }
        let invitation = self.invitation_memory.write().unwrap();

        for (idx, _) in invitation.iter() {
            println!("invite: {idx}, peer_idx: {peer_idx}");
        }

        let Some(group_id) = invitation.get(&peer_idx) else {
            return Err(ConanGroupError::NotFound.into());
        };
        let mut groups = self.groups.write().unwrap();
        let target_group = if let Some((_, target_group)) = groups
            .iter_mut()
            .find(|f| f.1.group_id().as_slice() == group_id)
        {
            target_group
        } else {
            println!("Group not in memory, extracting from Database.");
            self.asst_sndr
                .send(Internal::IPCCmd(IPCCmd::InitiateGroup(group_id.clone())))?;
            let grp = MlsGroup::load(&self.openmls_store, &GroupId::from_slice(group_id))?
                .ok_or(ConanGroupError::NotFound)?;
            let dbgrp = self.dbconn.get_group_by_group_id(group_id)?;
            groups.insert(dbgrp.id, grp);
            let Some((_, target_group)) = groups
                .iter_mut()
                .find(|f| f.1.group_id().as_slice() == group_id)
            else {
                return Err(ConanGroupError::NotFound.into());
            };
            target_group
        };
        let keypackages = core::slice::from_ref(&key_package);
        let (commit, welcome, _) =
            target_group.add_members(&self.provider, &self.signer, keypackages)?;
        let mut peers = self.peers.write().unwrap();
        let Some(peer) = peers.get_mut(&peer_idx) else {
            return Err("No peer found at given Index".into());
        };
        let MlsMessageBodyOut::Welcome(welcome) = welcome.body().clone() else {
            return Err("No Welcome found. Ignoring.".into());
        };
        peer.command_sender
            .send(SlaveCmd::Msg(Msg::Welcome(welcome)))?;
        let members = target_group.get_members()?;
        for m in &members {
            let dbpeer = self
                .dbconn
                .get_peer_from_addr(m)?
                .ok_or(ConanGroupError::NotFound)?;
            if let Some(peer) = peers.get(&dbpeer.id) {
                peer.command_sender.send(SlaveCmd::Msg(Msg::GroupMessage(
                    target_group.group_id().to_vec(),
                    commit.tls_serialize_detached()?,
                )))?;
            }
        }
        Ok(())
    }

    pub fn handle_msg_welcome(
        &self,
        peer_idx: u16,
        welcome: Welcome,
    ) -> Result<(), Box<dyn Error>> {
        println!("group welcome");
        let mut peers = self.peers.write().unwrap();
        let Some(peer) = peers.get_mut(&peer_idx) else {
            return Err(ConanError::NotFound.into());
        };
        let mls_grp = MlsGroup::join_group(&self.provider, welcome)?;
        mls_grp.members().for_each(|m| {
            let data = m.credential.serialized_content().to_vec();
            let str = from_bytes::<String>(&data).unwrap();
            println!("mls group member: {str}");
        });
        let grp_id = mls_grp.group_id().to_vec();

        let dbgroup = DBGroup::new(grp_id.clone(), generate_name(3..8));

        peer.command_sender
            .send(SlaveCmd::Msg(Msg::GroupVerified(grp_id)))?;

        let db_grp = self.dbconn.insert_group(dbgroup)?;
        self.asst_sndr
            .send(Internal::IPCCmd(IPCCmd::GroupConnect(db_grp.id)))?;
        Ok(())
    }

    pub fn handle_msg_group_verified(
        &self,
        peer_idx: u16,
        group_id: &[u8],
    ) -> Result<(), Box<dyn Error>> {
        println!("group verified");
        {
            // removing from invitation memory, because the exchange is complete
            let mut invitation = self.invitation_memory.write().unwrap();
            let inv = invitation.remove(&peer_idx);
            println!("removed {peer_idx}: {inv:?}");
        }
        let mut groups = self.groups.write().unwrap();
        let Some((_grp_idx, grp)) = groups
            .iter_mut()
            .find(|g| g.1.group_id().as_slice() == group_id)
        else {
            return Err("Cannot find targetted group".into());
        };
        grp.merge_pending_commit(&self.provider)?;
        // the group is already saved in initializers database
        // so it can be retrieved
        let db_group = self.dbconn.get_group_by_group_id(group_id)?;

        // Inserting member in our database
        let peer = self
            .dbconn
            .get_peer_from_id(peer_idx)?
            .ok_or("Unknown Peer dropping connection")?;
        self.dbconn.insert_member(db_group.id, peer, true)?;

        let text = b"Hello there";
        let mut peers = self.peers.write().unwrap();
        let members = grp.members().collect::<Vec<_>>();
        for m in members {
            let link = from_bytes::<String>(m.credential.serialized_content())?;
            let peer = self.dbconn.get_peer_from_addr(&link)?.unwrap();
            if peer.id == 1 {
                continue;
            }
            let Some(peer) = peers.get_mut(&peer.id) else {
                continue;
            };
            let message = grp
                .create_message(&self.provider, &self.signer, text)
                .unwrap();
            let message_ser = message.tls_serialize_detached()?;
            peer.command_sender.send(SlaveCmd::Msg(Msg::GroupMessage(
                grp.group_id().to_vec(),
                message_ser,
            )))?;
        }
        ConanNotif::Sys("Group Action Complete.".into()).notify()?;
        Ok(())
    }

    pub fn handle_msg_group_error(&self, peer_idx: u16, err: String) -> Result<(), Box<dyn Error>> {
        let Ok(mut invitation) = self.invitation_memory.write() else {
            return Err("Cannot write to group.".into());
        };
        let group_id = invitation
            .remove(&peer_idx)
            .ok_or("Could not remove group.")?;
        let group_id = GroupId::from_slice(&group_id);
        let mut groups = self.groups.write().unwrap();
        let Some((_grp_idx, group)) = groups.iter_mut().find(|f| f.1.group_id() == &group_id)
        else {
            return Err("Could not find targetted group".into());
        };
        group.clear_pending_commit(&self.openmls_store)?;
        let db_group = self.dbconn.get_group_by_group_id(&group_id.to_vec())?;
        if !group.members().collect::<Vec<_>>().is_empty() {
            groups.remove(&db_group.id);
        }
        ConanNotif::Sys(err).notify()?;
        Ok(())
    }

    pub fn handle_group_message(
        &self,
        peer_idx: u16,
        group_id: &[u8],
        message: &[u8],
    ) -> Result<(), Box<dyn Error>> {
        let (message, _) = MlsMessageIn::tls_deserialize_bytes(message)?;
        let mut groups = self.groups.write().unwrap();
        let dbgroup = self.dbconn.get_group_by_group_id(group_id)?;
        let Some(group) = groups.get_mut(&dbgroup.id) else {
            return Err(ConanError::NotFound.into());
        };
        let message = message.try_into_protocol_message().unwrap();
        let processed_message = group.process_message(&self.provider, message).unwrap();
        let content = processed_message.into_content();
        match content {
            ProcessedMessageContent::ApplicationMessage(mess) => {
                let content = mess.into_bytes();
                let data = String::from_utf8_lossy(&content).to_string();
                let gc = GroupChat {
                    id: 0,
                    group_id: dbgroup.id,
                    sender_id: peer_idx,
                    data,
                    time: String::new(),
                };
                self.dbconn.insert_group_chat(gc)?;
            }
            ProcessedMessageContent::StagedCommitMessage(msg) => {
                group.merge_staged_commit(&self.provider, *msg)?;
            }
            _ => {}
        }
        Ok(())
    }

    pub fn handle_initiate_group(&self, group_id: Vec<u8>) -> Result<(), Box<dyn Error>> {
        self.asst_sndr
            .send(Internal::IPCCmd(IPCCmd::InitiateGroup(group_id)))?;
        Ok(())
    }

    pub fn remove_peer(&self, idx: u16) -> Result<(), Box<dyn Error>> {
        if let Ok(mut guard) = self.peers.write()
            && let Some(conn) = guard.remove(&idx)
        {
            println!("Removing Connection: {}", conn.id);
            if let Ok(Some(peer)) = self.dbconn.get_peer_from_id(conn.id) {
                _ = ConanNotif::Sys(format!("{} disconnected.", peer.name));
            }
        }
        let peers = self.peers.write().unwrap();
        let mut groups_to_kick = vec![];
        if let Ok(groups) = self.groups.read() {
            let group_vec = groups.iter().collect::<Vec<_>>();
            for (idx, grp) in &group_vec {
                let mut members = grp.get_members()?;
                for idx in 0..members.len() {
                    let m = &members[idx];
                    let peer = self.dbconn.get_peer_from_addr(m)?.unwrap();
                    if peers.get(&peer.id).is_some() {
                        members.remove(idx);
                    }
                }
                if !members.is_empty() {
                    groups_to_kick.push(**idx);
                }
            }
        }
        let mut groups = self.groups.write().unwrap();
        for idx in groups_to_kick {
            groups.remove(&idx);
        }
        Ok(())
    }
}
