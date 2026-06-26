use arti_client::{BootstrapBehavior, TorClient};
use futures::{Stream, StreamExt};
use rand::Rng;
use reqwest::Url;
use rustls::pki_types::Ipv4Addr;
use safelog::DisplayRedacted;
use serde::{Deserialize, Serialize};
use std::{
    error::Error,
    fmt::Debug,
    str::FromStr,
    sync::{Arc, mpsc},
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    sync::mpsc::{UnboundedReceiver, UnboundedSender},
};
use tor_cell::relaycell::msg::Connected;
use tor_hsservice::{HsNickname, OnionServiceConfig, RendRequest, RunningOnionService};
use x25519_dalek::{EphemeralSecret, PublicKey};

pub mod msg;
use crate::msg::{Msg, PeerStatus};

const SELF_PORT: u16 = 80;
pub struct PeerConnection {
    pub peer_addr: Option<(String, u16)>,
    pub self_addr: Option<(String, u16)>,
    pub service: Option<Arc<RunningOnionService>>,
    pub stream: impl Stream<Item = RendRequest>,
    pub msg_rx: UnboundedReceiver<Msg>,
    pub msg_sx: UnboundedSender<Msg>,
}

impl PeerConnection {
    /// Creates a brand new circuit with Tor Relays and returns a `PeerConnection`
    /// that can be later used for chatting
    ///
    /// TODO: This function still needs some additional changes
    pub async fn create() -> Result<Self, Box<dyn Error>> {
        let tor_client = TorClient::builder()
            .bootstrap_behavior(BootstrapBehavior::OnDemand)
            .create_bootstrapped()
            .await?;

        let nickname = HsNickname::new("conan-daemon".to_string())?;
        let svc_config = OnionServiceConfig::builder().nickname(nickname).build()?;
        let (service, mut request_stream) = client.launch_onion_service(svc_config)?.unwrap();
        let hsid: tor_hscrypto::pk::HsId = service.onion_address().unwrap();
        let (sx, rx) = tokio::sync::mpsc::unbounded_channel::<Msg>();

        Self {
            peer_addr: None,
            self_addr: Some((hsid.display_unredacted().to_string(), SELF_PORT)),
            service: Some(service),
            stream: request_stream,
            msg_rx: rx,
            msg_sx: sx,
        }
    }

    /// Used to connect to peer
    ///
    /// TODO: This function is yet incomplete
    pub async fn connect(peer_addr: String, port: u16) -> Result<PeerStatus, Box<dyn Error>> {
        Ok(PeerStatus::Connected)
    }

    /// Used to listen to incoming messages from peer
    /// TODO: This function is yet incomplete
    pub async fn listen(&self) -> Result<(), Box<dyn Error>> {
        Ok(())
    }

    /// Used to send messages [msg::Msg] to connected peer
    ///
    /// TODO: This function is yet incomplete
    pub async fn send(&self) -> Result<(), Box<dyn Error>> {
        Ok(())
    }
}
pub async fn init(addr: (String, u16)) -> Result<Vec<String>, Box<dyn Error>> {
    let client = TorClient::builder()
        .bootstrap_behavior(BootstrapBehavior::OnDemand)
        .create_bootstrapped()
        .await?;
    let nickname = HsNickname::new("conan-daemon".to_string())?;
    let svc_config = OnionServiceConfig::builder().nickname(nickname).build()?;
    let (service, mut request_stream) = client.launch_onion_service(svc_config)?.unwrap();

    let hsid: tor_hscrypto::pk::HsId = service.onion_address().unwrap();
    println!("onion bytes: {}", hsid.display_unredacted());

    while let Some(rendreq) = request_stream.next().await {
        tokio::spawn(async move {
            if let Ok(mut msg) = rendreq.accept().await {
                println!("client connected");
                while let Some(streq) = msg.next().await {
                    if let Ok(mut stream) = streq.accept(Connected::new_empty()).await {
                        let mut buf = [0u8; 4096];
                        let size = stream.read(&mut buf).await.unwrap();
                        println!("request: {}", String::from_utf8_lossy(&buf[..size]));
                        let mysecret = EphemeralSecret::random_from_rng(Rng);
                        let local_public_key = PublicKey::from(&mysecret);
                        let remote_public_key = PublicKey::from(remote_public_key);
                        let _ = stream
                            .write_all(
                                b"HTTP/1.1 200 OK\r\n\
                        Content-Type: text/plain\r\n\
                        Content-Length: 12\r\n\
                        Connection: keep-alive\r\n\
                        \r\n\
                        Hello World\n",
                            )
                            .await;
                        if let Err(e) = stream.shutdown().await {
                            eprintln!("Could not shut stream.\n{e}");
                        }
                    }
                }
            }
        });
    }
    let mut stream = client.connect(addr.clone()).await?;

    let msg = format!(
        "GET / HTTP/1.1\r\n\
     Host: {}\r\n\
     User-Agent: Mozilla/5.0 (Windows NT 10.0; rv:109.0) Gecko/20100101 Firefox/115.0\r\n\
     Connection: close\r\n\r\n",
        addr.0
    )
    .into_bytes();
    stream.write_all(&msg).await?;
    stream.flush().await?;
    let mut response = Vec::new();
    let mut buf = [0u8; 4096];
    loop {
        let size = stream.read(&mut buf).await?;
        if size == 0 {
            let res = String::from_utf8_lossy(&response)
                .split('\n')
                .map(|s| s.trim().to_owned())
                .filter(|f| !f.is_empty())
                .collect::<Vec<String>>();
            return Ok(res);
        }
        response.extend_from_slice(&buf[..size]);
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
        let url = Url::from_str("https://onionoo.torproject.org/summary?type=relay&running=true")?;
        let text = reqwest::get(url).await?.text().await?;
        let response: TorResponse = serde_json::from_str(&text)?;
        Ok(response)
    }
}
