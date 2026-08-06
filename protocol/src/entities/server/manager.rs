use arti_client::{BootstrapBehavior, TorClient, TorClientConfig, config::CfgPath};
use futures::{StreamExt, stream::BoxStream};
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

use crate::{
    comm::enums::IPCRes,
    config::{ConanConfig, parse_config},
    debug,
    entities::{
        database::peer::{Peer, PeerData},
        server::{manager_assistant::CommandHandler, slave::Slave},
    },
    extras::{codec::BincodeCodec, generate_name},
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
    pub groups: Arc<RwLock<HashMap<u8, ConanGroup>>>,
    /// `HashMap` for tracking temporary Groups
    /// here `u8` refers to peers (not group) associated with the given group
    pub temp_groups: Arc<RwLock<HashMap<u8, ConanGroup>>>,
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
        let conn = Connection::open(&config.db_path)?;
        conn.execute(
            &format!(
                "INSERT OR REPLACE INTO peer (id, name, address, is_friend) VALUES (1, 'Me', '{}', TRUE)",
                hsid.display_unredacted()
            ),
            (),
        )?;
        let (response_sender, response_receiver) = broadcast::channel::<(u8, Internal)>(100);

        Ok(Self {
            tor_client,
            peers: Arc::new(RwLock::new(HashMap::new())),
            groups: Arc::new(RwLock::new(HashMap::new())),
            temp_groups: Arc::new(RwLock::new(HashMap::new())),
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

    pub async fn make_peer_join_group(
        &self,
        peer_id: u8,
        group_id: u8,
    ) -> Result<(), Box<dyn Error>> {
        let peers = Arc::clone(&self.peers);
        let config = parse_config()?;
        let key = signing_key(config.arti_key_store).await?;
        let mut peers = peers.write().unwrap();
        #[allow(clippy::cast_possible_truncation)]
        let Some(target) = peers.get_mut(&(peer_id)) else {
            return Err("Cannot find target peer.".into());
        };
        let groups = Arc::clone(&self.groups);
        let mut groups = groups.write().unwrap();

        target.command_sender.send(SlaveCmd::Msg(Msg::JoinGroup))?;
        let Ok(mut tmp_grps) = self.temp_groups.write() else {
            return Err("Could not write to temp groups".into());
        };
        let group = match groups.remove(&group_id) {
            Some(e) => e,
            None => ConanGroup::build(&key)?,
        };
        #[allow(clippy::cast_possible_truncation)]
        tmp_grps.insert(peer_id, group);
        Ok(())
    }
    /// Used to setup Slave to Master communication Pipeline
    /// # Errors
    pub fn setup_slave_communication(&mut self) -> Result<(), Box<dyn Error>> {
        let mut rec = self.response_receiver.resubscribe();
        let sen = self.msg_sender.clone();
        let peers = Arc::clone(&self.peers);
        let groups = Arc::clone(&self.groups);
        let temp_groups = Arc::clone(&self.temp_groups);
        let dbconn = Connection::open(&self.config.db_path)?;
        let config = self.config.clone();
        let openmls_store: SqliteStorageProvider<BincodeCodec, Connection> =
            SqliteStorageProvider::new(Connection::open(&self.config.db_path)?);
        self.server_ready.store(true, Ordering::SeqCst);
        tokio::spawn(async move {
            let cmdhandler =
                CommandHandler::new(peers, groups, temp_groups, dbconn, sen, openmls_store);
            while let Ok((peer_idx, internal)) = rec.recv().await {
                let res = match internal {
                    Internal::Msg(msg) => match msg {
                        Msg::Text(text) => cmdhandler.handle_msg_text(peer_idx, text),
                        Msg::Verified => cmdhandler.handle_msg_verified(),
                        Msg::JoinGroup => {
                            cmdhandler.handle_msg_convert(peer_idx, config.arti_key_store.clone())
                        }
                        Msg::KeyPackage(key_idx, package) => {
                            cmdhandler.handle_msg_keypackage(peer_idx, key_idx, &package)
                        }
                        Msg::Welcome(group_idx, welcome, tree) => {
                            cmdhandler.handle_msg_welcome(peer_idx, &group_idx, welcome, tree)
                        }
                        Msg::GroupError(err) => cmdhandler.handle_msg_group_error(peer_idx, err),
                        Msg::GroupVerified(group_id) => {
                            cmdhandler.handle_msg_group_verified(peer_idx, &group_id)
                        }
                        Msg::GroupMessage(group_id, message) => {
                            cmdhandler.handle_group_message(&group_id, &message)
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
}
