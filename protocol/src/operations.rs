use crate::comm::enums::IPCRes;
use crate::comm::error::ConanError;
use crate::config::parse_config;
use crate::entities::slave::Slave;
use crate::msg::Internal;
use crate::{constants::ARTI_PRIVATE_KEY, msg::Msg};
use arti_client::{DataStream, TorClient};
use base64::Engine;
use crypto::{
    aead::{self, EncryptedMessage, MessageKey},
    ratchet::{RatchetMessage, RatchetSession},
};
use database::entities::peer::{Peer, PeerData};
use database::rusqlite::Connection;
use ed25519_dalek::{Signature, Verifier, VerifyingKey, ed25519::signature::rand_core::OsRng};
use extras::generate_name;
use futures::AsyncReadExt as FutureRead;
use safelog::DisplayRedacted;
use ssh_encoding::Decode;
use std::collections::HashMap;
use std::sync::RwLock;
use std::{error::Error, fs::File, io::Read, str::FromStr, sync::Arc};
use tokio::io::{AsyncReadExt, AsyncWriteExt, ReadHalf, WriteHalf};
use tokio::sync::broadcast;
use tor_hsservice::{HsId, RunningOnionService};
use tor_llcrypto::pk::ed25519::ExpandedKeypair;
use tor_rtcompat::PreferredRuntime;
use x25519_dalek::{EphemeralSecret, PublicKey, StaticSecret};

/// Handshake-only encryption using the static shared secret.
/// Used during the initial handshake before ratchet is established.
fn handshake_encrypt(
    shared_secret_key: &[u8; 32],
    data: &[u8],
) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    let key = MessageKey::from_bytes(*shared_secret_key);
    let msg = aead::encrypt(&key, 0, data)?;
    let mut out = msg.nonce.to_be_bytes().to_vec();
    out.extend_from_slice(&msg.ciphertext);
    Ok(out)
}

/// Handshake-only decryption using the static shared secret.
fn handshake_decrypt(
    shared_secret_key: &[u8; 32],
    data: &[u8],
) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    if data.len() < 8 {
        return Err("Data too short for nonce".into());
    }
    let nonce = u64::from_be_bytes(data[..8].try_into()?);
    let ciphertext = data[8..].to_vec();
    let key = MessageKey::from_bytes(*shared_secret_key);
    let msg = EncryptedMessage { nonce, ciphertext };
    Ok(aead::decrypt(&key, &msg)?)
}

/// Used to retrieve signing key for self tor server
/// # Errors
/// # Panics
pub async fn signing_key() -> Result<ExpandedKeypair, Box<dyn Error>> {
    let config = parse_config()?;
    let mut key_store_path = config.arti_key_store;
    key_store_path.push_str(ARTI_PRIVATE_KEY);
    let mut signing_file = File::open(key_store_path)?;
    let mut content = String::new();
    signing_file.read_to_string(&mut content)?;
    let filtered_content = content
        .lines()
        .filter(|l| !l.contains("---"))
        .collect::<String>();
    let payload = base64::engine::general_purpose::STANDARD.decode(filtered_content)?;
    let mut payload_slice = payload.as_slice();

    let mut magic = [0u8; 15];
    FutureRead::read_exact(&mut payload_slice, &mut magic).await?;
    if &magic != b"openssh-key-v1\0" {
        return Err("Invalid SSH magic".into());
    }
    let _cipher = String::decode(&mut payload_slice)?;
    let _kdf = String::decode(&mut payload_slice)?;
    let _kdf_opts = Vec::<u8>::decode(&mut payload_slice)?;
    let _num_keys = u32::decode(&mut payload_slice)?;
    let _outer_pub = Vec::<u8>::decode(&mut payload_slice)?;

    let inner_block_bytes = Vec::<u8>::decode(&mut payload_slice)?;

    let mut inner_bytes = inner_block_bytes.as_slice();

    let _check1 = u32::decode(&mut inner_bytes)?;
    let _check2 = u32::decode(&mut inner_bytes)?;

    let algo_name = String::decode(&mut inner_bytes)?;
    if &algo_name != "ed25519-expanded@spec.torproject.org" {
        return Err("Algorithm name mismatch".into());
    }

    let _pub_key = Vec::<u8>::decode(&mut inner_bytes)?;
    let priv_key = Vec::<u8>::decode(&mut inner_bytes)?;
    let priv_key = priv_key.try_into().unwrap();
    let expanded_key = ExpandedKeypair::from_secret_key_bytes(priv_key).unwrap();
    Ok(expanded_key)
}

