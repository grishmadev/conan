use crate::{
    constants::{ARTI_PRIVATE_KEY, ENCRYPTION_INFO},
    msg::Msg,
};
use arti_client::DataStream;
use base64::Engine;
use chacha20poly1305::{
    ChaCha20Poly1305, KeyInit, Nonce,
    aead::{Aead, Generate},
};
use ed25519_dalek::{Signature, Verifier, VerifyingKey, ed25519::signature::rand_core::OsRng};
use futures::AsyncReadExt as FutureRead;
use hkdf::Hkdf;
use safelog::DisplayRedacted;
use sha2::Sha256;
use ssh_encoding::Decode;
use std::{
    error::Error,
    fs::File,
    io::Read,
    str::FromStr,
    sync::{Arc, RwLock},
};
use tokio::io::{AsyncReadExt, AsyncWriteExt, ReadHalf, WriteHalf};
use tor_hsservice::HsId;
use tor_llcrypto::pk::ed25519::ExpandedKeypair;
use x25519_dalek::{EphemeralSecret, PublicKey};

/// Encrypts a `[Msg::msg]` turned to bytes to a vec of bytes
/// we assume `data` is just direct serialized version of the message without any kind of wrapper etc.
/// # Errors
#[inline]
pub fn encrypt(
    shared_secret_key: &[u8; 32],
    data: &[u8],
) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    let hk = Hkdf::<Sha256>::new(None, shared_secret_key);
    let mut encryption_key = [0u8; 32];
    hk.expand(ENCRYPTION_INFO.as_bytes(), &mut encryption_key)?;

    let cipher = ChaCha20Poly1305::new_from_slice(&encryption_key)?;
    let nonce = Nonce::generate_from_rng(&mut rand::rng());
    let cipher_text = cipher.encrypt(&nonce, data)?;
    let mut new_cipher_text = nonce.to_vec();
    new_cipher_text.extend(cipher_text);
    Ok(new_cipher_text)
}

/// Decrypts a &[u8] back to message
///
/// # Errors
#[inline]
pub fn decrypt(
    shared_secret_key: &[u8; 32],
    data: &[u8],
) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    let nonce_bytes: [u8; 12] = match data[..12].try_into() {
        Ok(s) => s,
        Err(e) => return Err(format!("Cannot convert slice to nonce. {e}").into()),
    };
    let cipher_bytes = data[12..].to_vec();
    let nonce = Nonce::cast_from_core(&nonce_bytes);
    let hk = Hkdf::<Sha256>::new(None, shared_secret_key);
    let mut encryption_key = [0u8; 32];
    hk.expand(ENCRYPTION_INFO.as_bytes(), &mut encryption_key)?;
    let cipher = ChaCha20Poly1305::new_from_slice(&encryption_key)?;
    let decrypted_bytes = cipher.decrypt(nonce, cipher_bytes.as_ref())?;

    Ok(decrypted_bytes)
}

