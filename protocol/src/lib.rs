pub mod comm;
pub mod constants;
pub mod msg;
pub mod operations;
#[cfg(test)]
pub mod tests;
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
    io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt},
    sync::mpsc::{Receiver, Sender, error::SendError},
};
use tor_cell::relaycell::msg::Connected;
use tor_hsservice::{HsId, HsNickname, OnionServiceConfig, RendRequest, RunningOnionService};
use x25519_dalek::{EphemeralSecret, PublicKey};

use crate::constants::{ARTI_KEYSTORE, ARTI_PRIVATE_KEY};

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
    local_msg_rx: Option<Receiver<Vec<u8>>>,

    /// Use this to send message to peer
    pub local_msg_sx: Sender<Vec<u8>>,

    /// Use this to receive message from peer
    pub remote_msg_rx: Receiver<Vec<u8>>,

    /// Used for sending messages that user push to `local_msg_sx`
    remote_msg_sx: Sender<Vec<u8>>,

    tor_client: Option<Arc<TorClient<tor_rtcompat::PreferredRuntime>>>,
}

impl PeerConnection {
    pub fn mock() -> Self {
        let (local_msg_sx, local_msg_rx) =
            tokio::sync::mpsc::channel::<Vec<u8>>(BOUNDED_CHANNEL_SIZE);
        let (remote_msg_sx, remote_msg_rx) =
            tokio::sync::mpsc::channel::<Vec<u8>>(BOUNDED_CHANNEL_SIZE);
        Self {
            local_msg_sx,
            local_msg_rx: Some(local_msg_rx),
            remote_msg_rx,
            remote_msg_sx,
            tor_client: None,
            peer_addr: None,
            self_addr: None,
            stream: None,
            service: None,
            shared_secret_key: [0u8; 32],
        }
    }
    /// Creates a brand new circuit with Tor Relays and returns a `PeerConnection`
    /// that can be later used for chatting
    ///
    /// # Errors
    /// TODO: This function still needs some additional changes
    pub async fn create() -> Result<Self, Box<dyn Error>> {
        let mut tor_config_builder = TorClientConfig::builder();
        let mut config_path = env::var("HOME")?;
        config_path.push_str(ARTI_KEYSTORE);
        let storage_builder = tor_config_builder.storage();
        let cfgpath = CfgPath::new(config_path.clone());
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
        let (service, request_stream) = match tor_client.launch_onion_service(svc_config)? {
            Some(s) => s,
            None => return Err("Could not launch onion service...".into()),
        };
        let hsid: tor_hsservice::HsId = match service.onion_address() {
            Some(s) => s,
            None => return Err("No HsId found.".into()),
        };
        println!("Server Address: {}", hsid.display_unredacted());
        let (local_msg_sx, local_msg_rx) =
            tokio::sync::mpsc::channel::<Vec<u8>>(BOUNDED_CHANNEL_SIZE);
        let (remote_msg_sx, remote_msg_rx) =
            tokio::sync::mpsc::channel::<Vec<u8>>(BOUNDED_CHANNEL_SIZE);

        // adding actual path for config file
        config_path.push_str(ARTI_PRIVATE_KEY);

        Ok(Self {
            peer_addr: None,
            self_addr: Some((hsid, SELF_PORT)),
            service: Some(service),
            stream: Some(request_stream.boxed()),
            local_msg_rx: Some(local_msg_rx),
            local_msg_sx,
            remote_msg_rx,
            remote_msg_sx,
            shared_secret_key: [0u8; 32],
            tor_client: Some(tor_client),
        })
    }

    /// Used to listen to incoming messages from peer and append it to `msg_rx`
    /// # Errors
    pub async fn init_server(&mut self) -> Result<(), Box<dyn Error>> {
        let mut stream = self.stream.take().ok_or("No stream assigned yet.")?;
        while let Some(rendreq) = stream.next().await {
            if let Ok(mut s_stream_req) = rendreq.accept().await {
                debug!("Client Connected.");
                while let Some(streamreq) = s_stream_req.next().await {
                    if let Ok(stream) = streamreq.accept(Connected::new_empty()).await {
                        let (reader, writer) = tokio::io::split(stream);
                        self.handle_stream(reader, writer).await?;
                    }
                }
            }
        }

        Ok(())
    }

