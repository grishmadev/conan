use crate::{
    constants::{ARTI_PRIVATE_KEY, ENCRYPTION_INFO},
    msg::Msg,
};
use base64::Engine;
use chacha20poly1305::{
    ChaCha20Poly1305, KeyInit, Nonce,
    aead::{Aead, Generate},
};
use ed25519_dalek::{Signature, Verifier, VerifyingKey};
use hkdf::Hkdf;
use sha2::Sha256;
use ssh_encoding::Decode;
use std::{error::Error, fs::File, io::Read, str::FromStr};
use tokio::io::{AsyncWriteExt, WriteHalf};
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

// Commented for now. maybe removed or used later

// pub fn generate_keypair() -> Result<(), Box<dyn Error>> {
//     let signing_path = Path::new(&home_dir).join(Path::new(ARTI_KEYSTORE));
//     if !signing_path.exists() {
//         let signing_key = SigningKey::generate(&mut OsRng);
//         let mut signing_file = OpenOptions::new()
//             .create(true)
//             .truncate(true)
//             .write(true)
//             .open(signing_path)?;
//         signing_file.write_all(signing_key.as_bytes())?;
//     }
//     Ok(())
// }

/// Used to retrieve signing key for self tor server
///
/// # Errors
/// # Panics
pub fn signing_key(arti_key_store: String) -> Result<ExpandedKeypair, Box<dyn Error>> {
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
    payload_slice.read_exact(&mut magic)?;
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
