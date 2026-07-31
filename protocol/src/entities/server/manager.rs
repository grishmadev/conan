use arti_client::{BootstrapBehavior, TorClient, TorClientConfig, config::CfgPath};
use futures::{StreamExt, stream::BoxStream};
use openmls::prelude::{
    BasicCredential, CredentialWithKey, DeserializeBytes, KeyPackage, MlsMessageBodyOut,
    MlsMessageIn, ProcessedMessageContent, RatchetTreeIn, tls_codec::Serialize,
};
use rand::random_range;
use rusqlite::Connection;
use safelog::DisplayRedacted;
use std::{
    collections::HashMap,
    error::Error,
    sync::{
        Arc, RwLock,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};
use tokio::sync::broadcast;
use tor_cell::relaycell::msg::Connected;
use tor_hsservice::{HsNickname, OnionServiceConfig, RendRequest, RunningOnionService};

use crate::{
    comm::{
        enums::{IPCRes, from_bytes, to_bytes},
        notification::ConanNotif,
    },
    config::ConanConfig,
    database::DBConnection,
    debug,
    entities::{
        database::{
            chat::{Chat, ChatData},
            peer::{Peer, PeerData},
        },
        server::slave::Slave,
    },
    extras::generate_name,
    mls::ConanGroup,
    msg::{Internal, Msg, PeerStatus, SlaveCmd},
    operations::dialer_actor,
};

pub struct Manager {
    pub tor_client: Arc<TorClient<tor_rtcompat::PreferredRuntime>>,
    /// NOTE: Only for assigning to `Slaves`, not to be used by manager itself
    pub response_sender: broadcast::Sender<(u8, Internal)>,
    pub stream: Option<BoxStream<'static, RendRequest>>,
    pub service: Arc<RunningOnionService>,
    pub dbconn: Connection,
    /// States whether Server Ready or not
    pub server_ready: AtomicBool,

    /// Channel for sending message to Master
    pub msg_sender: broadcast::Sender<IPCRes>,
    /// Used for receiving messages from Slaves and transferring them to Master
    pub response_receiver: broadcast::Receiver<(u8, Internal)>,
    /// `HashMap` for tracking active peers
    pub peers: Arc<RwLock<HashMap<u8, Slave>>>,
    /// `HashMap` for tracking active Groups
    pub groups: Arc<RwLock<HashMap<u8, ConanGroup>>>,
    /// Paths chosen during startup
    pub config: ConanConfig,
}

impl Manager {
    /// # Errors
    pub async fn create(
        msg_sender: broadcast::Sender<IPCRes>,
        config: ConanConfig,
    ) -> Result<Self, Box<dyn Error>> {
        let mut tor_config_builder = TorClientConfig::builder();
        let stream_timeout_config = tor_config_builder.stream_timeouts();
        // setting timeout to 20 secs bcoz we'd rather not wait 60 secs when its destined to fail
        stream_timeout_config.connect_timeout(Duration::from_secs(20));
        let storage_builder = tor_config_builder.storage();
        let state_path = CfgPath::new(config.arti_key_store.clone());
        let cache_path = CfgPath::new(config.cache_path.clone());
        storage_builder.cache_dir(cache_path).state_dir(state_path);
        let tor_config = tor_config_builder.build()?;

        debug!("Starting Server...");
        let tor_client = TorClient::builder()
            .bootstrap_behavior(BootstrapBehavior::OnDemand)
            .config(tor_config)
            .create_bootstrapped()
            .await?;

        let nickname = HsNickname::new("conan-daemon".to_string())?;
        let svc_config = OnionServiceConfig::builder().nickname(nickname).build()?;
        let service;
        let request_stream;
        match tor_client.launch_onion_service(svc_config) {
            Ok(Some((tservice, trequest_stream))) => {
                service = tservice;
                request_stream = trequest_stream;
            }
            Err(e) => return Err(format!("Error while launching tor server.\n{e}").into()),
            _ => {
                return Err("Could not launch onion service...".into());
            }
        }

        let hsid: tor_hsservice::HsId = match service.onion_address() {
            Some(s) => s,
            None => return Err("No HsId found.".into()),
        };
        println!("Server Address: {}", hsid.display_unredacted());
        let conn = DBConnection::build(&config.db_path)?;
        conn.execute(&format!(
            "INSERT OR REPLACE INTO peer (id, name, address) VALUES (1, 'Me', '{}')",
            hsid.display_unredacted()
        ))?;
        let (response_sender, response_receiver) = broadcast::channel::<(u8, Internal)>(100);

        Ok(Self {
            tor_client,
            peers: Arc::new(RwLock::new(HashMap::new())),
            groups: Arc::new(RwLock::new(HashMap::new())),
            stream: Some(request_stream.boxed()),
            service,
            server_ready: AtomicBool::new(false),
            dbconn: Connection::open(&config.db_path)?,
            msg_sender,
            response_receiver,
            response_sender,
            config,
        })
    }

    /// # Errors
    /// # Panics
    pub fn init_server(&mut self) -> Result<(), Box<dyn Error>> {
        debug!("Initializing Server.");
        let mut stream = self.stream.take().unwrap();

        // spawn a thread for handling connections from network
        let msg_sender = self.msg_sender.clone();
        let response_sender = self.response_sender.clone();
        let peers = Arc::clone(&self.peers);
        let service = self.service.clone();
        let config = self.config.clone();
        tokio::spawn(async move {
            let msg_sender = msg_sender.clone();
            let response_sender = response_sender.clone();
            let peers = Arc::clone(&peers);
            let service = Arc::clone(&service);
            loop {
                while let Some(rendreq) = stream.next().await {
                    let config = config.clone();
                    let msg_sender = msg_sender.clone();
                    let response_sender = response_sender.clone();
                    let peers = Arc::clone(&peers);
                    let service = Arc::clone(&service);
                    _ = msg_sender.send(IPCRes::Notification(
                        "Someone is trying to connect.".to_string(),
                    ));
                    println!("Client Detected.");
                    tokio::spawn(async move {
                        match rendreq.accept().await {
                            Ok(mut stream) => {
                                while let Some(strreq) = stream.next().await {
                                    let service = Arc::clone(&service);
                                    let msg_sender = msg_sender.clone();
                                    let response_sender = response_sender.clone();
                                    let config = config.clone();
                                    match strreq.accept(Connected::new_empty()).await {
                                        Ok(stream) => {
                                            let (reader, writer) = tokio::io::split(stream);
                                            let mut conn = Slave::build(
                                                0,
                                                reader,
                                                writer,
                                                service,
                                                config,
                                                msg_sender,
                                                response_sender,
                                                None,
                                            )
                                            .unwrap();
                                            let peer_idx = match conn.connect_as_listener().await {
                                                Ok(s) => s,
                                                Err(e) => {
                                                    eprintln!("Cannot connect as listener.\n{e}");
                                                    continue;
                                                }
                                            };
                                            if conn.spawn_communication().is_ok() {
                                                println!(
                                                    "Slave Communication Pipeline Established."
                                                );
                                            }
                                            if let Ok(mut peers) = peers.write() {
                                                peers.insert(peer_idx, conn);
                                            }
                                        }
                                        Err(e) => {
                                            eprintln!("Error in connecting to peer: {e}");
                                        }
                                    }
                                }
                            }
                            Err(e) => {
                                eprintln!("Failed to complete handshake.\n{e}");
                            }
                        }
                    });
                }
            }
        });

        Ok(())
    }

    /// Connects to Peer's Tor Address as a dialer (Seeking connection)
    /// # Errors
    /// # Panics
    pub fn connect_as_dialer(
        &mut self,
        peer_addr: (String, u16),
    ) -> Result<PeerStatus, Box<dyn Error>> {
        let tor_client = Arc::clone(&self.tor_client);
        let msg_sender = self.msg_sender.clone();
        let mut dbconn = Connection::open(&self.config.db_path)?;
        let response_sender = self.response_sender.clone();
        let peers = Arc::clone(&self.peers);
        if let Some(hsid) = self.service.onion_address()
            && peer_addr.0 == hsid.display_unredacted().to_string()
        {
            msg_sender.send(IPCRes::Error("Cannot connect to Self.".to_string()))?;
            return Ok(PeerStatus::NotFound);
        }
        // checking if peer is already in our connection
        {
            let peer = dbconn.get_peer_from_addr(&peer_addr.0)?;
            if let Some(peer) = peer {
                #[allow(clippy::cast_possible_truncation)]
                if peers.read().unwrap().contains_key(&(peer.id as u8)) {
                    msg_sender.send(IPCRes::Connected(peer_addr.0, peer_addr.1))?;
                    msg_sender.send(IPCRes::Notification(format!(
                        "Already connected to {}",
                        peer.name
                    )))?;
                    return Ok(PeerStatus::Connected);
                }
            }
        }
        let service = Arc::clone(&self.service);
        let config = self.config.clone();
        let msg_sender = self.msg_sender.clone();
        tokio::spawn(async move {
            println!("Connecting to peer...");
            let mut stream = None;

            for i in 1..=5 {
                match tor_client.connect(&peer_addr).await {
                    Ok(s) => {
                        stream = Some(s);
                        break;
                    }
                    Err(e) => eprintln!("Error while connecting: {e}"),
                }
                if i == 5 {
                    msg_sender
                        .send(IPCRes::Error("Failed to Connect.".to_string()))
                        .unwrap();
                } else {
                    msg_sender
                        .send(IPCRes::Notification(format!(
                            "Retrying Connection. [{i}/5]"
                        )))
                        .unwrap();
                    eprintln!("Retrying...");
                }
            }
            let Some(stream) = stream else {
                msg_sender.send(IPCRes::Error(format!(
                    "Could not connect to {}. Make sure the address is correct and server active..",
                    peer_addr.0
                ))).unwrap();
                return;
            };
            let (mut reader, mut writer) = tokio::io::split(stream);

            let local_hsid = service
                .onion_address()
                .ok_or("Onion Address Not found.")
                .unwrap();
            let session = match dialer_actor(
                config.arti_key_store.clone(),
                &mut reader,
                &mut writer,
                local_hsid,
                &peer_addr,
            )
            .await
            {
                Ok(s) => s,
                Err(e) => {
                    let msg = format!("Error while Reaching out.\n{e}");
                    msg_sender.send(IPCRes::Error(msg)).unwrap();
                    return;
                }
            };
            let known = dbconn.get_peer_from_addr(&peer_addr.0).unwrap();
            let trans = dbconn.transaction().unwrap();
            let name = generate_name(random_range(3..10));
            let idx = if let Some(known_peer) = known {
                known_peer.id
            } else {
                let peer = Peer::build(&name, &peer_addr.0);
                let peer = trans.insert_peer(peer).unwrap();
                peer.id
            };

            #[allow(clippy::cast_possible_truncation)]
            let mut conn = Slave::build(
                idx as u8,
                reader,
                writer,
                service,
                config,
                msg_sender.clone(),
                response_sender,
                Some(Arc::new(tokio::sync::RwLock::new(session))),
            )
            .unwrap();
            if conn.spawn_communication().is_ok() {
                let mut peers = peers.write().unwrap();
                #[allow(clippy::cast_possible_truncation)]
                peers.insert(idx as u8, conn);
                trans.commit().unwrap();
            } else {
                _ = trans.rollback();
            }
            println!("Exchange Complete..");
            msg_sender
                .send(IPCRes::Connected(peer_addr.0, peer_addr.1))
                .unwrap();
        });
        Ok(PeerStatus::Connected)
    }

    /// Used to setup Slave to Master communication Pipeline
    /// # Errors
    pub fn setup_slave_communication(&mut self) -> Result<(), Box<dyn Error>> {
        let mut rec = self.response_receiver.resubscribe();
        let sen = self.msg_sender.clone();
        let peers = Arc::clone(&self.peers);
        let groups = Arc::clone(&self.groups);
        let dbconn = Connection::open(&self.config.db_path)?;
        let msg_sen = self.msg_sender.clone();
        self.server_ready.store(true, Ordering::SeqCst);
        tokio::spawn(async move {
            while let Ok((idx, internal)) = rec.recv().await {
                let mut res = None;
                let idx_u32 = u32::from(idx);
                match internal {
                    Internal::Msg(msg) => match msg {
                        Msg::Text(text) => {
                            let chat = Chat::chat_to_rec(&text, idx_u32);
                            for _ in 0..3 {
                                if let Err(e) = dbconn.insert_chat(chat.clone()) {
                                    println!("Error inserting chat: {e}");
                                } else {
                                    break;
                                }
                            }
                            res = Some(IPCRes::Text(idx, text.clone()));
                            let Ok(Some(target)) = dbconn.get_peer_from_id(idx_u32) else {
                                _ = msg_sen
                                    .send(IPCRes::Error("Cannot find peer in database.".into()));
                                continue;
                            };
                            _ = ConanNotif::Text(target.name, text).notify().await;
                        }
                        Msg::Verified => {
                            res = Some(IPCRes::Notification("Verified.".into()));
                            _ = ConanNotif::Sys("Peer Verified".into()).notify().await;
                        }
                        Msg::Convert => {
                            println!("got convert to group");
                            let new_group = ConanGroup::build("my-group").unwrap();
                            let credential_with_key = CredentialWithKey {
                                credential: BasicCredential::new("Conan".into()).into(),
                                signature_key: new_group.signer.public().into(),
                            };
                            let key_package = KeyPackage::builder()
                                .build(
                                    new_group.group.ciphersuite(),
                                    &new_group.provider,
                                    &new_group.signer,
                                    credential_with_key,
                                )
                                .unwrap();
                            let Ok(mut peers) = peers.write() else {
                                continue;
                            };
                            let Some(peer) = peers.get_mut(&idx) else {
                                continue;
                            };

                            let key_package_ser = to_bytes(key_package.key_package());
                            peer.command_sender
                                .send(SlaveCmd::Msg(Msg::KeyPackage(key_package_ser)))
                                .unwrap();
                            let mut groups = groups.write().unwrap();
                            groups.insert(0, new_group);
                        }
                        Msg::KeyPackage(package) => {
                            println!("got keypackage");
                            let Ok(key_package) = from_bytes::<KeyPackage>(&package) else {
                                continue;
                            };
                            let Ok(mut groups) = groups.write() else {
                                continue;
                            };
                            let Some(group) = groups.get_mut(&0) else {
                                continue;
                            };
                            let (_commit, welcome) = group.add_members(&key_package).unwrap();

                            let Ok(mut peers) = peers.write() else {
                                continue;
                            };
                            let Some(peer) = peers.get_mut(&idx) else {
                                continue;
                            };
                            let MlsMessageBodyOut::Welcome(welcome) = welcome.body().clone() else {
                                println!("No Welcome found. Ignoring.");
                                continue;
                            };
                            let tree: RatchetTreeIn = group.group.export_ratchet_tree().into();

                            peer.command_sender
                                .send(SlaveCmd::Msg(Msg::Welcome(welcome, tree)))
                                .unwrap();
                        }
                        Msg::Welcome(welcome, tree) => {
                            println!("group welcome");
                            let Ok(mut peers) = peers.write() else {
                                continue;
                            };
                            let Some(peer) = peers.get_mut(&idx) else {
                                continue;
                            };
                            if let Ok(mut groups) = groups.write() {
                                let group = groups.get_mut(&0).unwrap();
                                let mls_group =
                                    ConanGroup::join_group(&group.provider, welcome, tree).unwrap();
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
                        }
                        Msg::GroupError(err) => {
                            let Ok(mut groups) = groups.write() else {
                                continue;
                            };
                            let Some(group) = groups.get_mut(&0) else {
                                continue;
                            };
                            group.group.merge_pending_commit(&group.provider).unwrap();
                            _ = ConanNotif::Sys(err);
                        }
                        Msg::GroupVerified => {
                            println!("group verified");
                            let Ok(mut groups) = groups.write() else {
                                continue;
                            };
                            let Some(group) = groups.get_mut(&0) else {
                                continue;
                            };
                            group.members.insert(idx);
                            group.group.merge_pending_commit(&group.provider).unwrap();
                            let text = b"Hello there";
                            let Ok(mut peers) = peers.write() else {
                                continue;
                            };
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

                                let message_ser = message.tls_serialize_detached().unwrap();
                                peer.command_sender
                                    .send(SlaveCmd::Msg(Msg::GroupMessage(message_ser)))
                                    .unwrap();
                            }
                            _ = ConanNotif::Sys("Group Action Complete.".into());
                        }
                        Msg::GroupMessage(message) => {
                            println!("got group message");
                            let (message, _) =
                                MlsMessageIn::tls_deserialize_bytes(&message).unwrap();
                            let Ok(mut groups) = groups.write() else {
                                continue;
                            };
                            let Some(group) = groups.get_mut(&0) else {
                                continue;
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
                        }
                        _ => unimplemented!(),
                    },
                    Internal::RemovePeer(idx) => {
                        if let Ok(mut guard) = peers.write()
                            && let Some(conn) = guard.remove(&idx)
                        {
                            println!("Removing Connection: {}", conn.id);
                            if let Ok(Some(peer)) = dbconn.get_peer_from_id(u32::from(conn.id)) {
                                _ = ConanNotif::Sys(format!("{} disconnected.", peer.name));
                            }
                        }
                    }
                    _ => continue,
                }
                if let Some(msg) = res {
                    _ = sen.send(msg);
                }
            }
        });
        Ok(())
    }
}
