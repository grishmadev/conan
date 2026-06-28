use bincode::config;
use chacha20poly1305::{
    ChaCha20Poly1305, KeyInit, Nonce,
    aead::{Aead, Generate},
};
use hkdf::Hkdf;
use sha2::Sha256;

use crate::{constants::ENCRYPTION_INFO, msg::Msg};

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
) -> Result<Msg, Box<dyn std::error::Error>> {
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
    let (msg, _) = bincode::serde::decode_from_slice(&decrypted_bytes, config::legacy())?;

    Ok(msg)
}