/// Performs Curve25519 Handshake
/// # Errors
pub fn x25519_handshake(
    remote_public_key: &mut Option<PublicKey>,
    local_public_key: PublicKey,
    peer_addr: &str,
    msg: Msg,
) -> Result<(), Box<dyn Error>> {
    let Msg::SignedAndPublicKey(signature, claimed_local_public_key, claimed_remote_public_key) =
        msg
    else {
        return Err("No Signed Public key found.".into());
    };
    let local_public_key = local_public_key.as_bytes();
    if local_public_key != &claimed_local_public_key {
        return Err("local key mismatch. Aborting.".into());
    }
    let hsid = HsId::from_str(peer_addr)?;
    let hsid_bytes = hsid.as_ref();
    let verifying_key = VerifyingKey::from_bytes(hsid_bytes)?;
    let signature = Signature::try_from(&signature[..])?;
    let mut combined_key = vec![];
    combined_key.extend_from_slice(local_public_key);
    combined_key.extend_from_slice(&claimed_remote_public_key);
    verifying_key.verify(&combined_key, &signature)?;
    *remote_public_key = Some(PublicKey::from(claimed_remote_public_key));
    Ok(())
}

/// Performs Elliptical Diffie Hellman key exchange.
pub fn edhverify(
    local_private_key: EphemeralSecret,
    remote_public_key: PublicKey,
    ssk: &mut Option<[u8; 32]>,
) {
    let shared_secret_key = local_private_key.diffie_hellman(&remote_public_key);
    *ssk = Some(*shared_secret_key.as_bytes());
}

#[must_use]
/// Derive a ratchet key pair from the shared secret via HKDF.
/// Both sides derive the same Bob ratchet key from the shared secret,
/// so Alice can compute Bob's ratchet public key independently.
/// # Panics
pub fn derive_bob_ratchet_key(shared_secret: &[u8; 32]) -> (StaticSecret, PublicKey) {
    use crypto::aead::hkdf_derive;
    let derived = hkdf_derive::<32>(shared_secret, None, b"conan-v1-bob-ratchet")
        .expect("HKDF cannot fail with valid-length output");
    let priv_key = StaticSecret::from(derived);
    let pub_key = PublicKey::from(&priv_key);
    (priv_key, pub_key)
}