    /// # Errors
    pub async fn handle_stream<T, S>(&mut self, reader: T, writer: S) -> Result<(), Box<dyn Error>>
    where
        T: AsyncReadExt + Unpin + Send + 'static,
        S: AsyncWriteExt + Unpin + Send + 'static,
    {
        let mut reader = reader;
        let mut writer = writer;
        // let (mut reader, mut writer) = tokio::io::split(stream);
        self.connect_as_listener(&mut reader, &mut writer).await?;
        debug!("Secret key set.");

        let remote_msg_sx = self.remote_msg_sx.clone();
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
                        _ = remote_msg_sx.send(final_buf.clone());
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
        let mut local_msg_rx = self.local_msg_rx.take().ok_or("None recieved")?;

        // We spawn this tread to listen for messages that the user sends to peer
        // convert it excrypted message, and send it over the writer
        tokio::spawn(async move {
            while let Some(msg) = local_msg_rx.recv().await {
                _ = writer.write_all(&msg).await;
            }
        });
        Ok(())
    }

    /// Used to connect to dialer
    ///
    /// # Errors
    pub async fn connect_as_listener<T, S>(
        &mut self,
        reader: &mut T,
        writer: &mut S,
    ) -> Result<(), Box<dyn Error>>
    where
        T: AsyncReadExt + Unpin,
        S: AsyncWriteExt + Unpin,
    {
        let local_private_key = EphemeralSecret::random_from_rng(OsRng);
        let local_public_key = PublicKey::from(&local_private_key);
        let signing_key = self.signing_key()?;
        let signature = signing_key.sign(local_public_key.as_bytes());
        let msg = Msg::SignedAndPublicKey(signature.to_vec(), *local_public_key.as_bytes());
        let shared_and_signed_key = bincode::serde::encode_to_vec(msg, config::legacy())?;
        debug!("Sending Signature & Public Key to peer.");
        writer.write_all(&shared_and_signed_key).await?;
        let mut vec = Vec::new();
        let mut buf = [0u8; 1024];
        debug!("Reading peer's public key.");
        while reader.read(&mut buf).await? != 0 {
            vec.extend_from_slice(&buf);
        }
        debug!("Parsing peer's public key.");
        let (recv_msg, _) = bincode::serde::decode_from_slice::<Msg, _>(&vec, config::legacy())?;

        if let Msg::PublicKey(remote_public_key) = recv_msg {
            let rpk = PublicKey::from(remote_public_key);
            let ssk = local_private_key.diffie_hellman(&rpk);
            self.shared_secret_key = *ssk.as_bytes();
        }
        Ok(())
    }

    #[inline]
    async fn connect_with_addr(
        &mut self,
        peer_addr: (HsId, u16),
    ) -> Result<(), Box<arti_client::Error>> {
        let tuple = (peer_addr.0.display_unredacted().to_string(), peer_addr.1);
        self.peer_addr = Some(tuple.clone());
        self.tor_client.as_ref().unwrap().connect(tuple).await?;
        Ok(())
    }

