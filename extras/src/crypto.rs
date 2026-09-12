use chacha20poly1305::{
    AeadInOut, ChaCha20Poly1305, KeyInit, Nonce,
    aead::{Buffer, Generate, Key, arrayvec::ArrayVec},
};
use std::error::Error;

/// Used to encrypt a message with given key
/// # Errors
pub fn direct_encrypt(key: &Key<ChaCha20Poly1305>, data: &[u8]) -> Result<Vec<u8>, Box<dyn Error>> {
    let cipher = ChaCha20Poly1305::new(key);
    let nonce = Nonce::generate();
    let mut arrayvec = ArrayVec::<u8, 128>::new();
    arrayvec.extend_from_slice(data)?;
    cipher.encrypt_in_place(&nonce, b"", &mut arrayvec)?;
    let mut buffer = Vec::with_capacity(nonce.len() + arrayvec.len());
    buffer.extend_from_slice(&nonce);
    buffer.extend_from_slice(&arrayvec);
    Ok(buffer)
}

/// Used to decrypt a message with given key
/// # Errors
pub fn direct_decrypt(key: &Key<ChaCha20Poly1305>, data: &[u8]) -> Result<Vec<u8>, Box<dyn Error>> {
    let cipher = ChaCha20Poly1305::new(key);
    let (nonce_bytes, cipher_text) = data.split_at(12);
    let nonce = nonce_bytes.try_into()?;
    let mut buffer = vec![];
    buffer.extend_from_slice(cipher_text);
    cipher.decrypt_in_place(&nonce, b"", &mut buffer)?;
    Ok(buffer)
}
