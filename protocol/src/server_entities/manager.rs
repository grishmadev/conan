use arti_client::{BootstrapBehavior, DataStream, TorClient, TorClientConfig, config::CfgPath};
use ed25519_dalek::ed25519::signature::rand_core::OsRng;
use futures::{StreamExt, stream::BoxStream};
use rand::random_range;
use safelog::DisplayRedacted;
use std::{
    collections::HashMap,
    error::Error,
    ops::Add,
    str::FromStr,
    sync::{Arc, Mutex, RwLock},
    time::Duration,
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    sync::broadcast,
};
use tor_cell::relaycell::msg::Connected;
use tor_hsservice::{HsId, HsNickname, OnionServiceConfig, RendRequest, RunningOnionService};
use x25519_dalek::{EphemeralSecret, PublicKey};

use crate::{
    comm::enums::IPCRes,
    config::parse_config,
    database::DBConnection,
    database_entities::peer::{Peer, PeerData},
    debug,
    extras::generate_name,
    msg::{Msg, PeerStatus},
    operations::{edhverify, encrypt, signing_key, x25519_handshake},
    server_entities::slave::Slave,
};

pub struct Manager {
    tor_client: Arc<TorClient<tor_rtcompat::PreferredRuntime>>,
    /// NOTE: Only for assigning to `Slaves`, not to be used by manager itself
    response_sender: broadcast::Sender<(u8, Msg)>,

    stream: Option<BoxStream<'static, RendRequest>>,
    _service: Arc<RunningOnionService>,
    pub dbconn: DBConnection,

    /// Channel for sending message to Master
    pub msg_sender: broadcast::Sender<IPCRes>,
    /// Used for receiving messages from Slaves and transferring them to Master
    pub response_receiver: broadcast::Receiver<(u8, Msg)>,
    /// `HashMap` for tracking active peers
    pub peers: Arc<Mutex<HashMap<u8, Slave>>>,
}

