//! Double Ratchet session encryption.
//!
//! Implements the [Signal Double Ratchet algorithm][dr] (spec revision 4, 2025),
//! providing forward secrecy, break-in recovery, and header encryption for a
//! messaging session.

use crate::constants::{
    INFO_CHAIN, INFO_INIT_HKR, INFO_INIT_HKS, INFO_INIT_RK, INFO_MESSAGE, INFO_ROOT, MAX_SKIP,
    MAX_SKIPPED_KEYS,
};

use super::aead::{
    EncryptedMessage, KeyMaterial, MessageKey, decrypt_with_aad, encrypt_with_aad, hkdf_derive,
};
use ed25519_dalek::ed25519::signature::rand_core::OsRng;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use thiserror::Error;
use x25519_dalek::{PublicKey, StaticSecret};
use zeroize::ZeroizeOnDrop;

// Errors

#[derive(Debug, Error)]
pub enum RatchetError {
    #[error("error deriving key via HKDF")]
    HkdfExpand,
    #[error("AEAD error: {0}")]
    Aead(#[from] super::aead::AeadError),
    /// Send chain is not initialised. For the sender (remote), this indicates a
    /// bug — `init_sender` always populates the send chain. For the receiver
    /// (local), this means `encrypt` was called before the first DH ratchet step.
    #[error("send chain not initialized — encrypt called before first DH ratchet step")]
    SendChainNotInitialized,
    /// Receive chain is not initialised. This means `decrypt` was called on local's
    /// session before he received remote's first message (which triggers the first
    /// DH ratchet step that creates the recv chain).
    #[error("recv chain not initialized — no message received yet")]
    RecvChainNotInitialized,
    #[error("invalid DH public key")]
    InvalidDhKey,
    #[error("too many skipped messages ({0} > MAX_SKIP)")]
    TooManySkipped(u64),
    #[error("skipped message key store is full")]
    SkippedKeysFull,
    /// No known header key could decrypt the incoming header. Either the message
    /// is from an unknown session, or it has been corrupted in transmit.
    #[error("header decryption failed — unknown session or corrupted header")]
    HeaderDecryptionFailed,
}

// RatchetMessage

/// An encrypted Double Ratchet message: encrypted header + AEAD payload.
///
/// Both the header and the payload are opaque to a network observer. The header
/// is encrypted with ChaCha20-Poly1305 (spec §4), hiding the sender's ratchet
/// public key, message counter, and previous chain length. The payload is bound
/// to the header ciphertext as AEAD associated data, so any tampering — including
/// substituting a different header — is detected on decryption.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RatchetMessage {
    pub encrypted_header: EncryptedMessage,
    pub payload: EncryptedMessage,
}

// Domain info labels (RFC 5869 `info` parameter)
//
// Each label scopes one HKDF derivation to a specific purpose. They carry no
// entropy — they are protocol constants. The "atrio-v1-" prefix prevents
// collisions with any other protocol that might share the same key material.
//
// INFO_ROOT      : KDF_RK step — produces new RK + CK + NHK (96 bytes).
// INFO_CHAIN     : KDF_CK step — next chain key.
// INFO_MESSAGE   : KDF_CK step — message key.
// INFO_INIT_RK   : initial root key from shared secret.
// INFO_INIT_HKS  : initial send header key (HKs) from shared secret.
// INFO_INIT_HKR  : initial recv header key (HKr) from shared secret.
//
// HKs and HKr are derived with separate labels so remote and local produce
// different keys from the same shared secret for each direction.

// HeaderKey

/// A key used to encrypt or decrypt a ratchet message header.
///
/// Header keys rotate on every DH ratchet step. The current key encrypts/decrypts
/// headers for the active chain; the *next* key is pre-derived and stored so the
/// receiver can identify which DH epoch an incoming message belongs to without
/// first decrypting the payload.
#[derive(Clone, ZeroizeOnDrop)]
struct HeaderKey(KeyMaterial);

impl HeaderKey {
    fn from_bytes(bytes: KeyMaterial) -> Self {
        Self(bytes)
    }

    fn as_message_key(&self) -> MessageKey {
        MessageKey::from_bytes(self.0)
    }
}

// ChainKey — KDF_CK

#[derive(Clone, ZeroizeOnDrop)]
struct ChainKey(KeyMaterial);

impl ChainKey {
    fn from_bytes(bytes: KeyMaterial) -> Self {
        Self(bytes)
    }