/// Used to retrieve signing key for self tor server
///
/// # Errors
/// # Panics
pub async fn signing_key(arti_key_store: String) -> Result<ExpandedKeypair, Box<dyn Error>> {
    let mut key_store_path = arti_key_store;
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

    // 3. Extract the isolated inner private block
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
    peer_addr: &(String, u16),
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
    let hsid = HsId::from_str(&peer_addr.0)?;
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
/// # Errors
pub fn edhverify(
    local_private_key: EphemeralSecret,
    remote_public_key: PublicKey,
    ssk: &mut Option<[u8; 32]>,
) {
    let shared_secret_key = local_private_key.diffie_hellman(&remote_public_key);
    *ssk = Some(*shared_secret_key.as_bytes());
}

pub async fn listener_actor(
    arti_key_store: String,
    reader: &mut ReadHalf<DataStream>,
    writer: &mut WriteHalf<DataStream>,
    ssk: &mut Option<[u8; 32]>,
    assign_remote_hsid: &mut Option<String>,
    local_hsid: HsId,
) -> Result<(), Box<dyn Error>> {
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
    let signing_key = signing_key(arti_key_store).await?;

    // creating signature using local ed25519 private key and stacking local and remote
    // ephemeral keys
    let mut data = vec![];
    data.extend_from_slice(&remote_public_key);
    data.extend_from_slice(&local_public_key);
    let data: [u8; 64] = data.try_into().unwrap();
    let signature = signing_key.sign(&data);

    // remote entities should always be put before local entities
    // because kindness & respect is one of many factors that makes us human.
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
    *ssk = Some(*shared_secret_key.as_bytes());

    println!("Parsing peer's public key.");
    let size = reader.read_u16().await? as usize;
    let mut buf = vec![0u8; size];
    let size = reader.read_exact(&mut buf).await?;

    // decrypting and reading here.
    let decrypted = decrypt(shared_secret_key.as_bytes(), &buf[..size])?;
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
    // and tell it to wrap things up from its side
    println!("Verified. Sending Approval.");
    let verified = encrypt(
        shared_secret_key.as_bytes(),
        Msg::Verified.to_vec().as_slice(),
    )?;
    #[allow(clippy::cast_possible_truncation)]
    writer.write_u16(verified.len() as u16).await?;
    writer.write_all(&verified).await?;
    writer.flush().await?;

    // reader.read_to_end(&mut vec![]).await?;
    println!("Verification Complete.");
    Ok(())
}

/// Use this toAct as a Dialer
/// # Errors
/// # Panics
pub async fn dialer_actor<R, W>(
    arti_key_store: String,
    reader: &mut ReadHalf<R>,
    writer: &mut WriteHalf<W>,
    ssk: &mut Option<[u8; 32]>,
    local_hsid: HsId,
    peer_addr: &(String, u16),
) -> Result<(), Box<dyn Error>>
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
    // and assigning in to `remote_public_key`
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
    // after the we confirm we have indeed connected to the right listener,
    // we combine the x25519 remote public key with our private key to form
    // a diffie hellman shared secret key.
    println!("Confirmed. Assigning Shared Secret Key using Diffie Hellman key exchange.");
    edhverify(local_private_key, remote_public_key, ssk);
    let Some(shared_secret_key) = *ssk else {
        return Err("Couldn't get Shared Secret Key.".into());
    };
    let signing_key = signing_key(arti_key_store).await?;

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
    let remote_hsid = HsId::from_str(&peer_addr.0)?;
    let remote_hsid_bytes = remote_hsid.as_ref();
    let msg = Msg::SignedAndPublicKey(
        signature.to_bytes().to_vec(),
        *remote_hsid_bytes,
        *local_hsid_bytes,
    );

    // encrypting and sending here.
    let encrypted = encrypt(&shared_secret_key, &msg.to_vec())?;
    #[allow(clippy::cast_possible_truncation)]
    writer.write_u16(encrypted.len() as u16).await?;
    writer.write_all(&encrypted).await?;
    writer.flush().await?;

    // waiting for listener's approval
    println!("Waiting for approval..");
    let size = reader.read_u16().await?;
    let mut buf = vec![0u8; size as usize];
    reader.read_exact(&mut buf).await?;
    let decrypted = decrypt(&shared_secret_key, &buf)?;
    let Msg::Verified = Msg::from_bytes(&decrypted) else {
        return Err("Didn't receive Approval, Aborting.".into());
    };
    Ok(())
}

/// Encrypts message before writing to writer
pub async fn send<T>(
    writer: &mut WriteHalf<T>,
    msg: Vec<u8>,
    ssk: Arc<RwLock<[u8; 32]>>,
) -> Result<(), Box<dyn Error>>
where
    T: AsyncReadExt + AsyncWriteExt,
{
    let encrypted;
    {
        let ssk = ssk.read().unwrap();
        encrypted = encrypt(&ssk, &msg)?;
    }
    #[allow(clippy::cast_possible_truncation)]
    writer.write_u16(encrypted.len() as u16).await?;
    writer.write_all(&encrypted).await?;
    writer.flush().await?;
    Ok(())
}

/// Decrypts message before returning
pub async fn recv<T>(
    reader: &mut ReadHalf<T>,
    ssk: Arc<RwLock<[u8; 32]>>,
) -> Result<Vec<u8>, Box<dyn Error>>
where
    T: AsyncReadExt + AsyncWriteExt,
{
    let size = reader.read_u16().await?;
    let mut buf = vec![0u8; size as usize];
    reader.read_exact(&mut buf).await?;

    let ssk = ssk.read().unwrap();
    let decrypted = match decrypt(&ssk, &buf) {
        Ok(s) => s,
        Err(e) => {
            return Err(format!("Corrupted message.\n{e}").into());
        }
    };
    Ok(decrypted)
}
