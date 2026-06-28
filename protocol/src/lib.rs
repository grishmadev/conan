pub mod constants;
pub mod msg;
pub mod operations;
use arti_client::{BootstrapBehavior, TorClient, TorClientConfig, config::CfgPath};
use bincode::config;
use constants::{BOUNDED_CHANNEL_SIZE, SELF_PORT, TOR_RELAY_LIST_URL};
use ed25519_dalek::{
    Signature, Signer, SigningKey, Verifier, VerifyingKey, ed25519::signature::rand_core::OsRng,
};
use futures::{StreamExt, stream::BoxStream};
use msg::{Msg, PeerStatus};
use operations::{decrypt, encrypt};
use reqwest::Url;
use safelog::DisplayRedacted;
use serde::{Deserialize, Serialize};
use std::{
    env,
    error::Error,
    fmt::Debug,
    fs::{File, OpenOptions},
    io::{Read, Write},
    path::Path,
    str::FromStr,
    sync::Arc,
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    sync::mpsc::{Receiver, Sender},
};
use tor_cell::relaycell::msg::Connected;
use tor_hsservice::{HsId, HsNickname, OnionServiceConfig, RendRequest, RunningOnionService};
use x25519_dalek::{EphemeralSecret, PublicKey};

#[macro_export]
macro_rules! debug {
    ($($arg:tt)*) => {
        #[cfg(debug_assertions)]
        {
            println!("[DEBUG] {}", format_args!($($arg)*));
        }
    };
}

pub struct PeerConnection {
    pub peer_addr: Option<(String, u16)>,
    pub self_addr: Option<(HsId, u16)>,
    pub service: Option<Arc<RunningOnionService>>,
    pub stream: Option<BoxStream<'static, RendRequest>>,
    /// Shared secret key after diffie helmann exchange
    shared_secret_key: [u8; 32],
    /// Used for receiving message user sent via `local_msg_sx`
    local_msg_rx: Option<Receiver<Msg>>,

    /// Use this to send message to peer
    pub local_msg_sx: Sender<Msg>,

    /// Use this to receive message from peer
    pub remote_msg_rx: Receiver<Msg>,

    /// Used for sending messages that user push to `local_msg_sx`
    remote_msg_sx: Sender<Msg>,

    tor_client: Arc<TorClient<tor_rtcompat::PreferredRuntime>>,

    config_path: String,
}

impl PeerConnection {
    /// Creates a brand new circuit with Tor Relays and returns a `PeerConnection`
    /// that can be later used for chatting
    ///
    /// # Errors
    /// TODO: This function still needs some additional changes
    pub async fn create() -> Result<Self, Box<dyn Error>> {
        let mut tor_config_builder = TorClientConfig::builder();
        let mut config_path = env::var("HOME")?;
        config_path.push_str("/.conan/keys/signing_key");
        let storage_builder = tor_config_builder.storage();
        let cfgpath = CfgPath::new(config_path.clone());
        storage_builder.state_dir(cfgpath);
        let tor_config = tor_config_builder.build()?;

        let tor_client = TorClient::builder()
            .bootstrap_behavior(BootstrapBehavior::OnDemand)
            .config(tor_config)
            .create_bootstrapped()
            .await?;

        let nickname = HsNickname::new("conan-daemon".to_string())?;
        let svc_config = OnionServiceConfig::builder().nickname(nickname).build()?;
        let (service, request_stream) = match tor_client.launch_onion_service(svc_config)? {
            Some(s) => s,
            None => return Err("Could not launch onion service...".into()),
        };
        let hsid: tor_hsservice::HsId = match service.onion_address() {
            Some(s) => s,
            None => {
                return Err("No HsId found.".into());
            }
        };
        let (local_msg_sx, local_msg_rx) = tokio::sync::mpsc::channel::<Msg>(BOUNDED_CHANNEL_SIZE);
        let (remote_msg_sx, remote_msg_rx) =
            tokio::sync::mpsc::channel::<Msg>(BOUNDED_CHANNEL_SIZE);

        Ok(Self {
            peer_addr: None,
            self_addr: Some((hsid, SELF_PORT)),
            service: Some(service),
            stream: Some(request_stream.boxed()),
            local_msg_rx: Some(local_msg_rx),
            local_msg_sx,
            remote_msg_rx,
            remote_msg_sx,
            config_path,
            shared_secret_key: [0u8; 32],
            tor_client,
        })
    }

