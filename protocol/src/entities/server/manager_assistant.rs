use crate::{
    comm::{
        enums::{IPCRes, from_bytes, to_bytes},
        notification::ConanNotif,
    },
    database::FromConnection,
    entities::{
        database::{
            chat::{Chat, ChatData},
            group::{ConnectionGroup, DBGroup},
            member::GroupMember,
            peer::PeerData,
        },
        server::slave::Slave,
    },
    extras::{
        codec::BincodeCodec,
        crypto::{direct_decrypt, direct_encrypt},
        generate_name,
    },
    mls::ConanGroup,
    msg::{Msg, SlaveCmd},
    operations::signing_key,
};
use chacha20poly1305::{
    ChaCha20Poly1305,
    aead::{Generate, Key},
};
use openmls::prelude::{
    DeserializeBytes, KeyPackage, MlsMessageBodyOut, MlsMessageIn, ProcessedMessageContent,
    RatchetTreeIn, Welcome, tls_codec::Serialize,
};
use openmls_sqlite_storage::SqliteStorageProvider;
use rusqlite::Connection;
use std::{
    collections::HashMap,
    error::Error,
    sync::{Arc, RwLock},
};
use tokio::sync::broadcast;
pub struct CommandHandler {
    peers: Arc<RwLock<HashMap<u8, Slave>>>,
    groups: Arc<RwLock<HashMap<u8, ConanGroup>>>,
    dbconn: Connection,
    msg_sen: broadcast::Sender<IPCRes>,
    openmls_store: SqliteStorageProvider<BincodeCodec, Connection>,
    /// here `u8` refers to peers (not group) associated with the given group
    temp_group_store: Arc<RwLock<HashMap<u8, ConanGroup>>>,
    internal_key: Key<ChaCha20Poly1305>,
    mls_conn: SqliteStorageProvider<BincodeCodec, Connection>,
}
impl CommandHandler {
    pub fn new(
        peers: Arc<RwLock<HashMap<u8, Slave>>>,
        groups: Arc<RwLock<HashMap<u8, ConanGroup>>>,
        temp_groups: Arc<RwLock<HashMap<u8, ConanGroup>>>,
        dbconn: Connection,
        msg_sen: broadcast::Sender<IPCRes>,
        openmls_store: SqliteStorageProvider<BincodeCodec, Connection>,
    ) -> Self {
        let mls_conn = SqliteStorageProvider::from_db(&dbconn).unwrap();
        Self {
            peers,
            groups,
            dbconn,
            msg_sen,
            openmls_store,
            temp_group_store: temp_groups,
            internal_key: Key::<ChaCha20Poly1305>::generate(),
            mls_conn,
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

    pub fn handle_msg_convert(
        &self,
        peer_idx: u8,
        arti_path: String,
    ) -> Result<(), Box<dyn Error>> {
        println!("got convert to group");
        let peers = Arc::clone(&self.peers);
        let temp_groups = Arc::clone(&self.temp_group_store);
        let key = self.internal_key;
        tokio::spawn(async move {
            let signing_key = signing_key(arti_path).await.unwrap();
            let new_group = ConanGroup::build(&signing_key).unwrap();
            let key_package = new_group.key_package_bundle("Conan").unwrap();
            let Ok(mut peers) = peers.write() else {
                println!("Cannot write to peers.");
                return;
            };
            let Some(peer) = peers.get_mut(&peer_idx) else {
                println!("Cannot get selected peer.");
                return;
            };
            let key_package_ser = to_bytes(key_package.key_package());
            let secure_idx = direct_encrypt(&key, &(peer_idx.to_be_bytes())).unwrap();
            peer.command_sender
                .send(SlaveCmd::Msg(Msg::KeyPackage(secure_idx, key_package_ser)))
                .unwrap();
            let mut groups = temp_groups.write().unwrap();

            // inserting the group temporarily and waiting for confirmation
            println!("peer idx: {peer_idx}");
            groups.insert(peer_idx, new_group);
        });
        Ok(())
    }

    pub fn handle_msg_keypackage(
        &self,
        peer_idx: u8,
        key_idx: Vec<u8>,
        data: &[u8],
    ) -> Result<(), Box<dyn Error>> {
        println!("got keypackage");
        let key_package = from_bytes::<KeyPackage>(data)?;
        {
            let groups = self.groups.read().unwrap();
            for (idx, _) in groups.iter() {
                println!("group_idx: {idx}");
            }
        }
        let mut groups = self.temp_group_store.write().unwrap();
        for (idx, _) in groups.iter() {
            println!("temp_group_idx: {idx}, peer_idx: {peer_idx}");
        }
        let Some(group) = groups.get_mut(&peer_idx) else {
            return Err("No Group found at given Index".into());
        };
        let (_commit, welcome) = group.add_members(&key_package)?;
        let mut peers = self.peers.write().unwrap();
        let Some(peer) = peers.get_mut(&peer_idx) else {
            return Err("No peer found at given Index".into());
        };
        let MlsMessageBodyOut::Welcome(welcome) = welcome.body().clone() else {
            return Err("No Welcome found. Ignoring.".into());
        };
        let tree: RatchetTreeIn = group.group.export_ratchet_tree().into();
        peer.command_sender
            .send(SlaveCmd::Msg(Msg::Welcome(key_idx, welcome, tree)))?;
        Ok(())
    }

    pub fn handle_msg_welcome(
        &self,
        peer_idx: u8,
        grp_idx: &[u8],
        welcome: Welcome,
        tree: RatchetTreeIn,
    ) -> Result<(), Box<dyn Error>> {
        println!("group welcome");
        let mut peers = self.peers.write().unwrap();
        println!("check 00");
        let Some(peer) = peers.get_mut(&peer_idx) else {
            println!("No peer found at given index");
            return Err("No peer found at given index".into());
        };
        println!("check 0");
        let grp_idx = direct_decrypt(&self.internal_key, grp_idx)?;
        let grp_idx = u8::from_be_bytes(grp_idx.try_into().unwrap());
        println!("check 1");
        if let Ok(mut tmp_grps) = self.temp_group_store.write() {
            let tmp_grp = tmp_grps
                .get_mut(&grp_idx)
                .ok_or("Cannot get targeted peer")?;
            let mls_grp = ConanGroup::join_group(&tmp_grp.provider, welcome, tree)?;
            let grp_id = mls_grp.group_id().to_vec();
            let grp_name = generate_name(3..8);
            let dbgroup = DBGroup::new(grp_id.clone(), grp_name);
            tmp_grp.group = mls_grp;
            peer.command_sender
                .send(SlaveCmd::Msg(Msg::GroupVerified(grp_id)))
                .unwrap();
            tmp_grp.members.insert(peer.id);
            let Ok(mut grps) = self.groups.write() else {
                return Err("Cannot write to groups.".into());
            };
            let db_grp = self.dbconn.insert_group(dbgroup)?;

            // we remove from temporary group to parmanent group and insert it to database
            grps.insert(
                db_grp.id,
                tmp_grps
                    .remove(&grp_idx)
                    .ok_or("Could not remove group from temporary map.")?,
            );
        } else {
            peer.command_sender.send(SlaveCmd::Msg(Msg::GroupError(
                "Some Error Occured while joining group.".to_string(),
            )))?;
        }
        Ok(())
    }

    pub fn handle_msg_group_verified(
        &self,
        idx: u8,
        group_id: &[u8],
    ) -> Result<(), Box<dyn Error>> {
        println!("group verified");
        let mut temp_groups = self.temp_group_store.write().unwrap();
        let (_grp_idx, grp) = temp_groups
            .iter_mut()
            .find(|(_, conn)| conn.group.group_id().to_vec() == group_id)
            .ok_or("No group found in temp")?;
        grp.members.insert(idx);
        grp.group.merge_pending_commit(&grp.provider)?;
        // let group = MlsGroup::load(&self.mls_conn, &GroupId::from_slice(&group_id))?;
        // the group is already saved in initializers database
        // so it can be retrieved
        let db_group = self.dbconn.get_group_by_group_id(group_id)?;

        // Inserting member in our database
        let peer = self
            .dbconn
            .get_peer_from_id(u32::from(idx))?
            .ok_or("Unknown Peer dropping connection")?;
        self.dbconn.insert_member(db_group.id, peer, true)?;

        let text = b"Hello there";
        let mut peers = self.peers.write().unwrap();
        let members = grp.members.iter().collect::<Vec<&u8>>();
        for idx in members {
            println!("member: {idx}");
            let Some(peer) = peers.get_mut(idx) else {
                continue;
            };
            let message = grp
                .group
                .create_message(&grp.provider, &grp.signer, text)
                .unwrap();
            let message_ser = message.tls_serialize_detached()?;
            peer.command_sender.send(SlaveCmd::Msg(Msg::GroupMessage(
                grp.group.group_id().to_vec(),
                message_ser,
            )))?;
        }
        ConanNotif::Sys("Group Action Complete.".into()).notify()?;
        Ok(())
    }

    pub fn handle_msg_group_error(&self, peer_idx: u8, err: String) -> Result<(), Box<dyn Error>> {
        let Ok(mut tmp_grps) = self.temp_group_store.write() else {
            return Err("Cannot write to group.".into());
        };
        let mut removed_group = tmp_grps
            .remove(&peer_idx)
            .ok_or("Could not remove group.")?;
        removed_group
            .group
            .clear_pending_commit(&self.openmls_store)?;
        let db_group = self
            .dbconn
            .get_group_by_group_id(&removed_group.group.group_id().to_vec())?;
        if !removed_group.members.is_empty() {
            let mut groups = self.groups.write().unwrap();
            groups.insert(db_group.id, removed_group);
        }
        ConanNotif::Sys(err).notify()?;
        Ok(())
    }

    pub fn handle_group_message(
        &self,
        group_id: &[u8],
        message: &[u8],
    ) -> Result<(), Box<dyn Error>> {
        println!("got group message");
        let (message, _) = MlsMessageIn::tls_deserialize_bytes(message)?;
        let mut groups = self.groups.write().unwrap();
        let dbgroup = self.dbconn.get_group_by_group_id(group_id)?;
        let Some(group) = groups.get_mut(&dbgroup.id) else {
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