    /// Advances the symmetric-key ratchet one step, producing a message key and
    /// replacing the current chain key with the derived successor.
    ///
    /// Two separate HKDF invocations with different `info` values give us two
    /// independent 32-byte outputs from the same IKM without any length extension
    /// concerns. The old chain key bytes are overwritten before the new value is
    /// stored; `ZeroizeOnDrop` cleans the struct on final drop.
    fn advance(&mut self) -> Result<MessageKey, RatchetError> {
        let msg_bytes = hkdf_derive::<32>(&self.0, None, INFO_MESSAGE)?;
        let next_bytes = hkdf_derive::<32>(&self.0, None, INFO_CHAIN)?;
        self.0 = next_bytes;
        Ok(MessageKey::from_bytes(msg_bytes))
    }
}

#[derive(ZeroizeOnDrop)]
struct RootKey(KeyMaterial);

impl RootKey {
    fn from_bytes(bytes: KeyMaterial) -> Self {
        Self(bytes)
    }
    /// Advances the root chain using fresh DH output, returning the new chain key
    /// and the next header key for that direction.
    ///
    /// Output layout (96 bytes):
    ///   [0..32]  → new root key  (replaces self.0 in place)
    ///   [32..64] → new chain key (send or recv depending on direction)
    ///   [64..96] → next header key (send or recv depending on direction)
    fn advance(&mut self, dh_output: &KeyMaterial) -> Result<(ChainKey, HeaderKey), RatchetError> {
        let material = hkdf_derive::<96>(dh_output, Some(&self.0), INFO_ROOT)?;
        self.0 = material[..32].try_into().unwrap();
        let chain_key = ChainKey::from_bytes(material[32..64].try_into().unwrap());
        let next_header_key = HeaderKey::from_bytes(material[64..96].try_into().unwrap());
        Ok((chain_key, next_header_key))
    }
}

/// The plaintext contents of a ratchet message header.
///
/// This struct is never transmitted directly — it is always serialised and
/// encrypted before being placed in a [`RatchetMessage`].
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub struct RatchetHeader {
    pub dh_pub: KeyMaterial,
    pub prev_chain_length: u64,
    pub message_number: u64,
}

impl RatchetHeader {
    pub(crate) fn new(dh_pub: KeyMaterial, prev_chain_length: u64, message_number: u64) -> Self {
        Self {
            dh_pub,
            prev_chain_length,
            message_number,
        }
    }

    /// Serialises the header to a fixed 48-byte sequence.
    ///
    /// Layout: [`dh_pub` (32)] | [`prev_chain_length` (8, Big Endian)] | [`message_number` (8, Big Endian)]
    /// Fixed-width fields make the encoding unambiguous without a length prefix.
    pub(crate) fn to_bytes(&self) -> [u8; 48] {
        let mut buf = [0u8; 48];
        buf[..32].copy_from_slice(&self.dh_pub);
        buf[32..40].copy_from_slice(&self.prev_chain_length.to_be_bytes());
        buf[40..48].copy_from_slice(&self.message_number.to_be_bytes());
        buf
    }

    /// Deserialises a header from exactly 48 bytes.
    fn from_bytes(bytes: &[u8]) -> Option<Self> {
        if bytes.len() != 48 {
            return None;
        }
        let dh_pub: KeyMaterial = bytes[..32].try_into().ok()?;
        let prev_chain_length = u64::from_be_bytes(bytes[32..40].try_into().ok()?);
        let message_number = u64::from_be_bytes(bytes[40..48].try_into().ok()?);
        Some(Self {
            dh_pub,
            prev_chain_length,
            message_number,
        })
    }
}

// Skipped message keys — indexed by (header_key_bytes, message_number), not
// (dh_pub, message_number), since dh_pub is encrypted in the header.
// Manual Drop impl: draining drops each MessageKey (which carries ZeroizeOnDrop).
struct SkippedKeys(HashMap<(KeyMaterial, u64), MessageKey>);

impl Drop for SkippedKeys {
    fn drop(&mut self) {
        self.0.drain();
    }
}

impl SkippedKeys {
    fn new() -> Self {
        Self(HashMap::new())
    }

    fn get_and_remove(
        &mut self,
        header_key: &KeyMaterial,
        message_number: u64,
    ) -> Option<MessageKey> {
        self.0.remove(&(*header_key, message_number))
    }

    fn insert(
        &mut self,
        header_key: KeyMaterial,
        message_number: u64,
        message_key: MessageKey,
    ) -> Result<(), RatchetError> {
        if self.0.len() >= MAX_SKIPPED_KEYS {
            return Err(RatchetError::SkippedKeysFull);
        }
        self.0.insert((header_key, message_number), message_key);
        Ok(())
    }