    /// Used to connect to peer
    ///
    /// # Errors
    ///
    /// TODO: This function is yet incomplete
    pub async fn connect(
        &mut self,
        peer_addr: HsId,
        port: u16,
    ) -> Result<PeerStatus, Box<dyn Error>> {
        println!("Connecting to {}.", peer_addr.display_unredacted());
        self.tor_client
            .connect((peer_addr.display_unredacted().to_string(), port))
            .await?;
        println!("Connected. Verifying integrity.");
        let msg = self.recv().await;
        if msg.is_none() {
            self.send(Msg::Begin).await?;
        }
        let local_private_key = EphemeralSecret::random_from_rng(OsRng);
        let mut remote_public_key = None;
        println!("Performing X25519 Handshake.");
        self.x25519_handshake(&local_private_key, &mut remote_public_key)
            .await?;
        println!("Handshake Complete. Performing Eliptical Diffie-Helmann key exchange.");
        // At this point, we know remote_public_key is filled
        if let Some(remote_public_key) = remote_public_key {
            self.edhverify(local_private_key, remote_public_key).await?;
            println!("Exchange Complete..");
        } else {
            println!("Did not receive remote public key.");
        }
        // if we connect first, peer sends us their public key;
        // let key = self.edhverify(remote_public_key)

        // self.peer_addr = Some((peer_addr, port));
        // Ok(PeerStatus::Connected)
        todo!()
    }

    /// # Panics
    /// Used to listen to incoming messages from peer and append it to `msg_rx`
    /// # Errors
    /// TODO: This function is yet incomplete
    pub async fn init_server(&mut self) -> Result<(), Box<dyn Error>> {
        let mut stream = self.stream.take().unwrap();
        while let Some(rendreq) = stream.next().await {
            if let Ok(mut s_stream_req) = rendreq.accept().await {
                debug!("Client Connected.");
                while let Some(streamreq) = s_stream_req.next().await {
                    let remote_msg_sx = self.remote_msg_sx.clone();
                    if let Ok(stream) = streamreq.accept(Connected::new_empty()).await {
                        let (mut reader, mut writer) = stream.split();
                        let local_private_key = EphemeralSecret::random_from_rng(OsRng);
                        let local_public_key = PublicKey::from(&local_private_key);
                        let signing_key = self.signing_key()?;
                        let signature = signing_key.sign(local_public_key.as_bytes());
                        let msg = Msg::SignedAndPublicKey(
                            signature.to_vec(),
                            *local_public_key.as_bytes(),
                        );
                        let shared_and_signed_key =
                            bincode::serde::encode_to_vec(msg, config::legacy())?;
                        debug!("Sending Signature & Public Key to peer.");
                        writer.write_all(&shared_and_signed_key).await?;
                        let mut vec = Vec::new();
                        let mut buf = [0u8; 1024];
                        debug!("Reading peer's public key.");
                        while reader.read(&mut buf).await? != 0 {
                            vec.extend_from_slice(&buf);
                        }
                        debug!("Parsing peer's public key.");
                        let (recv_msg, _) =
                            bincode::serde::decode_from_slice::<Msg, _>(&vec, config::legacy())?;

                        if let Msg::PublicKey(remote_public_key) = recv_msg {
                            let rpk = PublicKey::from(remote_public_key);
                            let ssk = local_private_key.diffie_hellman(&rpk);
                            self.shared_secret_key = *ssk.as_bytes();
                        }
                        debug!("Secret key set.");
                        let shared_secret_key = self.shared_secret_key;
                        // We spawn a tokio thread to listen for incoming messages from peer
                        // we convert it to clear message and push it to remote_msg_sx
                        tokio::spawn(async move {
                            let mut final_buf = vec![];
                            let mut buf = [0u8; 4096];
                            loop {
                                match reader.read(&mut buf).await {
                                    Ok(0) => {
                                        if final_buf.is_empty() {
                                            break;
                                        }
                                        let msg = decrypt(&shared_secret_key, &buf).unwrap();
                                        _ = remote_msg_sx.send(msg);
                                    }
                                    Ok(size) => {
                                        final_buf.extend_from_slice(&buf[..size]);
                                    }
                                    Err(e) => {
                                        eprintln!("Error found: {e}");
                                        break;
                                    }
                                }
                            }
                        });
                        let mut local_msg_rx =
                            self.local_msg_rx.take().ok_or("None recieved").unwrap();

                        // We spawn this tread to listen for messages that the user sends to peer
                        // convert it excrypted message, and send it over the writer
                        tokio::spawn(async move {
                            while let Some(msg) = local_msg_rx.recv().await {
                                let buf =
                                    bincode::serde::encode_to_vec(msg, config::legacy()).unwrap();
                                let encrypted_msg = encrypt(&shared_secret_key, &buf).unwrap();
                                _ = writer.write_all(&encrypted_msg).await;
                            }
                        });
                    }
                }
            }
        }

        Ok(())
    }

