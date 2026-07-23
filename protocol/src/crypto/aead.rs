//! Authenticated Encryption with Associated Data (AEAD) utilities.
//!
//! This module provides utilities for authenticated encryption with associated data.

use chacha20poly1305::{
    ChaCha20Poly1305, Nonce,
    aead::{Aead, KeyInit, Payload},
};
use hkdf::Hkdf;
use serde::{Deserialize, Serialize};
use sha2::Sha256;
use thiserror::Error;
use zeroize::ZeroizeOnDrop;

/// Represents the Key Material for an AEAD cipher with 32 bytes.
pub type KeyMaterial = [u8; 32];

// Errors

/// Represents errors that can occur during AEAD operations.
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

/// Derives `N` bytes from `input_key_material` using HKDF-SHA256 with the given `salt` and `info`.
///
/// - `input_key_material`: input key material (must be secret and uniform for cryptographic use).
/// - `salt`: optional public entropy. Pass `None` when `input_key_material` is already uniformly
///   random (e.g. the output of X3DH or a chain key) — RFC 5869 §2.2 specifies that
///   a missing salt is equivalent to a zero-filled block of the hash output length.
///   Pass `Some(s)` when mixing in an external root key as the salt (`KDF_RK` step).
/// - `info`: domain separation label scoping this derivation to a specific purpose.
/// # Errors
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

/// A `MessageKey` is a 32-byte key material used for AEAD encryption and decryption
#[derive(Clone, ZeroizeOnDrop)]
pub struct MessageKey(KeyMaterial);

impl MessageKey {
    #[must_use]
    /// Creates a new `MessageKey` from a 32-byte key material.
    pub fn from_bytes(bytes: KeyMaterial) -> Self {
        Self(bytes)
    }

    #[must_use]
    pub fn as_bytes(&self) -> &KeyMaterial {
        &self.0
    }
}

impl std::fmt::Debug for MessageKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "MessageKey([redacted])")
    }
}

// Encrypted message

// An encrypted message containing the nonce and ciphertext.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EncryptedMessage {
    pub nonce: u64,
    pub ciphertext: Vec<u8>,
}

// Encrypt / Decrypt

/// Encrypts `plaintext` with `key` and `nonce_counter`, with no associated data.
/// # Errors
pub fn encrypt(
    key: &MessageKey,
    nonce_counter: u64,
    plaintext: &[u8],
) -> Result<EncryptedMessage, AeadError> {
    encrypt_with_aad(key, nonce_counter, plaintext, b"")
}

/// Encrypts `plaintext` with `key` and `nonce_counter`, with associated data `aad`.
/// # Errors
pub fn encrypt_with_aad(
    key: &MessageKey,
    nonce_counter: u64,
    plaintext: &[u8],
    aad: &[u8],
) -> Result<EncryptedMessage, AeadError> {
    let cipher =
        ChaCha20Poly1305::new_from_slice(key.as_bytes()).map_err(|_| AeadError::Encrypt)?;
    let nonce = nonce_from_u64(nonce_counter);
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
        nonce: nonce_counter,
        ciphertext,
    })
}

/// Decrypts `msg` with `key`, with no associated data.
/// # Errors
pub fn decrypt(key: &MessageKey, msg: &EncryptedMessage) -> Result<Vec<u8>, AeadError> {
    decrypt_with_aad(key, msg, b"")
}

/// Decrypts `msg` with `key` and verifies `aad`.
/// # Errors
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

// Helpers

/// Maps a u64 counter to a 96-bit ChaCha20-Poly1305 nonce.
fn nonce_from_u64(nonce_counter: u64) -> Nonce {
    let mut n = [0u8; 12];
    n[4..].copy_from_slice(&nonce_counter.to_be_bytes()); // using big endian bytes
    Nonce::from(n)
}