    fn len(&self) -> usize {
        self.0.len()
    }
}

/// Full Double Ratchet session state.
///
/// # Initialization
///
/// Both parties first establish a shared secret via a key agreement protocol
/// (e.g. X3DH or Noise IK). The resulting 32-byte output is passed directly
/// to `init_sender` / `init_receiver`.
///
/// - remote (initiator) calls [`RatchetSession::init_sender`] with the shared
///   secret and local's ratchet public key.
/// - local   (responder) calls [`RatchetSession::init_receiver`] with the shared
///   secret and his own ratchet [`StaticSecret`].
///
/// # Security
///
/// All key material is zeroized on drop. Public keys and counters are not secret
/// and are skipped from zeroization. See field comments for details.
#[derive(ZeroizeOnDrop)]
pub struct RatchetSession {
    root_key: RootKey,
    send_chain: Option<ChainKey>,
    recv_chain: Option<ChainKey>,

    header_key_send: Option<HeaderKey>,
    header_key_recv: Option<HeaderKey>,
    next_header_key_send: Option<HeaderKey>,
    next_header_key_recv: Option<HeaderKey>,

    local_private_key: StaticSecret,
    #[zeroize(skip)]
    local_public_key: PublicKey,
    #[zeroize(skip)]
    remote_public_key: Option<PublicKey>,

    #[zeroize(skip)]
    send_count: u64,
    #[zeroize(skip)]
    recv_count: u64,
    #[zeroize(skip)]
    prev_chain_length: u64,

    #[zeroize(skip)]
    skipped: SkippedKeys,
}

impl RatchetSession {
    /// Initialize Ratchet Session for Sending
    /// # Errors
    pub fn init_sender(
        shared_secret: &KeyMaterial,
        remote_dh_pub_bytes: &KeyMaterial,
    ) -> Result<Self, RatchetError> {
        let remote_public_key = PublicKey::from(*remote_dh_pub_bytes);

        // Derive initial keys from the shared secret (output of X3DH handshake).
        let root_key_bytes = hkdf_derive::<32>(shared_secret, None, INFO_INIT_RK)?;
        let header_key_send_bytes = hkdf_derive::<32>(shared_secret, None, INFO_INIT_HKS)?;
        let header_key_recv_bytes = hkdf_derive::<32>(shared_secret, None, INFO_INIT_HKR)?;

        let mut root_key = RootKey::from_bytes(root_key_bytes);

        // Generate ephemeral DH keypair for the first ratchet step.
        let local_private_key = StaticSecret::random_from_rng(OsRng);
        let local_public_key = PublicKey::from(&local_private_key);

        // Perform first DH to derive the initial sending chain.
        let dh_output = local_private_key.diffie_hellman(&remote_public_key);
        let (send_chain, next_header_key_send) = root_key.advance(dh_output.as_bytes())?;

        Ok(Self {
            root_key,
            send_chain: Some(send_chain),
            recv_chain: None,
            header_key_send: Some(HeaderKey::from_bytes(header_key_send_bytes)),
            header_key_recv: None,
            next_header_key_send: Some(next_header_key_send),
            next_header_key_recv: Some(HeaderKey::from_bytes(header_key_recv_bytes)),
            local_private_key,
            local_public_key,
            remote_public_key: Some(remote_public_key),
            send_count: 0,
            recv_count: 0,
            prev_chain_length: 0,
            skipped: SkippedKeys::new(),
        })
    }

    /// Initialize Ratchet Session for Receiver
    /// # Errors
    pub fn init_receiver(
        shared_secret: &KeyMaterial,
        local_private_key: StaticSecret,
    ) -> Result<Self, RatchetError> {
        // Derive initial keys from the shared secret (output of X3DH handshake).
        // Note: HKs and HKr labels are swapped compared to sender — what is "send"
        // for remote is "recv" for local and vice versa.
        let root_key_bytes = hkdf_derive::<32>(shared_secret, None, INFO_INIT_RK)?;
        let header_key_send_bytes = hkdf_derive::<32>(shared_secret, None, INFO_INIT_HKS)?;
        let header_key_recv_bytes = hkdf_derive::<32>(shared_secret, None, INFO_INIT_HKR)?;

        let root_key = RootKey::from_bytes(root_key_bytes);

        let local_public_key = PublicKey::from(&local_private_key);

        Ok(Self {
            root_key,
            send_chain: None,
            recv_chain: None,
            header_key_send: Some(HeaderKey::from_bytes(header_key_send_bytes)),
            header_key_recv: None,
            next_header_key_send: None,
            next_header_key_recv: Some(HeaderKey::from_bytes(header_key_recv_bytes)),
            local_private_key,
            local_public_key,
            remote_public_key: None,
            send_count: 0,
            recv_count: 0,
            prev_chain_length: 0,
            skipped: SkippedKeys::new(),
        })
    }

