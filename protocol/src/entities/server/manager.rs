use arti_client::{BootstrapBehavior, TorClient, TorClientConfig, config::CfgPath};
use futures::{StreamExt, stream::BoxStream};
use openmls::{
    group::{GroupId, MlsGroup},
    prelude::tls_codec::Serialize,
};
use openmls_sqlite_storage::SqliteStorageProvider;
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
use tor_llcrypto::pk::ed25519::ExpandedKeypair;

use crate::{
    comm::enums::{IPCCmd, IPCRes},
    config::ConanConfig,
    database::ConnectionClone,
    debug,
    entities::{
        database::{
            group::ConnectionGroup,
            group_chat::{ConnectionGroupChat, GroupChat},
            peer::{Peer, PeerData},
        },
        server::{manager_assistant::CommandHandler, slave::Slave},
    },
    extras::{codec::JsonCodec, generate_name, mls_provider::ConanMlsProvider},
    mls::ConanGroup,
    msg::{Internal, Msg, PeerStatus, SlaveCmd},
    operations::{dialer_actor, signing_key},
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
    pub groups: Arc<RwLock<HashMap<u8, MlsGroup>>>,
    /// `HashMap` for tracking temporary Groups
    /// here `u8` refers to peers (not group) associated with the given group
    pub invitation_memory: Arc<RwLock<HashMap<u8, Vec<u8>>>>,
    /// Paths chosen during startup
    pub config: ConanConfig,
    pub identity_key: ExpandedKeypair,
    pub provider: ConanMlsProvider,
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
        let conn = Connection::open(&config.db_path)?;
        conn.execute(
            &format!(
                "INSERT OR REPLACE INTO peer (id, name, address, is_friend) VALUES (1, 'Me', '{}', TRUE)",
                hsid.display_unredacted()
            ),
            (),
        )?;
        let (response_sender, response_receiver) = broadcast::channel::<(u8, Internal)>(100);
        let identity_key = signing_key().await?;
        let provider = ConanMlsProvider::new(&conn)?;

        Ok(Self {
            tor_client,
            peers: Arc::new(RwLock::new(HashMap::new())),
            groups: Arc::new(RwLock::new(HashMap::new())),
            invitation_memory: Arc::new(RwLock::new(HashMap::new())),
            stream: Some(request_stream.boxed()),
            service,
            server_ready: AtomicBool::new(false),
            dbconn: Connection::open(&config.db_path)?,
            msg_sender,
            response_receiver,
            response_sender,
            config,
            identity_key,
            provider,
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
                    println!("Peer Detected.");
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

        tokio::spawn(async move {});

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
                        .send(IPCRes::Error(
                            "Failed to Connect.\nPlease check you internet connection".to_string(),
                        ))
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
            let session = match dialer_actor(&mut reader, &mut writer, local_hsid, &peer_addr).await
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
            let name = generate_name(3..10);
            let idx = if let Some(known_peer) = known {
                known_peer.id
            } else {
                let peer = Peer::build(&name, &peer_addr.0, true);
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

    pub fn get_mls_group_from_idx(&self, idx: u8) -> Result<MlsGroup, Box<dyn Error>> {
        let dbgrp = self.dbconn.get_group_by_idx(idx)?;
        let storage = SqliteStorageProvider::<JsonCodec, _>::new(&self.dbconn);
        let mlsgrp = MlsGroup::load(&storage, &GroupId::from_slice(&dbgrp.group_id))?
            .ok_or("Cannot find MlsGroup")?;
        Ok(mlsgrp)
    }

    /// # Errors
    pub fn make_peer_join_group(&self, peer_id: u32, group_idx: u32) -> Result<(), Box<dyn Error>> {
        println!("joining peer: {peer_id}, group idx: {group_idx}");
        #[allow(clippy::cast_possible_truncation)]
        let peer_id = peer_id as u8;
        #[allow(clippy::cast_possible_truncation)]
        let group_idx = group_idx as u8;
        let peers = Arc::clone(&self.peers);
        let Ok(mut peers) = peers.write() else {
            return Err("Could not write to peer".into());
        };
        #[allow(clippy::cast_possible_truncation)]
        let Some(target) = peers.get_mut(&peer_id) else {
            return Err("Cannot find target peer.".into());
        };

        target.command_sender.send(SlaveCmd::Msg(Msg::JoinGroup))?;
        #[allow(clippy::cast_possible_truncation)]
        let Ok(mut groups) = self.groups.write() else {
            return Err("Could not write to groups".into());
        };
        let dbgroup = self.dbconn.get_group_by_idx(group_idx)?;
        let self_link = self
            .dbconn
            .get_peer_from_id(1)?
            .ok_or("Cannot find self link")?
            .address;
        let group = MlsGroup::build(&self.identity_key, &self_link)?;
        // Loading it in memory
        groups.insert(group_idx, group);
        println!("WRITING TO INVITATION MEMORY");
        let Ok(mut invitation) = self.invitation_memory.write() else {
            return Err("Could not write to invitation".into());
        };
        invitation.insert(peer_id, dbgroup.group_id);
        println!("WROTE TO INVITATION MEMORY");
        Ok(())
    }
    /// Used to setup Slave to Master communication Pipeline
    /// # Errors
    pub fn setup_slave_communication(&mut self) -> Result<(), Box<dyn Error>> {
        let mut rec = self.response_receiver.resubscribe();
        let msg_sen = self.msg_sender.clone();
        let peers = Arc::clone(&self.peers);
        let groups = Arc::clone(&self.groups);
        let invitation_memory = Arc::clone(&self.invitation_memory);
        let dbconn = self.dbconn.try_clone()?;
        // let config = self.config.clone();
        let expanded_key =
            ExpandedKeypair::from_secret_key_bytes(self.identity_key.to_secret_key_bytes())
                .ok_or("Cannot extract expanded key")?;
        let provider = ConanMlsProvider::new(&dbconn)?;
        self.server_ready.store(true, Ordering::SeqCst);
        tokio::spawn(async move {
            let cmdhandler = CommandHandler::new(
                peers,
                groups,
                invitation_memory,
                dbconn,
                msg_sen,
                expanded_key,
                provider,
            );
            while let Ok((peer_idx, internal)) = rec.recv().await {
                let res = match internal {
                    Internal::Msg(msg) => match msg {
                        Msg::Text(text) => cmdhandler.handle_msg_text(peer_idx, text),
                        Msg::Verified => cmdhandler.handle_msg_verified(),
                        Msg::JoinGroup => cmdhandler.handle_msg_convert(peer_idx),
                        Msg::KeyPackage(package) => {
                            cmdhandler.handle_msg_keypackage(peer_idx, &package)
                        }
                        Msg::Welcome(welcome) => cmdhandler.handle_msg_welcome(peer_idx, welcome),
                        Msg::GroupError(err) => cmdhandler.handle_msg_group_error(peer_idx, err),
                        Msg::GroupVerified(group_id) => {
                            cmdhandler.handle_msg_group_verified(peer_idx, &group_id)
                        }
                        Msg::GroupMessage(group_id, message) => {
                            cmdhandler.handle_group_message(peer_idx, &group_id, &message)
                        }
                        _ => unimplemented!(),
                    },
                    Internal::RemovePeer(idx) => cmdhandler.remove_peer(idx),
                    _ => continue,
                };
                if let Err(err) = res {
                    eprintln!("Error in Msg: {err:?}");
                }
            }
        });
        Ok(())
    }

    pub async fn connect_to_group(&mut self, group: &mut MlsGroup) -> Result<(), Box<dyn Error>> {
        // getting all the members embedded in the group
        let members = group.get_members()?;
        // creating a list to remember all the members not connected (yet).
        let mut members_to_connect = vec![];
        for m in members {
            // getting peer from db or creating a new one in case there isn't one
            let peer = if let Some(peer) = self.dbconn.get_peer_from_addr(&m)? {
                peer
            } else {
                let new_peer = Peer::build(&generate_name(3..10), &m, false);
                self.dbconn.insert_peer(new_peer)?
            };
            let peers = self.peers.read().unwrap();
            // adding to created list if not already connected
            if !peers.contains_key(&(peer.id as u8)) {
                members_to_connect.push(peer.clone());
            }
        }
        for peer in members_to_connect {
            println!("connect member");
            if let Err(e) = self.connect_as_dialer(peer.address, 80).await {
                eprintln!("Error while connecting to group member.. {e:?}");
            } else {
                println!("Connected to group member. inserting: {}", peer.id);
                let peers = self.peers.write().unwrap();
                if let Some(peer) = peers.get(&(peer.id as u8)) {
                    peer.command_sender
                        .send(SlaveCmd::Msg(Msg::InitiateGroup(group.group_id().to_vec())))?;
                }
            }
        }
        Ok(())
    }

    pub fn send_msg(&self, grp: &mut MlsGroup, text: &str) -> Result<(), Box<dyn Error>> {
        let (signer, _, _) = MlsGroup::signer_from_expanded_key(&self.identity_key);
        let msg = grp.create_message(&self.provider, &signer, text.as_bytes())?;
        let peers = self.peers.read().unwrap();
        let members = grp.get_members()?;
        println!("list members: {members:#?}");
        for m in &members {
            // fanning out to all active members
            let Some(peer) = self.dbconn.get_peer_from_addr(m)? else {
                println!("Could not get targetted member.");
                continue;
            };
            if let Some(peer) = peers.get(&(peer.id as u8)) {
                println!("sending to group");
                peer.command_sender.send(SlaveCmd::Msg(Msg::GroupMessage(
                    grp.group_id().to_vec(),
                    msg.tls_serialize_detached()?,
                )))?;
            } else {
                println!("Targetted Peer not found.");
            }
        }
        let dbgrp = self
            .dbconn
            .get_group_by_group_id(&grp.group_id().to_vec())?;
        let chat = GroupChat {
            id: 0,
            group_id: dbgrp.id,
            data: text.into(),
            sender_id: 1,
            time: String::new(),
        };
        self.dbconn.insert_group_chat(chat)?;
        Ok(())
    }
}
