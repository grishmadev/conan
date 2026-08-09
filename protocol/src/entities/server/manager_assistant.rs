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
            group_chat::{ConnectionGroupChat, GroupChat},
            member::GroupMember,
            peer::PeerData,
        },
        server::slave::Slave,
    },
    extras::{codec::BincodeCodec, generate_name, mls_provider::ConanMlsProvider},
    mls::ConanGroup,
    msg::{Msg, SlaveCmd},
    operations::signing_key,
};
use openmls::{
    group::GroupId,
    prelude::{
        DeserializeBytes, KeyPackage, MlsMessageBodyOut, MlsMessageIn, ProcessedMessageContent,
        RatchetTreeIn, SignatureScheme, Welcome, tls_codec::Serialize,
    },
};
use openmls_basic_credential::SignatureKeyPair;
use openmls_sqlite_storage::SqliteStorageProvider;
use rusqlite::Connection;
use std::{
    collections::HashMap,
    error::Error,
    sync::{Arc, RwLock},
};
use tokio::sync::broadcast;
use tor_llcrypto::pk::ed25519::ExpandedKeypair;
pub struct CommandHandler {
    peers: Arc<RwLock<HashMap<u8, Slave>>>,
    groups: Arc<RwLock<HashMap<u8, ConanGroup>>>,
    dbconn: Connection,
    msg_sen: broadcast::Sender<IPCRes>,
    openmls_store: SqliteStorageProvider<BincodeCodec, Connection>,
    /// here `u8` refers to peers associated with the given group id before joining
    invitation_memory: Arc<RwLock<HashMap<u8, Vec<u8>>>>,
    identity_key: ExpandedKeypair,
    provider: ConanMlsProvider,
}
impl CommandHandler {
    pub fn new(
        peers: Arc<RwLock<HashMap<u8, Slave>>>,
        groups: Arc<RwLock<HashMap<u8, ConanGroup>>>,
        invitation_memory: Arc<RwLock<HashMap<u8, Vec<u8>>>>,
        dbconn: Connection,
        msg_sen: broadcast::Sender<IPCRes>,
        expanded_key: ExpandedKeypair,
        provider: ConanMlsProvider,
    ) -> Self {
        let openmls_store = SqliteStorageProvider::from_db(&dbconn).unwrap();
        Self {
            peers,
            groups,
            dbconn,
            msg_sen,
            openmls_store,
            invitation_memory,
            identity_key: expanded_key,
            provider,
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
        let signer = if let Some(signer) = SignatureKeyPair::read(
            &self.openmls_store,
            &self.identity_key.public().to_bytes(),
            SignatureScheme::ED25519,
        ) {
            signer
        } else {
            let signer = SignatureKeyPair::from_raw(
                SignatureScheme::ED25519,
                self.identity_key.to_secret_key_bytes().to_vec(),
                self.identity_key.public().to_bytes().to_vec(),
            );
            signer.store(&self.openmls_store)?;
            signer
        };
        let peers = Arc::clone(&self.peers);
        let provider = ConanMlsProvider::new(&self.dbconn)?;
        tokio::spawn(async move {
            let signing_key = signing_key(arti_path).await.unwrap();
            let new_group = ConanGroup::build(&signing_key).unwrap();
            let key_package = new_group.key_package_bundle(&signer, &provider).unwrap();
            let Ok(mut peers) = peers.write() else {
                println!("Cannot write to peers.");
                return;
            };
            let Some(peer) = peers.get_mut(&peer_idx) else {
                println!("Cannot get selected peer.");
                return;
            };
            let key_package_ser = to_bytes(key_package.key_package());
            // let secure_idx = direct_encrypt(&key, &(peer_idx.to_be_bytes())).unwrap();
            peer.command_sender
                .send(SlaveCmd::Msg(Msg::KeyPackage(key_package_ser)))
                .unwrap();
            println!("peer idx: {peer_idx}");
        });
        Ok(())
    }

    pub fn handle_msg_keypackage(&self, peer_idx: u8, data: &[u8]) -> Result<(), Box<dyn Error>> {
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
            return Err("No Group found at given Index".into());
        };
        let mut groups = self.groups.write().unwrap();
        let Some((_grp_idx, target_group)) = groups
            .iter_mut()
            .find(|f| f.1.group.group_id().as_slice() == group_id)
        else {
            // TODO: need to change later
            return Ok(());
        };
        let (_commit, welcome) = target_group.add_members(&key_package, &self.provider)?;
        let mut peers = self.peers.write().unwrap();
        let Some(peer) = peers.get_mut(&peer_idx) else {
            return Err("No peer found at given Index".into());
        };
        let MlsMessageBodyOut::Welcome(welcome) = welcome.body().clone() else {
            return Err("No Welcome found. Ignoring.".into());
        };
        let tree: RatchetTreeIn = target_group.group.export_ratchet_tree().into();
        peer.command_sender
            .send(SlaveCmd::Msg(Msg::Welcome(welcome, tree)))?;
        Ok(())
    }