    #[must_use]
    /// Returns local Ephemaral public key
    pub fn local_public_key(&self) -> KeyMaterial {
        self.local_public_key.to_bytes()
    }

    /// Encrypts a plaintext message, producing a [`RatchetMessage`] containing
    /// an encrypted header and an AEAD-encrypted payload.
    ///
    /// Steps:
    /// 1. Advance the sending chain to derive a unique message key.
    /// 2. Build the ratchet header (local DH public key, previous chain length, message number).
    /// 3. Encrypt the header with the current send header key (hides metadata from observers).
    /// 4. Encrypt the payload with the message key, binding it to the associated data and header.
    /// # Errors
    pub fn encrypt(
        &mut self,
        plaintext: &[u8],
        associated_data: &[u8],
    ) -> Result<RatchetMessage, RatchetError> {
        // Advance the symmetric-key ratchet to get a fresh message key.
        let send_chain = self
            .send_chain
            .as_mut()
            .ok_or(RatchetError::SendChainNotInitialized)?;
        let message_key = send_chain.advance()?;

        // Build the plaintext header with current ratchet state.
        let header = RatchetHeader::new(
            self.local_public_key.to_bytes(),
            self.prev_chain_length,
            self.send_count,
        );

        // Encrypt the header so an observer cannot see the DH public key or counters.
        let header_key = self
            .header_key_send
            .as_ref()
            .ok_or(RatchetError::SendChainNotInitialized)?;
        let encrypted_header = encrypt_with_aad(
            &header_key.as_message_key(),
            self.send_count,
            &header.to_bytes(),
            &[],
        )?;

        // Encrypt the payload, binding it to the associated data and encrypted header.
        let aad = Self::build_aad(associated_data, &encrypted_header);
        let payload = encrypt_with_aad(&message_key, self.send_count, plaintext, &aad)?;
        self.send_count += 1;

        Ok(RatchetMessage {
            encrypted_header,
            payload,
        })
    }

    /// Decrypts an incoming [`RatchetMessage`], advancing the ratchet as needed.
    ///
    /// Steps:
    /// 1. Trial-decrypt the header with current and next header keys.
    /// 2. Check if this message key was previously skipped (out-of-order delivery).
    /// 3. If the header contains a new DH public key, skip pending message keys and
    ///    perform a DH ratchet step to derive new chains.
    /// 4. Skip any remaining message keys up to the target message number.
    /// 5. Advance the receiving chain to derive the message key and decrypt.
    /// # Errors
    pub fn decrypt(
        &mut self,
        msg: &RatchetMessage,
        associated_data: &[u8],
    ) -> Result<Vec<u8>, RatchetError> {
        // Try to decrypt the header with both current and next header keys.
        let (header, header_key_used) = self.try_decrypt_header(&msg.encrypted_header)?;
        let aad = Self::build_aad(associated_data, &msg.encrypted_header);

        // If this message key was skipped earlier, use it directly.
        if let Some(message_key) = self
            .skipped
            .get_and_remove(&header_key_used, header.message_number)
        {
            return decrypt_with_aad(&message_key, &msg.payload, &aad).map_err(RatchetError::Aead);
        }

        // Detect if the sender performed a DH ratchet step (new DH public key).
        let is_new_dh_key = self
            .remote_public_key
            .is_none_or(|p| p.to_bytes() != header.dh_pub);

        if is_new_dh_key {
            // Skip any remaining messages from the old sending chain before ratcheting.
            self.skip_message_keys(header.prev_chain_length)?;
            self.dh_ratchet_step(&header.dh_pub)?;
        }

        // Skip message keys up to the target message number.
        self.skip_message_keys(header.message_number)?;

        // Advance the receiving chain and decrypt the payload.
        let recv_chain = self
            .recv_chain
            .as_mut()
            .ok_or(RatchetError::RecvChainNotInitialized)?;
        let message_key = recv_chain.advance()?;
        self.recv_count += 1;

        decrypt_with_aad(&message_key, &msg.payload, &aad).map_err(RatchetError::Aead)
    }

