use chacha20poly1305::{
    ChaCha20Poly1305, Nonce,
    aead::{Aead, KeyInit, Payload},
};
use hkdf::Hkdf;
use serde::{Deserialize, Serialize};
use sha2::Sha256;
use thiserror::Error;
use zeroize::ZeroizeOnDrop;

pub type KeyMaterial = [u8; 32];

#[derive(Debug, Error)]
pub enum AeadError {
    #[error("encryption error")]
    Encrypt,
    #[error("decryption error — message corrupted or tampered with")]
    Decrypt,
    #[error("nonce exhausted")]
    NonceExhausted,
    #[error("HKDF expand error")]
    HkdfExpand,
}

pub fn hkdf_derive<const N: usize>(
    ikm: &[u8],
    salt: Option<&[u8]>,
    info: &[u8],
) -> Result<[u8; N], AeadError> {
    let hk = Hkdf::<Sha256>::new(salt, ikm);
    let mut out = [0u8; N];
    hk.expand(info, &mut out)
        .map_err(|_| AeadError::HkdfExpand)?;
    Ok(out)
}

#[derive(Clone, ZeroizeOnDrop)]
pub struct MessageKey(KeyMaterial);

impl MessageKey {
    pub fn from_bytes(bytes: KeyMaterial) -> Self {
        Self(bytes)
    }

    pub fn as_bytes(&self) -> &KeyMaterial {
        &self.0
    }
}

impl std::fmt::Debug for MessageKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "MessageKey([redacted])")
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EncryptedMessage {
    pub nonce: u64,
    pub ciphertext: Vec<u8>,
}

pub fn encrypt(
    key: &MessageKey,
    nonce_val: u64,
    plaintext: &[u8],
) -> Result<EncryptedMessage, AeadError> {
    encrypt_with_aad(key, nonce_val, plaintext, b"")
}

pub fn encrypt_with_aad(
    key: &MessageKey,
    nonce_val: u64,
    plaintext: &[u8],
    aad: &[u8],
) -> Result<EncryptedMessage, AeadError> {
    let cipher =
        ChaCha20Poly1305::new_from_slice(key.as_bytes()).map_err(|_| AeadError::Encrypt)?;
    let nonce = nonce_from_u64(nonce_val);
    let ciphertext = cipher
        .encrypt(
            &nonce,
            Payload {
                msg: plaintext,
                aad,
            },
        )
        .map_err(|_| AeadError::Encrypt)?;
    Ok(EncryptedMessage {
        nonce: nonce_val,
        ciphertext,
    })
}

pub fn decrypt(key: &MessageKey, msg: &EncryptedMessage) -> Result<Vec<u8>, AeadError> {
    decrypt_with_aad(key, msg, b"")
}

pub fn decrypt_with_aad(
    key: &MessageKey,
    msg: &EncryptedMessage,
    aad: &[u8],
) -> Result<Vec<u8>, AeadError> {
    let cipher =
        ChaCha20Poly1305::new_from_slice(key.as_bytes()).map_err(|_| AeadError::Encrypt)?;
    let nonce = nonce_from_u64(msg.nonce);
    cipher
        .decrypt(
            &nonce,
            Payload {
                msg: &msg.ciphertext,
                aad,
            },
        )
        .map_err(|_| AeadError::Decrypt)
}

fn nonce_from_u64(nonce_val: u64) -> Nonce {
    let mut n = [0u8; 12];
    n[4..].copy_from_slice(&nonce_val.to_be_bytes()); // using big endian bytes
    Nonce::from(n)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_key() -> MessageKey {
        MessageKey::from_bytes([0x42u8; 32])
    }

    #[test]
    fn basic_encrypt_and_decrypt() {
        let key = test_key();
        let msg = encrypt(&key, 1, b"hello from conan").unwrap();
        assert_ne!(msg.ciphertext, b"hello from conan");
        let plain = decrypt(&key, &msg).unwrap();
        assert_eq!(plain, b"hello from conan");
    }

    #[test]
    fn incorrect_key_fails() {
        let key1 = MessageKey::from_bytes([0x01u8; 32]);
        let key2 = MessageKey::from_bytes([0x02u8; 32]);
        let msg = encrypt(&key2, 1, b"secret").unwrap();
        assert!(decrypt(&key1, &msg).is_err());
    }

    #[test]
    fn manipulated_ciphertext_fails() {
        let key = test_key();
        let mut msg = encrypt(&key, 1, b"real message").unwrap();
        msg.ciphertext[0] ^= 0xFF; // modify a byte in the ciphertext, should fail decryption
        assert!(decrypt(&key, &msg).is_err());
    }

    #[test]
    fn hkdf_is_deterministic() {
        let k1 = hkdf_derive::<32>(b"ikm", Some(b"salt"), b"conan-v1-test").unwrap();
        let k2 = hkdf_derive::<32>(b"ikm", Some(b"salt"), b"conan-v1-test").unwrap();
        assert_eq!(k1, k2); // for the same exact input, output should be the same
    }

    #[test]
    fn hkdf_distinct_info_produces_distinct_keys() {
        let k1 = hkdf_derive::<32>(b"ikm", Some(b"salt"), b"conan-v1-purpose-a").unwrap();
        let k2 = hkdf_derive::<32>(b"ikm", Some(b"salt"), b"conan-v1-purpose-b").unwrap();
        assert_ne!(k1, k2); // for distinct info, output should be different
    }

    #[test]
    fn hkdf_empty_salt_is_valid() {
        let k = hkdf_derive::<32>(b"uniform-ikm", None, b"conan-v1-test").unwrap();
        assert_eq!(k.len(), 32);
    }

    #[test]
    fn aad_roundtrip() {
        let key = test_key();
        let aad = b"conan-v1-header-aad";
        let msg = encrypt_with_aad(&key, 1, b"secret message", aad).unwrap();
        let plain = decrypt_with_aad(&key, &msg, aad).unwrap();
        assert_eq!(plain, b"secret message");
    }

    #[test]
    fn tampered_aad_fails() {
        let key = test_key();
        let msg = encrypt_with_aad(&key, 1, b"message", b"real-header").unwrap();
        assert!(decrypt_with_aad(&key, &msg, b"modified-header").is_err());
    }
}
