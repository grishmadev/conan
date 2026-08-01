use std::{
    collections::HashMap,
    error::Error,
    sync::{Arc, RwLock},
};

use openmls::prelude::{
    DeserializeBytes, KeyPackage, MlsMessageBodyOut, MlsMessageIn, ProcessedMessageContent,
    RatchetTreeIn, Welcome, tls_codec::Serialize,
};
use rusqlite::Connection;
use tokio::sync::broadcast;

use crate::{
    comm::{
        enums::{IPCRes, from_bytes, to_bytes},
        notification::ConanNotif,
    },
    entities::{
        database::{
            chat::{Chat, ChatData},
            peer::PeerData,
        },
        server::slave::Slave,
    },
    mls::ConanGroup,
    msg::{Msg, SlaveCmd},
};

pub struct CommandHandler {
    peers: Arc<RwLock<HashMap<u8, Slave>>>,
    groups: Arc<RwLock<HashMap<u8, ConanGroup>>>,
    dbconn: Connection,
    msg_sen: broadcast::Sender<IPCRes>,
}

impl CommandHandler {
    pub fn new(
        peers: Arc<RwLock<HashMap<u8, Slave>>>,
        groups: Arc<RwLock<HashMap<u8, ConanGroup>>>,
        dbconn: Connection,
        msg_sen: broadcast::Sender<IPCRes>,
    ) -> Self {
        Self {
            peers,
            groups,
            dbconn,
            msg_sen,
        }
    }

    pub fn handle_msg_text(&self, idx: u8, text: String) -> Result<(), Box<dyn Error>> {
        let chat = Chat::chat_to_rec(&text, u32::from(idx));
        for _ in 0..3 {
            if let Err(e) = self.dbconn.insert_chat(chat.clone()) {
                println!("Error inserting chat: {e}");
            } else {
                break;
            }
        }
        self.msg_sen.send(IPCRes::Text(idx, text.clone()))?;
        let Ok(Some(target)) = self.dbconn.get_peer_from_id(u32::from(idx)) else {
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

    pub fn handle_msg_convert(&self, idx: u8, name: String) -> Result<(), Box<dyn Error>> {
        println!("got convert to group");
        let new_group = ConanGroup::build(&name).unwrap();
        let key_package = new_group.key_package_bundle("Conan").unwrap();
        let Ok(mut peers) = self.peers.write() else {
            return Err("Cannot write to peers.".into());
        };
        let Some(peer) = peers.get_mut(&idx) else {
            return Err("Cannot get selected peer.".into());
        };

        let key_package_ser = to_bytes(key_package.key_package());
        peer.command_sender
            .send(SlaveCmd::Msg(Msg::KeyPackage(key_package_ser)))
            .unwrap();
        let mut groups = self.groups.write().unwrap();
        groups.insert(0, new_group);
        Ok(())
    }

    pub fn handle_msg_keypackage(&self, idx: u8, data: Vec<u8>) -> Result<(), Box<dyn Error>> {
        println!("got keypackage");
        let key_package = from_bytes::<KeyPackage>(&data)?;
        let mut groups = self.groups.write().unwrap();
        let Some(group) = groups.get_mut(&0) else {
            return Err("No Group found at given Index".into());
        };
        let (_commit, welcome) = group.add_members(&key_package)?;

        let mut peers = self.peers.write().unwrap();
        let Some(peer) = peers.get_mut(&idx) else {
            return Err("No peer found at given Index".into());
        };
        let MlsMessageBodyOut::Welcome(welcome) = welcome.body().clone() else {
            return Err("No Welcome found. Ignoring.".into());
        };
        let tree: RatchetTreeIn = group.group.export_ratchet_tree().into();

        peer.command_sender
            .send(SlaveCmd::Msg(Msg::Welcome(welcome, tree)))?;
        Ok(())
    }

    pub fn handle_msg_welcome(
        &self,
        idx: u8,
        welcome: Welcome,
        tree: RatchetTreeIn,
    ) -> Result<(), Box<dyn Error>> {
        println!("group welcome");
        let mut peers = self.peers.write().unwrap();
        let Some(peer) = peers.get_mut(&idx) else {
            return Err("No peer found at given index".into());
        };
        if let Ok(mut groups) = self.groups.write() {
            let group = groups.get_mut(&0).unwrap();
            let mls_group = ConanGroup::join_group(&group.provider, welcome, tree).unwrap();
            group.group = mls_group;
            peer.command_sender
                .send(SlaveCmd::Msg(Msg::GroupVerified))
                .unwrap();
            group.members.insert(peer.id);
        } else {
            peer.command_sender
                .send(SlaveCmd::Msg(Msg::GroupError(
                    "Some Error Occured while joining group.".to_string(),
                )))
                .unwrap();
        }
        Ok(())
    }

    pub fn handle_msg_group_error(&self, err: String) -> Result<(), Box<dyn Error>> {
        let Ok(mut groups) = self.groups.write() else {
            return Err("Cannot write to group.".into());
        };
        let Some(group) = groups.get_mut(&0) else {
            return Err("No Group found at given index".into());
        };
        group.group.merge_pending_commit(&group.provider).unwrap();
        ConanNotif::Sys(err).notify()?;
        Ok(())
    }

    pub fn handle_msg_group_verified(&self, idx: u8) -> Result<(), Box<dyn Error>> {
        println!("group verified");
        let mut groups = self.groups.write().unwrap();
        let Some(group) = groups.get_mut(&0) else {
            return Err("Cannot find selected group".into());
        };
        group.members.insert(idx);
        group.group.merge_pending_commit(&group.provider).unwrap();
        let text = b"Hello there";
        let mut peers = self.peers.write().unwrap();
        let members = group.members.iter().collect::<Vec<_>>();
        for idx in members {
            println!("member: {idx}");
            let Some(peer) = peers.get_mut(idx) else {
                continue;
            };
            let message = group
                .group
                .create_message(&group.provider, &group.signer, text)
                .unwrap();

            let message_ser = message.tls_serialize_detached()?;
            peer.command_sender
                .send(SlaveCmd::Msg(Msg::GroupMessage(message_ser)))?;
        }
        ConanNotif::Sys("Group Action Complete.".into()).notify()?;
        Ok(())
    }

    pub fn handle_group_message(&self, message: Vec<u8>) -> Result<(), Box<dyn Error>> {
        println!("got group message");
        let (message, _) = MlsMessageIn::tls_deserialize_bytes(&message)?;
        let mut groups = self.groups.write().unwrap();
        let Some(group) = groups.get_mut(&0) else {
            return Err("Cannot find selected group".into());
        };
        let message = message.try_into_protocol_message().unwrap();
        let processed_message = group
            .group
            .process_message(&group.provider, message)
            .unwrap();
        let content = processed_message.into_content();
        if let ProcessedMessageContent::ApplicationMessage(mess) = content {
            let content = mess.into_bytes();
            let string = String::from_utf8_lossy(&content).to_string();
            println!("got: {string}");
        }
        group.group.merge_pending_commit(&group.provider).unwrap();
        Ok(())
    }

    pub fn remove_peer(&self, idx: u8) -> Result<(), Box<dyn Error>> {
        if let Ok(mut guard) = self.peers.write()
            && let Some(conn) = guard.remove(&idx)
        {
            println!("Removing Connection: {}", conn.id);
            if let Ok(Some(peer)) = self.dbconn.get_peer_from_id(u32::from(conn.id)) {
                _ = ConanNotif::Sys(format!("{} disconnected.", peer.name));
            }
        }
        Ok(())
    }
}