/// Perform function of listener once called
/// Returns (`RatchetSession`, `remote_hsid`) on success
/// # Errors
/// # Panics
pub async fn listener_actor(
    reader: &mut ReadHalf<DataStream>,
    writer: &mut WriteHalf<DataStream>,
    assign_remote_hsid: &mut Option<String>,
    local_hsid: HsId,
) -> Result<(RatchetSession, Option<String>), Box<dyn Error>> {
    // reading dialer's x25519 public key.
    let size = reader.read_u16().await? as usize;
    let mut buf = vec![0u8; size];
    let size = reader.read_exact(&mut buf).await?;
    let recv_msg = Msg::from_bytes(&buf[..size]);

    let Msg::PublicKey(remote_public_key) = recv_msg else {
        return Err("Did not receive remote public key. aborting.".into());
    };
    let local_private_key = EphemeralSecret::random_from_rng(OsRng);
    let local_public_key = PublicKey::from(&local_private_key).to_bytes();
    let signing_key = signing_key().await?;

    // creating signature using local ed25519 private key and stacking local and remote
    // ephemeral keys
    let mut data = vec![];
    data.extend_from_slice(&remote_public_key);
    data.extend_from_slice(&local_public_key);
    let data: [u8; 64] = data.try_into().unwrap();
    let signature = signing_key.sign(&data);

    // remote entities should always be put before local entities
    let msg = Msg::SignedAndPublicKey(
        signature.to_bytes().to_vec(),
        remote_public_key,
        local_public_key,
    );

    let payload = msg.to_vec();
    // writing message to dialer
    println!("Sending Signature & Public Key to peer.");
    #[allow(clippy::cast_possible_truncation)]
    writer.write_u16(payload.len() as u16).await?;
    writer.write_all(&payload).await?;
    writer.flush().await?;

    let rpk = PublicKey::from(remote_public_key);
    let shared_secret_key = local_private_key.diffie_hellman(&rpk);
    let shared_secret_bytes = *shared_secret_key.as_bytes();

    println!("Parsing peer's public key.");
    let size = reader.read_u16().await? as usize;
    let mut buf = vec![0u8; size];
    let size = reader.read_exact(&mut buf).await?;

    // decrypting and reading here.
    let decrypted = handshake_decrypt(&shared_secret_bytes, &buf[..size])?;
    let recv_msg = Msg::from_bytes(&decrypted);
    let Msg::SignedAndPublicKey(signature, claimed_local_hsid_bytes, claimed_remote_hsid_bytes) =
        recv_msg
    else {
        return Err("No Signed Key found from Peer.".into());
    };
    let local_hsid_bytes = local_hsid.as_ref();
    let remote_hsid = HsId::from(claimed_remote_hsid_bytes);
    println!("Peer's Address: {}", remote_hsid.display_unredacted());
    *assign_remote_hsid = Some(remote_hsid.display_unredacted().to_string());
    if local_hsid_bytes != &claimed_local_hsid_bytes {
        return Err("HsId key mismatch. Dropping connection.".into());
    }

    // prepare data that we assume was same when prepared by dialer
    // and verify it with dialer's claimed ed25519 public key
    let mut data = vec![];
    data.extend_from_slice(&local_public_key);
    data.extend_from_slice(&remote_public_key);
    let data = data.as_slice();
    let signature = Signature::from_bytes(&signature.try_into().unwrap());
    let verifying_key = VerifyingKey::from_bytes(&claimed_remote_hsid_bytes)?;
    println!("Verifying Peer's Claim.");
    verifying_key.verify(data, &signature)?;

    // if verified, we proceed to send a final `Verified` message to dialer
    println!("Verified. Sending Approval.");
    let verified = handshake_encrypt(&shared_secret_bytes, Msg::Verified.to_vec().as_slice())?;
    #[allow(clippy::cast_possible_truncation)]
    writer.write_u16(verified.len() as u16).await?;
    writer.write_all(&verified).await?;
    writer.flush().await?;

    println!("Verification Complete.");

    // Initialize ratchet session
    // Both sides derive Bob's ratchet key from the shared secret
    let (bob_dh, _) = derive_bob_ratchet_key(&shared_secret_bytes);
    let session = RatchetSession::init_receiver(&shared_secret_bytes, bob_dh)?;

    Ok((session, assign_remote_hsid.clone()))
}