    pub fn handle_msg_welcome(
        &self,
        peer_idx: u8,
        welcome: Welcome,
        tree: RatchetTreeIn,
    ) -> Result<(), Box<dyn Error>> {
        println!("group welcome");
        let mut peers = self.peers.write().unwrap();
        let Some(peer) = peers.get_mut(&peer_idx) else {
            println!("No peer found at given index");
            return Err("No peer found at given index".into());
        };
        let mut grp = ConanGroup::build(&self.identity_key)?;
        let mls_grp = ConanGroup::join_group(welcome, tree)?;
        let grp_id = mls_grp.group_id().to_vec();
        grp.group = mls_grp;

        let grp_name = generate_name(3..8);
        let dbgroup = DBGroup::new(grp_id.clone(), grp_name);

        peer.command_sender
            .send(SlaveCmd::Msg(Msg::GroupVerified(grp_id)))
            .unwrap();

        let Ok(mut grps) = self.groups.write() else {
            return Err("Cannot write to groups.".into());
        };
        let db_grp = self.dbconn.insert_group(dbgroup)?;

        // we insert group to database and memory
        grps.insert(db_grp.id, grp);
        Ok(())
    }

    pub fn handle_msg_group_verified(
        &self,
        peer_idx: u8,
        group_id: &[u8],
    ) -> Result<(), Box<dyn Error>> {
        println!("group verified");
        {
            let mut invitation = self.invitation_memory.write().unwrap();
            invitation.remove(&peer_idx);
        }
        let mut groups = self.groups.write().unwrap();
        let Some((_grp_idx, grp)) = groups
            .iter_mut()
            .find(|g| g.1.group.group_id().as_slice() == group_id)
        else {
            // TODO: return error
            return Ok(());
        };
        grp.group.merge_pending_commit(&self.provider)?;
        // the group is already saved in initializers database
        // so it can be retrieved
        let db_group = self.dbconn.get_group_by_group_id(group_id)?;

        // Inserting member in our database
        let peer = self
            .dbconn
            .get_peer_from_id(u32::from(peer_idx))?
            .ok_or("Unknown Peer dropping connection")?;
        self.dbconn.insert_member(db_group.id, peer, true)?;

        let text = b"Hello there";
        let mut peers = self.peers.write().unwrap();
        for idx in grp.members.iter() {
            println!("member: {idx}");
            let Some(peer) = peers.get_mut(idx) else {
                continue;
            };
            let message = grp
                .group
                .create_message(&self.provider, &grp.signer, text)
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
        let Ok(mut invitation) = self.invitation_memory.write() else {
            return Err("Cannot write to group.".into());
        };
        let group_id = invitation
            .remove(&peer_idx)
            .ok_or("Could not remove group.")?;
        let group_id = GroupId::from_slice(&group_id);
        let mut groups = self.groups.write().unwrap();
        let Some((_grp_idx, group)) = groups
            .iter_mut()
            .find(|f| f.1.group.group_id() == &group_id)
        else {
            return Err("Could not find targetted group".into());
        };
        group.group.clear_pending_commit(&self.openmls_store)?;
        let db_group = self.dbconn.get_group_by_group_id(&group_id.to_vec())?;
        if !group.members.is_empty() {
            groups.remove(&db_group.id);
        }
        ConanNotif::Sys(err).notify()?;
        Ok(())
    }

    pub fn handle_group_message(
        &self,
        peer_idx: u8,
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
            .process_message(&self.provider, message)
            .unwrap();
        let content = processed_message.into_content();
        match content {
            ProcessedMessageContent::ApplicationMessage(mess) => {
                let content = mess.into_bytes();
                let data = String::from_utf8_lossy(&content).to_string();
                println!("got: {data}");
                let gc = GroupChat {
                    id: 0,
                    group_id: dbgroup.id,
                    sender_id: peer_idx,
                    data,
                    time: String::new(),
                };
                self.dbconn.insert_group_chat(gc)?;
            }
            _ => {}
        }
        group.group.merge_pending_commit(&self.provider).unwrap();
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
