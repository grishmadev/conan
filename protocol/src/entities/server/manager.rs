use arti_client::{BootstrapBehavior, TorClient, TorClientConfig, config::CfgPath};
use futures::{StreamExt, stream::BoxStream};
use rand::random_range;
use rusqlite::Connection;
use safelog::DisplayRedacted;
use std::{
    collections::HashMap,
    error::Error,
    sync::{Arc, Mutex, RwLock},
    time::Duration,
};
use tokio::sync::broadcast;
use tor_cell::relaycell::msg::Connected;
use tor_hsservice::{HsNickname, OnionServiceConfig, RendRequest, RunningOnionService};

use crate::{
    comm::enums::IPCRes,
    config::ConanConfig,
    database::DBConnection,
    debug,
    entities::{
        database::peer::{Peer, PeerData},
        server::slave::Slave,
    },
    extras::generate_name,
    msg::{Internal, Msg, PeerStatus},
    operations::dialer_actor,
};

pub struct Manager {
    tor_client: Arc<TorClient<tor_rtcompat::PreferredRuntime>>,
    /// NOTE: Only for assigning to `Slaves`, not to be used by manager itself
    response_sender: broadcast::Sender<(u8, Internal)>,

    stream: Option<BoxStream<'static, RendRequest>>,
    service: Arc<RunningOnionService>,
    pub dbconn: Connection,

    /// Channel for sending message to Master
    pub msg_sender: broadcast::Sender<IPCRes>,
    /// Used for receiving messages from Slaves and transferring them to Master
    pub response_receiver: broadcast::Receiver<(u8, Internal)>,
    /// `HashMap` for tracking active peers
    pub peers: Arc<Mutex<HashMap<u8, Slave>>>,
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
        let Ok(Some((service, request_stream))) = tor_client.launch_onion_service(svc_config)
        else {
            return Err("Could not launch onion service...".into());
        };

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
            peers: Arc::new(Mutex::new(HashMap::new())),
            stream: Some(request_stream.boxed()),
            service,
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
                                            let mut conn = Slave {
                                                id: 0,
                                                reader: Some(reader),
                                                writer,
                                                service,
                                                msg_sender,
                                                response_sender,
                                                dbconn: Connection::open(&config.db_path).unwrap(),
                                                config,
                                                shared_secret_key: Arc::new(RwLock::new([0u8; 32])),
                                            };
                                            let peer_idx = match conn.connect_as_listener().await {
                                                Ok(s) => s,
                                                Err(e) => {
                                                    eprintln!("Cannot connect as listener.\n{e}");
                                                    continue;
                                                }
                                            };
                                            if conn.spawn_communication().is_ok() {
                                                let mut guard = peers.lock().unwrap();
                                                guard.insert(peer_idx, conn);
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
        // checking if peer is already in our connected
        {
            let peer = dbconn.get_peer(&peer_addr.0)?;
            if let Some(peer) = peer {
                #[allow(clippy::cast_possible_truncation)]
                if peers.lock().unwrap().contains_key(&(peer.id as u8)) {
                    msg_sender.send(IPCRes::Connected(peer_addr.0, peer_addr.1))?;
                    msg_sender.send(IPCRes::Notification(format!(
                        "Already connected to {}",
                        peer.name.unwrap_or("Peer".to_string())
                    )))?;
                    return Ok(PeerStatus::Connected);
                }
            }
        }
        let service = Arc::clone(&self.service);
        let config = self.config.clone();
        tokio::spawn(async move {
            println!("Connecting to peer...");
            let mut stream = None;

            for _ in 0..5 {
                match tor_client.connect(&peer_addr).await {
                    Ok(s) => {
                        stream = Some(s);
                        break;
                    }
                    Err(e) => eprintln!("Error while connecting: {e}"),
                }
                eprintln!("Retrying...");
            }
            let Some(stream) = stream else {
                msg_sender.send(IPCRes::Error(format!(
                    "Could not connect to {}. Make sure the address is correct and server active..",
                    peer_addr.0
                ))).unwrap();
                return;
            };
            let (mut reader, mut writer) = tokio::io::split(stream);
            let mut shared_secret_key = None;

            let local_hsid = service
                .onion_address()
                .ok_or("Onion Address Not found.")
                .unwrap();
            if let Err(e) = dialer_actor(
                config.arti_key_store.clone(),
                &mut reader,
                &mut writer,
                &mut shared_secret_key,
                local_hsid,
                &peer_addr,
            )
            .await
            {
                let msg = format!("Error while Reaching out.\n{e}");
                msg_sender.send(IPCRes::Error(msg)).unwrap();
                return;
            }
            let Some(shared_secret_key) = shared_secret_key else {
                eprintln!("Could not parse Shared Secret Key. Aborting.");
                return;
            };
            let known = dbconn.get_peer(&peer_addr.0).unwrap();
            let mut idx = None;
            let trans = dbconn.transaction().unwrap();
            if known.is_none() {
                let peer = Peer::build(&generate_name(random_range(3..10)), &peer_addr.0);
                let peer = trans.insert_peer(peer).unwrap();
                idx = Some(peer.id as u8);
            }
            let Some(idx) = idx else {
                eprintln!("Could not add peer to database.");
                return;
            };

            let mut conn = Slave {
                id: idx,
                reader: Some(reader),
                writer,
                service,
                dbconn: Connection::open(&config.db_path).unwrap(),
                config,
                msg_sender: msg_sender.clone(),
                response_sender,
                shared_secret_key: Arc::new(RwLock::new(shared_secret_key)),
            };
            if conn.spawn_communication().is_ok() {
                let mut peers = peers.lock().unwrap();
                #[allow(clippy::cast_possible_truncation)]
                peers.insert(idx, conn);
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
        tokio::spawn(async move {
            while let Ok(internal) = rec.recv().await {
                let mut res = None;
                match internal.1 {
                    Internal::Msg(msg) => match msg {
                        Msg::Text(text) => {
                            res = Some(IPCRes::Text(internal.0, text));
                        }
                        Msg::Verified => {
                            res = Some(IPCRes::Notification("Verified.".into()));
                        }
                        _ => continue,
                    },
                    Internal::RemovePeer(idx) => {
                        if let Ok(mut guard) = peers.lock() {
                            guard.remove(&idx);
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
