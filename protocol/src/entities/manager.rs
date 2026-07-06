use arti_client::{BootstrapBehavior, DataStream, TorClient, TorClientConfig, config::CfgPath};
use bincode::config as cfg;
use ed25519_dalek::{Signature, Verifier, VerifyingKey, ed25519::signature::rand_core::OsRng};
use futures::{StreamExt, stream::BoxStream};
use safelog::DisplayRedacted;
use std::{
    collections::HashMap,
    error::Error,
    str::FromStr,
    sync::{Arc, Mutex, RwLock, mpsc},
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt, WriteHalf},
    sync::broadcast,
};
use tor_cell::relaycell::msg::Connected;
use tor_hsservice::{HsId, HsNickname, OnionServiceConfig, RendRequest, RunningOnionService};
use x25519_dalek::{EphemeralSecret, PublicKey};

use crate::{
    comm::enums::{IPCCmd, IPCRes},
    config::parse_config,
    debug,
    entities::slave::Slave,
    msg::{Msg, PeerStatus},
};

pub struct Manager {
    tor_client: Arc<TorClient<tor_rtcompat::PreferredRuntime>>,
    /// NOTE: Only for assigning to `Slaves`, not to be used by manager itself
    response_sender: broadcast::Sender<(u8, Msg)>,

    stream: Option<BoxStream<'static, RendRequest>>,
    _service: Arc<RunningOnionService>,

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
        let arti_store = config.arti_key_store;
        let mut tor_config_builder = TorClientConfig::builder();
        let storage_builder = tor_config_builder.storage();
        let cfgpath = CfgPath::new(arti_store.clone());
        storage_builder.state_dir(cfgpath);
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
        let (response_sender, response_receiver) = broadcast::channel::<(u8, Msg)>(100);

        Ok(Self {
            tor_client,
            peers: Arc::new(Mutex::new(HashMap::new())),
            stream: Some(request_stream.boxed()),
            _service: service,
            msg_sender,
            response_receiver,
            response_sender,
        })
    }

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
                                            // loop {
                                            conn.connect_as_listener().await.unwrap();
                                            // {
                                            //     Ok(_) => break,
                                            //     Err(e) => {
                                            //         eprintln!("Cannot connect to peer.\n{e}");
                                            //         println!("Retrying...");
                                            //     }
                                            // }
                                            // }
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
    pub async fn connect_as_dialer(
        &mut self,
        peer_addr: (String, u16),
    ) -> Result<PeerStatus, Box<dyn Error>> {
        debug!("Connecting to peer...");
        let stream = loop {
            match self.tor_client.connect(&peer_addr).await {
                Ok(s) => break s,
                Err(e) => eprintln!("Error while connecting: {e}"),
            }
        };
        let (mut reader, mut writer) = tokio::io::split(stream);
        let mut buf = [0u8; 4096 * 4];
        let size = reader.read(&mut buf).await?;

        let (de_msg, _) =
            bincode::serde::decode_from_slice::<Msg, _>(&buf[..size], cfg::standard())?;
        debug!("received msg: {:?}", de_msg);

        let local_private_key = EphemeralSecret::random_from_rng(OsRng);
        let mut remote_public_key = None;
        println!("Performing X25519 Handshake.");
        self.x25519_handshake(&mut remote_public_key, &peer_addr, de_msg)?;
        let local_public_key = PublicKey::from(&local_private_key);
        let msg = Msg::PublicKey(*local_public_key.as_bytes());
        debug!("SENDING msg:\n{:?}", msg);
        let msg_bytes = bincode::serde::encode_to_vec(msg, cfg::standard())?;

        writer.write_all(&msg_bytes).await?;
        writer.flush().await?;
        println!("Handshake Complete. Performing Elliptical Diffie-Hellman key exchange.");
        // At this point, we know remote_public_key is filled
        let Some(remote_public_key) = remote_public_key else {
            return Err("Remote Public Key not set.".into());
        };
        let mut shared_secret_key = None;
        self.edhverify(
            &mut writer,
            local_private_key,
            remote_public_key,
            &mut shared_secret_key,
        )
        .await?;
        let msg_sen = self.msg_sender.clone();
        let response_sender = self.response_sender.clone();
        let Some(shared_secret_key) = shared_secret_key else {
            return Err("Couldn't get Shared Secret Key.".into());
        };
        let mut conn = Slave {
            reader: Some(reader),
            writer,
            msg_sender: msg_sen,
            response_sender,
            shared_secret_key: Arc::new(RwLock::new(shared_secret_key)),
        };
        _ = conn.spawn_communication();
        {
            let mut peers = self.peers.lock().unwrap();
            #[allow(clippy::cast_possible_truncation)]
            let idx = peers.len() as u8 + 1;
            peers.insert(idx, conn);
        }
        println!("Exchange Complete..");
        Ok(PeerStatus::Connected)
    }

    fn x25519_handshake(
        &self,
        remote_public_key: &mut Option<PublicKey>,
        peer_addr: &(String, u16),
        msg: Msg,
    ) -> Result<(), Box<dyn Error>> {
        let Msg::SignedAndPublicKey(signature, claimed_remote_public_key) = msg else {
            return Err("No Signed Public key found.".into());
        };
        let hsid = HsId::from_str(&peer_addr.0)?;
        let hsid_bytes = hsid.as_ref();
        let verifying_key = VerifyingKey::from_bytes(hsid_bytes)?;
        let signature: [u8; 64] = match signature.try_into() {
            Ok(s) => s,
            Err(e) => {
                return Err(format!("Cannot convert signature to Array, len {}", e.len()).into());
            }
        };
        let signature = Signature::from_bytes(&signature);
        verifying_key.verify(&claimed_remote_public_key, &signature)?;
        *remote_public_key = Some(PublicKey::from(claimed_remote_public_key));
        Ok(())
    }

    async fn edhverify(
        &mut self,
        writer: &mut WriteHalf<DataStream>,
        local_private_key: EphemeralSecret,
        remote_public_key: PublicKey,
        ssk: &mut Option<[u8; 32]>,
    ) -> Result<(), Box<dyn Error>> {
        let local_public_key = PublicKey::from(&local_private_key);
        let shared_secret_key = local_private_key.diffie_hellman(&remote_public_key);
        let mut buf = [0u8; 4096];
        let msg_size = bincode::serde::encode_into_slice(
            Msg::PublicKey(*local_public_key.as_bytes()),
            &mut buf,
            cfg::standard(),
        )?;
        writer.write_all(&buf[..msg_size]).await?;
        writer.flush().await?;
        *ssk = Some(*shared_secret_key.as_bytes());

        Ok(())
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