/// Act as a Dialer
/// Returns `RatchetSession` on success
/// # Panics
/// # Errors
pub async fn dialer_actor<R, W>(
    reader: &mut ReadHalf<R>,
    writer: &mut WriteHalf<W>,
    local_hsid: HsId,
    peer_addr: &str,
) -> Result<RatchetSession, Box<dyn Error>>
where
    R: AsyncReadExt,
    W: AsyncWriteExt,
{
    let local_private_key = EphemeralSecret::random_from_rng(OsRng);
    let local_public_key = PublicKey::from(&local_private_key);
    // writing x25519 public key to stream
    let msg = Msg::PublicKey(local_public_key.to_bytes());
    let msg_bytes = msg.to_vec();
    #[allow(clippy::cast_possible_truncation)]
    writer.write_u16(msg_bytes.len() as u16).await?;
    writer.write_all(&msg_bytes).await?;
    writer.flush().await?;

    // Reading listener's signature of local and remote x25519 public key
    let size = reader.read_u16().await? as usize;
    let mut buf = vec![0u8; size];
    let size = reader.read_exact(&mut buf).await?;
    let de_msg = Msg::from_bytes(&buf[..size]);

    let mut remote_public_key = None;
    println!("Performing X25519 Handshake.");
    x25519_handshake(&mut remote_public_key, local_public_key, peer_addr, de_msg)?;
    let Some(remote_public_key) = remote_public_key else {
        return Err("Remote Public Key not set.".into());
    };
    println!("Confirmed. Assigning Shared Secret Key using Diffie Hellman key exchange.");
    let mut ssk = None;
    edhverify(local_private_key, remote_public_key, &mut ssk);
    let Some(shared_secret_key) = ssk else {
        return Err("Couldn't get Shared Secret Key.".into());
    };
    let signing_key = signing_key().await?;

    // preparing message containing signed combined key of remote and local x25519 public key
    // and remote and local ed25519 public key on an encrypted channel using shared secret
    // key we calculated earlier.
    let mut data = vec![];
    data.extend_from_slice(remote_public_key.as_bytes());
    data.extend_from_slice(local_public_key.as_bytes());
    let data: [u8; 64] = data.try_into().unwrap();
    println!("Signing, Encrypting, Sending Message for approval.");
    let signature = signing_key.sign(&data);
    let local_hsid_bytes = local_hsid.as_ref();
    let remote_hsid = HsId::from_str(peer_addr)?;
    println!("Peer's Address: {}", remote_hsid.display_unredacted());
    let remote_hsid_bytes = remote_hsid.as_ref();
    let msg = Msg::SignedAndPublicKey(
        signature.to_bytes().to_vec(),
        *remote_hsid_bytes,
        *local_hsid_bytes,
    );

    // encrypting and sending here.
    let encrypted = handshake_encrypt(&shared_secret_key, &msg.to_vec())?;
    #[allow(clippy::cast_possible_truncation)]
    writer.write_u16(encrypted.len() as u16).await?;
    writer.write_all(&encrypted).await?;
    writer.flush().await?;

    // waiting for listener's approval
    println!("Waiting for approval..");
    let size = reader.read_u16().await?;
    let mut buf = vec![0u8; size as usize];
    reader.read_exact(&mut buf).await?;
    let decrypted = handshake_decrypt(&shared_secret_key, &buf)?;
    let Msg::Verified = Msg::from_bytes(&decrypted) else {
        return Err("Didn't receive Approval, Aborting.".into());
    };

    // Initialize ratchet session
    // Both sides derive Bob's ratchet key from the shared secret
    let (_, bob_dh_pub) = derive_bob_ratchet_key(&shared_secret_key);
    let session = RatchetSession::init_sender(&shared_secret_key, &bob_dh_pub.to_bytes())?;

    Ok(session)
}

/// Encrypts message before writing to writer using Double Ratchet
/// # Errors
pub async fn send<T>(
    writer: &mut WriteHalf<T>,
    msg: Msg,
    ratchet: &Arc<tokio::sync::RwLock<RatchetSession>>,
) -> Result<(), Box<dyn Error>>
where
    T: AsyncReadExt + AsyncWriteExt,
{
    let msg = msg.to_vec();
    let ratchet_msg = {
        let mut ratchet = ratchet.write().await;
        ratchet.encrypt(&msg, b"")?
    };
    let serialized = bincode::serde::encode_to_vec(&ratchet_msg, bincode::config::standard())?;
    #[allow(clippy::cast_possible_truncation)]
    writer.write_u16(serialized.len() as u16).await?;
    writer.write_all(&serialized).await?;
    writer.flush().await?;
    Ok(())
}