    /// Used to send messages `[msg::Msg]` to connected peer
    ///
    /// # Errors
    pub async fn send(&self, msg: Msg) -> Result<(), Box<dyn Error>> {
        self.local_msg_sx.send(msg).await?;
        Ok(())
    }

    #[inline]
    pub async fn recv(&mut self) -> Option<Msg> {
        self.remote_msg_rx.recv().await
    }

    /// Use this when reaching out to another peer
    ///
    /// # Errors
    pub async fn x25519_handshake(
        &mut self,
        local_private_key: &EphemeralSecret,
        remote_public_key: &mut Option<PublicKey>,
    ) -> Result<(), Box<dyn Error>> {
        let local_public_key = PublicKey::from(local_private_key);
        let reply = self.recv().await;
        if let Some(Msg::SignedAndPublicKey(signature, claimed_remote_public_key)) = reply {
            let hsid_str = match self.peer_addr.as_ref() {
                Some(s) => &s.0,
                None => {
                    return Err("No Peer Address found.".into());
                }
            };
            let hsid = HsId::from_str(hsid_str)?;
            let hsid_bytes = hsid.as_ref();
            let verifying_key = VerifyingKey::from_bytes(hsid_bytes)?;
            let signature: [u8; 64] = match signature.try_into() {
                Ok(s) => s,
                Err(e) => {
                    return Err(
                        format!("Cannot convert signature to Array, len {}", e.len()).into(),
                    );
                }
            };
            let signature = Signature::from_bytes(&signature);
            verifying_key.verify(&claimed_remote_public_key, &signature)?;
            *remote_public_key = Some(PublicKey::from(claimed_remote_public_key));
        }

        self.send(Msg::PublicKey(*local_public_key.as_bytes()))
            .await?;
        Ok(())
    }

    /// Verifies the key using Eliptical diffie-helmann, and saves it in memory for further use along the chat
    ///
    /// # Errors
    pub async fn edhverify(
        &mut self,
        local_private_key: EphemeralSecret,
        remote_public_key: PublicKey,
    ) -> Result<(), Box<dyn Error>> {
        let local_public_key = PublicKey::from(&local_private_key);
        let shared_secret_key = local_private_key.diffie_hellman(&remote_public_key);
        self.send(Msg::PublicKey(*local_public_key.as_bytes()))
            .await?;
        self.shared_secret_key = *shared_secret_key.as_bytes();

        Ok(())
    }
}

trait ED25519 {
    fn generate_keypair(&self) -> Result<(), Box<dyn Error>>;
    fn signing_key(&self) -> Result<SigningKey, Box<dyn Error>>;
}

impl ED25519 for PeerConnection {
    fn generate_keypair(&self) -> Result<(), Box<dyn Error>> {
        let signing_path = Path::new(&self.config_path);
        if !signing_path.exists() {
            let signing_key = SigningKey::generate(&mut OsRng);
            let mut signing_file = OpenOptions::new()
                .create(true)
                .truncate(true)
                .write(true)
                .open(signing_path)?;
            signing_file.write_all(signing_key.as_bytes())?;
        }
        Ok(())
    }
    fn signing_key(&self) -> Result<SigningKey, Box<dyn Error>> {
        self.generate_keypair()?;
        let mut signing_file = File::open(&self.config_path)?;
        let mut buf = [0u8; 32];
        signing_file.read_exact(&mut buf)?;
        let key = SigningKey::from_bytes(&buf);
        Ok(key)
    }
}

#[derive(Serialize, Deserialize, Debug)]
pub struct RelayDetail {
    n: String,
    f: String,
    a: Vec<String>,
    r: bool,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct TorResponse {
    pub version: String,
    pub build_revision: String,
    pub relays_published: String,
    pub relays: Vec<RelayDetail>,
    pub bridges_published: String,
    pub bridges: Vec<String>,
}

impl TorResponse {
    /// Responds back with available tor relays
    ///
    /// ```
    /// let relays = TorResponse::get_response()?;
    /// ```
    /// # Errors
    pub async fn get_response() -> Result<TorResponse, Box<dyn Error>> {
        let url = Url::from_str(TOR_RELAY_LIST_URL)?;
        let text = reqwest::get(url).await?.text().await?;
        let response: TorResponse = serde_json::from_str(&text)?;
        Ok(response)
    }
}