impl Manager {
    /// # Errors
    pub async fn create(msg_sender: broadcast::Sender<IPCRes>) -> Result<Self, Box<dyn Error>> {
        let config = parse_config()?;
        let mut tor_config_builder = TorClientConfig::builder();
        let stream_timeout_config = tor_config_builder.stream_timeouts();
        // setting timeout to 20 secs bcoz we'd rather not wait 60 secs when its destined to fail
        stream_timeout_config.connect_timeout(Duration::from_secs(20));
        let storage_builder = tor_config_builder.storage();
        let state_path = CfgPath::new(config.arti_key_store.clone());
        let cache_path = CfgPath::new(config.cache_path);
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
        let conn = DBConnection::build()?;
        conn.execute(&format!(
            "INSERT OR REPLACE INTO peer (id, name, address) VALUES (1, 'Me', '{}')",
            hsid.display_unredacted()
        ))?;
        let (response_sender, response_receiver) = broadcast::channel::<(u8, Msg)>(100);
        let dbconn = DBConnection::build()?;

        Ok(Self {
            tor_client,
            peers: Arc::new(Mutex::new(HashMap::new())),
            stream: Some(request_stream.boxed()),
            _service: service,
            dbconn,
            msg_sender,
            response_receiver,
            response_sender,
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
        tokio::spawn(async move {
            let msg_sender = msg_sender.clone();
            let response_sender = response_sender.clone();
            let peers = Arc::clone(&peers);
            loop {
                while let Some(rendreq) = stream.next().await {
                    let msg_sender = msg_sender.clone();
                    let response_sender = response_sender.clone();
                    let peers = Arc::clone(&peers);
                    println!("Client Detected.");
                    tokio::spawn(async move {
                        match rendreq.accept().await {
                            Ok(mut stream) => {
                                while let Some(strreq) = stream.next().await {
                                    let msg_sender = msg_sender.clone();
                                    let response_sender = response_sender.clone();
                                    match strreq.accept(Connected::new_empty()).await {
                                        Ok(stream) => {
                                            let (reader, writer) = tokio::io::split(stream);
                                            let mut conn = Slave {
                                                reader: Some(reader),
                                                writer,
                                                msg_sender,
                                                response_sender,
                                                shared_secret_key: Arc::new(RwLock::new([0u8; 32])),
                                            };
                                            if let Err(e) = conn.connect_as_listener().await {
                                                eprintln!("Cannot connect as listener.\n{e}");
                                                continue;
                                            }
                                            if conn.spawn_communication().is_ok() {
                                                let mut guard = peers.lock().unwrap();
                                                #[allow(clippy::cast_possible_truncation)]
                                                let idx = guard.len() as u8;
                                                guard.insert(idx, conn);
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
        let db_conn = DBConnection::build()?;
        let response_sender = self.response_sender.clone();
        let peers = Arc::clone(&self.peers);
        let local_hsid = self
            ._service
            .onion_address()
            .ok_or("Onion Address Not found.")?;
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
            // writing x25519 public key to stream
            let local_private_key = EphemeralSecret::random_from_rng(OsRng);
            let local_public_key = PublicKey::from(&local_private_key);
            let msg = Msg::PublicKey(local_public_key.to_bytes());
            let msg_bytes = msg.to_vec();
            #[allow(clippy::cast_possible_truncation)]
            writer.write_u16(msg_bytes.len() as u16).await.unwrap();
            writer.write_all(&msg_bytes).await.unwrap();
            writer.flush().await.unwrap();

            let size = reader.read_u16().await.unwrap() as usize;
            let mut buf = vec![0u8; size];
            let size = reader.read_exact(&mut buf).await.unwrap();
            let de_msg = Msg::from_bytes(&buf[..size]);

            let mut remote_public_key = None;
            println!("Performing X25519 Handshake.");
            x25519_handshake(&mut remote_public_key, local_public_key, &peer_addr, de_msg).unwrap();
            let Some(remote_public_key) = remote_public_key else {
                msg_sender
                    .send(IPCRes::Error("Remote Public Key not set.".into()))
                    .unwrap();
                return;
            };
            let mut shared_secret_key = None;
            edhverify(local_private_key, remote_public_key, &mut shared_secret_key);
            let Some(shared_secret_key) = shared_secret_key else {
                msg_sender.send(IPCRes::Error("Couldn't get Shared Secret Key.".into()));
                return;
            };
            let config = parse_config().unwrap().arti_key_store;
            let signing_key = signing_key(config).unwrap();
            let mut data = vec![];
            data.extend_from_slice(remote_public_key.as_bytes());
            data.extend_from_slice(local_public_key.as_bytes());
            let data: [u8; 64] = data.try_into().unwrap();
            let signature = signing_key.sign(&data);
            let local_hsid_bytes = local_hsid.as_ref();
            let remote_hsid = HsId::from_str(&peer_addr.0).unwrap();
            let remote_hsid_bytes = remote_hsid.as_ref();
            let msg = Msg::SignedAndPublicKey(
                signature.to_bytes().to_vec(),
                *local_hsid_bytes,
                *remote_hsid_bytes,
            );
            let encrypted = encrypt(&shared_secret_key, &msg.to_vec()).unwrap();
            #[allow(clippy::cast_possible_truncation)]
            writer.write_u16(encrypted.len() as u16).await.unwrap();
            writer.write_all(&encrypted).await.unwrap();
            writer.flush().await.unwrap();

            println!("Handshake Complete. Performing Elliptical Diffie-Hellman key exchange.");
            // At this point, we know remote_public_key is filled
            let mut conn = Slave {
                reader: Some(reader),
                writer,
                msg_sender: msg_sender.clone(),
                response_sender,
                shared_secret_key: Arc::new(RwLock::new(shared_secret_key)),
            };
            _ = conn.spawn_communication();
            {
                let mut peers = peers.lock().unwrap();
                #[allow(clippy::cast_possible_truncation)]
                let idx = peers.len() as u8 + 1;
                peers.insert(idx, conn);
            }
            println!("Exchange Complete..");
            let peer = Peer::build(&generate_name(random_range(3..10)), &peer_addr.0);
            db_conn.insert_peer(peer).unwrap();
            msg_sender.send(IPCRes::Connected(peer_addr.0, 80)).unwrap();
        });
        Ok(PeerStatus::Connected)
    }

    /// Used to setup Slave to Master communication Pipeline
    /// # Errors
    pub fn setup_slave_communication(&mut self) -> Result<(), Box<dyn Error>> {
        let mut rec = self.response_receiver.resubscribe();
        let sen = self.msg_sender.clone();
        tokio::spawn(async move {
            while let Ok(msg) = rec.recv().await {
                let final_msg = match msg.1 {
                    Msg::Text(text) => IPCRes::Text(msg.0, text),
                    _ => {
                        continue;
                    }
                };
                _ = sen.send(final_msg);
            }
        });
        Ok(())
    }
}