    /// Used to connect to listener
    ///
    /// # Errors
    ///
    pub async fn connect_as_dialer(
        &mut self,
        peer_addr: (HsId, u16),
    ) -> Result<PeerStatus, Box<dyn Error>> {
        self.connect_with_addr(peer_addr).await?;
        println!("Connected. Verifying integrity.");
        let msg = self
            .recv_raw()
            .await
            .ok_or("Did not receive Message from peer..")?;
        // let mut final_buf = Vec::new();
        // let mut buf = [0u8; 4096];
        // loop {
        //     match reader.read(&mut buf).await {
        //         Ok(0) => break,
        //         Ok(s) => {
        //             final_buf.extend_from_slice(&buf[..s]);
        //         }
        //         Err(e) => {
        //             eprintln!("Could not read buffer, {e}");
        //             break;
        //         }
        //     }
        // }

        let (de_msg, _) = bincode::serde::decode_from_slice::<Msg, _>(&msg, config::legacy())?;
        let local_private_key = EphemeralSecret::random_from_rng(OsRng);
        let mut remote_public_key = None;
        println!("Performing X25519 Handshake.");
        self.x25519_handshake(&mut remote_public_key, de_msg)?;

        let local_public_key = PublicKey::from(&local_private_key);
        let msg = Msg::PublicKey(*local_public_key.as_bytes());
        let msg_bytes = bincode::serde::encode_to_vec(msg, config::legacy())?;
        self.send_raw(msg_bytes).await?;
        println!("Handshake Complete. Performing Eliptical Diffie-Helmann key exchange.");

        // At this point, we know remote_public_key is filled
        if let Some(remote_public_key) = remote_public_key {
            self.edhverify(local_private_key, remote_public_key).await?;
            println!("Exchange Complete..");
            Ok(PeerStatus::Connected)
        } else {
            println!("Did not receive remote public key.");
            Ok(PeerStatus::NotFound)
        }
    }

    /// Use to send bytes to `local_msg_sx` as is
    ///
    /// # Errors
    /// Follows `[tokio::sync::mpsc::SendError<Vec<u8>>]`
    #[inline]
    pub async fn send_raw(&self, msg: Vec<u8>) -> Result<(), SendError<Vec<u8>>> {
        self.local_msg_sx.send(msg).await
    }

    /// Used to send messages `[msg::Msg]` to connected peer
    ///
    /// # Errors
    pub async fn send(&self, msg: Msg) -> Result<(), Box<dyn Error>> {
        let msg = bincode::serde::encode_to_vec(msg, config::legacy())?;
        let encrypted_msg = encrypt(&self.shared_secret_key, &msg)?;
        self.send_raw(encrypted_msg).await?;
        Ok(())
    }

    /// Recieves data from peer as is.
    #[inline]
    pub async fn recv_raw(&mut self) -> Option<Vec<u8>> {
        self.remote_msg_rx.recv().await
    }

    /// Recieves decrypted `[Msg::msg]` from peer.
    ///
    /// # Errors
    /// Errors from possible corrupted decryption
    pub async fn recv(&mut self) -> Result<Option<Msg>, Box<dyn Error>> {
        let encr_msg = match self.recv_raw().await {
            Some(s) => s,
            None => return Ok(None),
        };
        let msg = decrypt(&self.shared_secret_key, &encr_msg)?;
        Ok(Some(msg))
    }

    /// Stage 1 of the 2 Stage Encryption process after tor connection
    /// Use this when reaching out to another peer
    ///
    /// # Errors
    pub fn x25519_handshake(
        &mut self,
        remote_public_key: &mut Option<PublicKey>,
        msg: Msg,
    ) -> Result<(), Box<dyn Error>> {
        if let Msg::SignedAndPublicKey(signature, claimed_remote_public_key) = msg {
            let hsid_str = match self.peer_addr.as_ref() {
                Some(s) => &s.0,
                None => return Err("No Peer Address found.".into()),
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
        Ok(())
    }

    /// Last Stage of 2 stage encryption
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
        let home_dir = env::var("HOME").unwrap();
        let signing_path = Path::new(&home_dir).join(Path::new(ARTI_KEYSTORE));
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
        // self.generate_keypair()?;
        let home_dir = env::var("HOME").unwrap();
        let path = Path::new(&home_dir)
            .join(Path::new(ARTI_KEYSTORE))
            .join(ARTI_PRIVATE_KEY);
        let mut signing_file = File::open(&path)?;
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