    /// Attempts to decrypt the encrypted header using both the current and next
    /// header keys. Returns the parsed header and the key that succeeded.
    ///
    /// This trial decryption allows the receiver to identify which DH epoch
    /// (current or next) the incoming message belongs to, without first
    /// knowing the sender's ratchet state.
    fn try_decrypt_header(
        &self,
        encrypted_header: &EncryptedMessage,
    ) -> Result<(RatchetHeader, KeyMaterial), RatchetError> {
        let candidates: [Option<&HeaderKey>; 2] = [
            self.header_key_recv.as_ref(),
            self.next_header_key_recv.as_ref(),
        ];

        for header_key in candidates.iter().flatten() {
            if let Ok(bytes) = decrypt_with_aad(&header_key.as_message_key(), encrypted_header, &[])
                && let Some(header) = RatchetHeader::from_bytes(&bytes)
            {
                return Ok((header, header_key.0));
            }
        }
        Err(RatchetError::HeaderDecryptionFailed)
    }

    fn dh_ratchet_step(&mut self, remote_pub_bytes: &KeyMaterial) -> Result<(), RatchetError> {
        let remote_public_key = PublicKey::from(*remote_pub_bytes);

        // Save the current send chain length before switching directions.
        self.prev_chain_length = self.send_count;
        self.send_count = 0;
        self.recv_count = 0;
        self.remote_public_key = Some(remote_public_key);

        // Receive direction: compute DH with remote's key, advance root key.
        let shared_secret_key = self.local_private_key.diffie_hellman(&remote_public_key);
        let (recv_chain_key, new_next_header_key_recv) =
            self.root_key.advance(shared_secret_key.as_bytes())?;
        self.header_key_recv = self.next_header_key_recv.take();
        self.next_header_key_recv = Some(new_next_header_key_recv);
        self.recv_chain = Some(recv_chain_key);

        // Send direction: generate new ephemeral DH keypair, advance root key.
        let new_secret = StaticSecret::random_from_rng(OsRng);
        let new_public = PublicKey::from(&new_secret);
        if self.next_header_key_send.is_some() {
            self.header_key_send = self.next_header_key_send.take();
        }

        let new_shared_secret = new_secret.diffie_hellman(&remote_public_key);
        let (send_chain_key, new_next_header_key_send) =
            self.root_key.advance(new_shared_secret.as_bytes())?;
        self.next_header_key_send = Some(new_next_header_key_send);
        self.send_chain = Some(send_chain_key);

        self.local_private_key = new_secret;
        self.local_public_key = new_public;

        Ok(())
    }

    /// Advances the receiving chain, storing each derived message key for later
    /// out-of-order delivery. Called when we receive a message with a higher
    /// message number than expected (e.g. messages arrived out of order over Tor).
    ///
    /// Each stored key is indexed by (`header_key`, `message_number`) so it can be
    /// retrieved when the delayed message eventually arrives.
    fn skip_message_keys(&mut self, until: u64) -> Result<(), RatchetError> {
        if until > self.recv_count + MAX_SKIP {
            return Err(RatchetError::TooManySkipped(until - self.recv_count));
        }

        if let Some(recv_chain) = self.recv_chain.as_mut() {
            let header_key = self.header_key_recv.as_ref().map_or([0u8; 32], |hk| hk.0);

            while self.recv_count < until {
                let message_key = recv_chain.advance()?;
                self.skipped
                    .insert(header_key, self.recv_count, message_key)?;
                self.recv_count += 1;
            }
        }

        Ok(())
    }

    /// Builds the AEAD associated data by concatenating the user-provided associated
    /// data with the encrypted header. This binds the payload to the specific header,
    /// preventing an attacker from substituting a different header.
    ///
    /// Layout: [`ad_len` (4, Big Endian)] | [ad] | [nonce (8, Big Endian)] | [ciphertext]
    fn build_aad(associated_data: &[u8], encrypted_header: &EncryptedMessage) -> Vec<u8> {
        let mut buffer =
            Vec::with_capacity(4 + associated_data.len() + 8 + encrypted_header.ciphertext.len());
        #[allow(clippy::cast_possible_truncation)]
        buffer.extend_from_slice(&(associated_data.len() as u32).to_be_bytes());
        buffer.extend_from_slice(associated_data);
        buffer.extend_from_slice(&encrypted_header.nonce.to_be_bytes());
        buffer.extend_from_slice(&encrypted_header.ciphertext);
        buffer
    }

    #[must_use]
    /// Number of skipped message keys currently in the store.
    pub fn skipped_keys_count(&self) -> usize {
        self.skipped.len()
    }
}

// Tests