/// Decrypts message before returning using Double Ratchet
/// # Errors
pub async fn recv<T>(
    reader: &mut ReadHalf<T>,
    ratchet: Arc<tokio::sync::RwLock<RatchetSession>>,
) -> Result<Vec<u8>, Box<dyn Error>>
where
    T: AsyncReadExt + AsyncWriteExt,
{
    let size = reader.read_u16().await?;
    let mut buf = vec![0u8; size as usize];
    reader.read_exact(&mut buf).await?;

    let ratchet_msg: RatchetMessage =
        bincode::serde::decode_from_slice(&buf, bincode::config::standard())
            .map_err(|e| format!("Failed to deserialize ratchet message: {e}"))?
            .0;

    let mut ratchet = ratchet.write().await;
    ratchet
        .decrypt(&ratchet_msg, b"")
        .map_err(|e| format!("Decryption failed: {e}").into())
}
/// Connects to Peer's Tor Address as a dialer (Seeking connection)
/// # Errors
/// # Panics
pub async fn connect_as_dialer(
    tor_client: Arc<TorClient<PreferredRuntime>>,
    msg_sender: broadcast::Sender<IPCRes>,
    dbconn: &mut Connection,
    response_sender: broadcast::Sender<(u16, Internal)>,
    peers: Arc<RwLock<HashMap<u16, Slave>>>,
    service: Arc<RunningOnionService>,
    addr: String,
    port: u16,
) -> Result<(), Box<dyn Error>> {
    if let Some(hsid) = service.onion_address()
        && addr == hsid.display_unredacted().to_string()
    {
        msg_sender.send(IPCRes::Error("Cannot connect to Self.".to_string()))?;
        return Ok(());
    }
    // checking if peer is already in our connection
    {
        let peer = dbconn.get_peer_from_addr(&addr)?;
        if let Some(peer) = peer {
            #[allow(clippy::cast_possible_truncation)]
            if peers.read().unwrap().contains_key(&peer.id) {
                msg_sender.send(IPCRes::Connected(peer.id, true))?;
                msg_sender.send(IPCRes::Notification(format!(
                    "Already connected to {}",
                    peer.name
                )))?;
                return Ok(());
            }
        }
    }
    let service = Arc::clone(&service);
    let msg_sender = msg_sender.clone();
    for i in 1..=5 {
        println!("Connecting to {addr}");
        match single_connect_as_dialer(
            Arc::clone(&tor_client),
            Arc::clone(&service),
            msg_sender.clone(),
            dbconn,
            response_sender.clone(),
            Arc::clone(&peers),
            addr.clone(),
            port,
        )
        .await
        {
            Ok(_) => {
                break;
            }
            Err(e) => eprintln!("Error while connecting:\n{e:?}"),
        }
        if i == 5 {
            msg_sender
                .send(IPCRes::Error(
                    "Failed to Connect.\nPlease check your Internet connection".to_string(),
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
    Ok(())
}

pub async fn single_connect_as_dialer(
    tor_client: Arc<TorClient<PreferredRuntime>>,
    service: Arc<RunningOnionService>,
    msg_sender: broadcast::Sender<IPCRes>,
    dbconn: &mut Connection,
    response_sender: broadcast::Sender<(u16, Internal)>,
    peers: Arc<RwLock<HashMap<u16, Slave>>>,
    addr: String,
    port: u16,
) -> Result<u16, ConanError> {
    let Ok(stream) = tor_client.connect(&(addr.clone(), port)).await else {
        return Err(ConanError::ConnectionError);
    };
    let (mut reader, mut writer) = tokio::io::split(stream);

    let local_hsid = service
        .onion_address()
        .ok_or("Onion Address Not found.")
        .unwrap();
    let session = match dialer_actor(&mut reader, &mut writer, local_hsid, &addr).await {
        Ok(s) => s,
        Err(e) => {
            let msg = format!("Error while Reaching out.\n{e:?}");
            msg_sender.send(IPCRes::Error(msg.clone())).unwrap();
            return Err(ConanError::ConnectionError);
        }
    };
    let known = dbconn.get_peer_from_addr(&addr).unwrap();
    let trans = dbconn.transaction().unwrap();
    let name = generate_name(3..10);
    let idx = if let Some(known_peer) = known {
        known_peer.id
    } else {
        let peer = Peer::build(&name, &addr, true);
        let peer = trans.insert_peer(peer).unwrap();
        peer.id
    };

    #[allow(clippy::cast_possible_truncation)]
    let mut conn = Slave::build(
        idx,
        reader,
        writer,
        service,
        msg_sender.clone(),
        response_sender,
        Some(Arc::new(tokio::sync::RwLock::new(session))),
    )
    .unwrap();
    if conn.spawn_communication().is_ok() {
        let mut peers = peers.write().unwrap();
        #[allow(clippy::cast_possible_truncation)]
        peers.insert(idx, conn);
        trans.commit().unwrap();
    } else {
        _ = trans.rollback();
    }
    println!("Exchange Complete..");
    msg_sender.send(IPCRes::Connected(idx, true)).unwrap();

    Ok(idx)
}
