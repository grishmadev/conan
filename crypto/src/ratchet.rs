use crate::aead::{
    EncryptedMessage, KeyMaterial, MessageKey, decrypt_with_aad, encrypt_with_aad, hkdf_derive,
};
use rand_core::OsRng;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use thiserror::Error;
use x25519_dalek::{PublicKey as X25519PublicKey, StaticSecret};
use zeroize::ZeroizeOnDrop;

#[derive(Debug, Error)]
pub enum RatchetError {
    #[error("error deriving key via HKDF")]
    HkdfExpand,
    #[error("AEAD error: {0}")]
    Aead(#[from] crate::aead::AeadError),
    #[error("send chain not initialized — encrypt called before first DH ratchet step")]
    SendChainNotInitialized,
    #[error("recv chain not initialized — no message received yet")]
    RecvChainNotInitialized,
    #[error("invalid DH public key")]
    InvalidDhKey,
    #[error("too many skipped messages ({0} > MAX_SKIP)")]
    TooManySkipped(u64),
    #[error("skipped message key store is full")]
    SkippedKeysFull,
    #[error("header decryption failed — unknown session or corrupted header")]
    HeaderDecryptionFailed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RatchetMessage {
    pub encrypted_header: EncryptedMessage,
    pub payload: EncryptedMessage,
}

const INFO_ROOT: &[u8] = b"conan-v1-root";
const INFO_CHAIN: &[u8] = b"conan-v1-chain";
const INFO_MESSAGE: &[u8] = b"conan-v1-message";
const INFO_INIT_RK: &[u8] = b"conan-v1-init-rk";
const INFO_INIT_HKS: &[u8] = b"conan-v1-init-hks";
const INFO_INIT_HKR: &[u8] = b"conan-v1-init-hkr";

const MAX_SKIP: u64 = 1000;
const MAX_SKIPPED_KEYS: usize = 2000;

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

#[derive(Clone, ZeroizeOnDrop)]
struct ChainKey(KeyMaterial);

impl ChainKey {
    fn from_bytes(bytes: KeyMaterial) -> Self {
        Self(bytes)
    }

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

    fn advance(&mut self, dh_output: &KeyMaterial) -> Result<(ChainKey, HeaderKey), RatchetError> {
        let material = hkdf_derive::<96>(dh_output, Some(&self.0), INFO_ROOT)?;
        self.0 = material[..32].try_into().unwrap();
        let ck = ChainKey::from_bytes(material[32..64].try_into().unwrap());
        let nhk = HeaderKey::from_bytes(material[64..96].try_into().unwrap());
        Ok((ck, nhk))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub struct RatchetHeader {
    pub dh_pub: KeyMaterial,
    pub pn: u64,
    pub n: u64,
}

impl RatchetHeader {
    pub(crate) fn new(dh_pub: KeyMaterial, pn: u64, n: u64) -> Self {
        Self { dh_pub, pn, n }
    }

    pub(crate) fn to_bytes(&self) -> [u8; 48] {
        let mut buf = [0u8; 48];
        buf[..32].copy_from_slice(&self.dh_pub);
        buf[32..40].copy_from_slice(&self.pn.to_be_bytes());
        buf[40..48].copy_from_slice(&self.n.to_be_bytes());
        buf
    }

    fn from_bytes(bytes: &[u8]) -> Option<Self> {
        if bytes.len() != 48 {
            return None;
        }
        let dh_pub: KeyMaterial = bytes[..32].try_into().ok()?;
        let pn = u64::from_be_bytes(bytes[32..40].try_into().ok()?);
        let n = u64::from_be_bytes(bytes[40..48].try_into().ok()?);
        Some(Self { dh_pub, pn, n })
    }
}

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

    fn get_and_remove(&mut self, hk: &KeyMaterial, n: u64) -> Option<MessageKey> {
        self.0.remove(&(*hk, n))
    }

    fn insert(&mut self, hk: KeyMaterial, n: u64, mk: MessageKey) -> Result<(), RatchetError> {
        if self.0.len() >= MAX_SKIPPED_KEYS {
            return Err(RatchetError::SkippedKeysFull);
        }
        self.0.insert((hk, n), mk);
        Ok(())
    }

    fn len(&self) -> usize {
        self.0.len()
    }
}

#[derive(ZeroizeOnDrop)]
pub struct RatchetSession {
    root_key: RootKey,
    send_chain: Option<ChainKey>,
    recv_chain: Option<ChainKey>,

    hk_send: Option<HeaderKey>,
    hk_recv: Option<HeaderKey>,
    nhk_send: Option<HeaderKey>,
    nhk_recv: Option<HeaderKey>,

    our_dh: StaticSecret,
    #[zeroize(skip)]
    our_dh_pub: X25519PublicKey,
    #[zeroize(skip)]
    their_dh_pub: Option<X25519PublicKey>,

    #[zeroize(skip)]
    ns: u64,
    #[zeroize(skip)]
    nr: u64,
    #[zeroize(skip)]
    pn: u64,

    #[zeroize(skip)]
    skipped: SkippedKeys,
}

impl RatchetSession {
    pub fn init_sender(
        shared_secret: &KeyMaterial,
        their_dh_pub_bytes: &KeyMaterial,
    ) -> Result<Self, RatchetError> {
        let their_dh_pub = X25519PublicKey::from(*their_dh_pub_bytes);

        let rk_bytes = hkdf_derive::<32>(shared_secret, None, INFO_INIT_RK)?;
        let hks_bytes = hkdf_derive::<32>(shared_secret, None, INFO_INIT_HKS)?;
        let hkr_bytes = hkdf_derive::<32>(shared_secret, None, INFO_INIT_HKR)?;

        let mut root_key = RootKey::from_bytes(rk_bytes);

        let our_dh = StaticSecret::random_from_rng(OsRng);
        let our_dh_pub = X25519PublicKey::from(&our_dh);

        let dh_out = our_dh.diffie_hellman(&their_dh_pub);
        let (send_chain, nhk_send) = root_key.advance(dh_out.as_bytes())?;

        Ok(Self {
            root_key,
            send_chain: Some(send_chain),
            recv_chain: None,
            hk_send: Some(HeaderKey::from_bytes(hks_bytes)),
            hk_recv: None,
            nhk_send: Some(nhk_send),
            nhk_recv: Some(HeaderKey::from_bytes(hkr_bytes)),
            our_dh,
            our_dh_pub,
            their_dh_pub: Some(their_dh_pub),
            ns: 0,
            nr: 0,
            pn: 0,
            skipped: SkippedKeys::new(),
        })
    }

    pub fn init_receiver(
        shared_secret: &KeyMaterial,
        our_dh: StaticSecret,
    ) -> Result<Self, RatchetError> {
        let rk_bytes = hkdf_derive::<32>(shared_secret, None, INFO_INIT_RK)?;
        let hks_bytes = hkdf_derive::<32>(shared_secret, None, INFO_INIT_HKR)?;
        let hkr_bytes = hkdf_derive::<32>(shared_secret, None, INFO_INIT_HKS)?;

        let our_dh_pub = X25519PublicKey::from(&our_dh);

        Ok(Self {
            root_key: RootKey::from_bytes(rk_bytes),
            send_chain: None,
            recv_chain: None,
            hk_send: Some(HeaderKey::from_bytes(hks_bytes)),
            hk_recv: None,
            nhk_send: None,
            nhk_recv: Some(HeaderKey::from_bytes(hkr_bytes)),
            our_dh,
            our_dh_pub,
            their_dh_pub: None,
            ns: 0,
            nr: 0,
            pn: 0,
            skipped: SkippedKeys::new(),
        })
    }

    pub fn our_public_key(&self) -> KeyMaterial {
        self.our_dh_pub.to_bytes()
    }

    pub fn encrypt(&mut self, plaintext: &[u8], ad: &[u8]) -> Result<RatchetMessage, RatchetError> {
        let send_chain = self
            .send_chain
            .as_mut()
            .ok_or(RatchetError::SendChainNotInitialized)?;
        let message_key = send_chain.advance()?;

        let header = RatchetHeader::new(self.our_dh_pub.to_bytes(), self.pn, self.ns);

        let hk = self
            .hk_send
            .as_ref()
            .ok_or(RatchetError::SendChainNotInitialized)?;
        let encrypted_header =
            encrypt_with_aad(&hk.as_message_key(), self.ns, &header.to_bytes(), &[])?;

        let aad = Self::concat_ad_enc(ad, &encrypted_header);
        let payload = encrypt_with_aad(&message_key, self.ns, plaintext, &aad)?;
        self.ns += 1;

        Ok(RatchetMessage {
            encrypted_header,
            payload,
        })
    }

    pub fn decrypt(&mut self, msg: &RatchetMessage, ad: &[u8]) -> Result<Vec<u8>, RatchetError> {
        let (header, hk_used) = self.try_decrypt_header(&msg.encrypted_header)?;
        let aad = Self::concat_ad_enc(ad, &msg.encrypted_header);

        if let Some(mk) = self.skipped.get_and_remove(&hk_used, header.n) {
            return decrypt_with_aad(&mk, &msg.payload, &aad).map_err(RatchetError::Aead);
        }

        let new_dh_key = self
            .their_dh_pub
            .map(|p| p.to_bytes() != header.dh_pub)
            .unwrap_or(true);

        if new_dh_key {
            self.skip_message_keys(header.pn)?;
            self.dh_ratchet_step(&header.dh_pub)?;
        }

        self.skip_message_keys(header.n)?;

        let recv_chain = self
            .recv_chain
            .as_mut()
            .ok_or(RatchetError::RecvChainNotInitialized)?;
        let message_key = recv_chain.advance()?;
        self.nr += 1;

        decrypt_with_aad(&message_key, &msg.payload, &aad).map_err(RatchetError::Aead)
    }

    fn try_decrypt_header(
        &self,
        enc_header: &EncryptedMessage,
    ) -> Result<(RatchetHeader, KeyMaterial), RatchetError> {
        let candidates: [Option<&HeaderKey>; 2] = [self.hk_recv.as_ref(), self.nhk_recv.as_ref()];

        for hk in candidates.iter().flatten() {
            if let Ok(bytes) = decrypt_with_aad(&hk.as_message_key(), enc_header, &[]) {
                if let Some(h) = RatchetHeader::from_bytes(&bytes) {
                    return Ok((h, hk.0));
                }
            }
        }
        Err(RatchetError::HeaderDecryptionFailed)
    }

    fn dh_ratchet_step(&mut self, their_pub_bytes: &KeyMaterial) -> Result<(), RatchetError> {
        let their_pub = X25519PublicKey::from(*their_pub_bytes);

        self.pn = self.ns;
        self.ns = 0;
        self.nr = 0;
        self.their_dh_pub = Some(their_pub);

        let dh_out = self.our_dh.diffie_hellman(&their_pub);
        let (recv_ck, new_nhk_recv) = self.root_key.advance(dh_out.as_bytes())?;
        self.hk_recv = self.nhk_recv.take();
        self.nhk_recv = Some(new_nhk_recv);
        self.recv_chain = Some(recv_ck);

        let new_dh = StaticSecret::random_from_rng(OsRng);
        let new_dh_pub = X25519PublicKey::from(&new_dh);
        if self.nhk_send.is_some() {
            self.hk_send = self.nhk_send.take();
        }

        let dh_out2 = new_dh.diffie_hellman(&their_pub);
        let (send_ck, new_nhk_send) = self.root_key.advance(dh_out2.as_bytes())?;
        self.nhk_send = Some(new_nhk_send);
        self.send_chain = Some(send_ck);

        self.our_dh = new_dh;
        self.our_dh_pub = new_dh_pub;

        Ok(())
    }

    fn skip_message_keys(&mut self, until: u64) -> Result<(), RatchetError> {
        if until > self.nr + MAX_SKIP {
            return Err(RatchetError::TooManySkipped(until - self.nr));
        }

        if let Some(recv_chain) = self.recv_chain.as_mut() {
            let hk = self.hk_recv.as_ref().map(|hk| hk.0).unwrap_or([0u8; 32]);

            while self.nr < until {
                let mk = recv_chain.advance()?;
                self.skipped.insert(hk, self.nr, mk)?;
                self.nr += 1;
            }
        }

        Ok(())
    }

    fn concat_ad_enc(ad: &[u8], enc_header: &EncryptedMessage) -> Vec<u8> {
        let mut out = Vec::with_capacity(4 + ad.len() + 8 + enc_header.ciphertext.len());
        out.extend_from_slice(&(ad.len() as u32).to_be_bytes());
        out.extend_from_slice(ad);
        out.extend_from_slice(&enc_header.nonce.to_be_bytes());
        out.extend_from_slice(&enc_header.ciphertext);
        out
    }

    pub fn skipped_keys_count(&self) -> usize {
        self.skipped.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn setup() -> (RatchetSession, RatchetSession) {
        let shared_secret = [0xABu8; 32];
        let bob_dh = StaticSecret::random_from_rng(OsRng);
        let bob_dh_pub = X25519PublicKey::from(&bob_dh);
        let alice = RatchetSession::init_sender(&shared_secret, &bob_dh_pub.to_bytes()).unwrap();
        let bob = RatchetSession::init_receiver(&shared_secret, bob_dh).unwrap();
        (alice, bob)
    }

    #[test]
    fn alice_sends_bob_receives() {
        let (mut alice, mut bob) = setup();
        let msg = alice.encrypt(b"hello bob", b"").unwrap();
        assert_eq!(bob.decrypt(&msg, b"").unwrap(), b"hello bob");
    }

    #[test]
    fn bidirectional_conversation() {
        let (mut alice, mut bob) = setup();
        let msg = alice.encrypt(b"hello bob", b"").unwrap();
        assert_eq!(bob.decrypt(&msg, b"").unwrap(), b"hello bob");
        let msg = bob.encrypt(b"hello alice", b"").unwrap();
        assert_eq!(alice.decrypt(&msg, b"").unwrap(), b"hello alice");
        let msg = alice.encrypt(b"second message", b"").unwrap();
        assert_eq!(bob.decrypt(&msg, b"").unwrap(), b"second message");
    }

    #[test]
    fn multiple_sequential_messages() {
        let (mut alice, mut bob) = setup();
        for m in &[b"one" as &[u8], b"two", b"three", b"four", b"five"] {
            let msg = alice.encrypt(m, b"").unwrap();
            assert_eq!(&bob.decrypt(&msg, b"").unwrap(), m);
        }
    }

    #[test]
    fn ciphertexts_differ_for_same_plaintext() {
        let (mut alice, _) = setup();
        let msg1 = alice.encrypt(b"repeated", b"").unwrap();
        let msg2 = alice.encrypt(b"repeated", b"").unwrap();
        assert_ne!(msg1.payload.ciphertext, msg2.payload.ciphertext);
    }

    #[test]
    fn out_of_order_delivery() {
        let (mut alice, mut bob) = setup();
        let msg1 = alice.encrypt(b"message 1", b"").unwrap();
        let msg2 = alice.encrypt(b"message 2", b"").unwrap();
        let msg3 = alice.encrypt(b"message 3", b"").unwrap();
        assert_eq!(bob.decrypt(&msg3, b"").unwrap(), b"message 3");
        assert_eq!(bob.skipped_keys_count(), 2);
        assert_eq!(bob.decrypt(&msg1, b"").unwrap(), b"message 1");
        assert_eq!(bob.decrypt(&msg2, b"").unwrap(), b"message 2");
        assert_eq!(bob.skipped_keys_count(), 0);
    }

    #[test]
    fn out_of_order_across_dh_ratchet() {
        let (mut alice, mut bob) = setup();
        let msg_a1 = alice.encrypt(b"a1", b"").unwrap();
        let msg_a2 = alice.encrypt(b"a2", b"").unwrap();
        assert_eq!(bob.decrypt(&msg_a2, b"").unwrap(), b"a2");
        assert_eq!(bob.skipped_keys_count(), 1);
        let msg_b1 = bob.encrypt(b"b1", b"").unwrap();
        assert_eq!(alice.decrypt(&msg_b1, b"").unwrap(), b"b1");
        assert_eq!(bob.decrypt(&msg_a1, b"").unwrap(), b"a1");
        assert_eq!(bob.skipped_keys_count(), 0);
    }

    #[test]
    fn header_pn_tracks_previous_chain_length() {
        let (mut alice, mut bob) = setup();
        let m1 = alice.encrypt(b"m1", b"").unwrap();
        let m2 = alice.encrypt(b"m2", b"").unwrap();
        let m3 = alice.encrypt(b"m3", b"").unwrap();
        bob.decrypt(&m1, b"").unwrap();
        bob.decrypt(&m2, b"").unwrap();
        bob.decrypt(&m3, b"").unwrap();
        let reply = bob.encrypt(b"reply", b"").unwrap();
        alice.decrypt(&reply, b"").unwrap();
        let m4 = alice.encrypt(b"m4", b"").unwrap();
        bob.decrypt(&m4, b"").unwrap();
    }

    #[test]
    fn session_ad_is_authenticated() {
        let (mut alice, mut bob) = setup();
        let ad = b"alice-id:bob-id";
        let msg = alice.encrypt(b"secret", ad).unwrap();
        assert!(bob.decrypt(&msg, ad).is_ok());
        assert!(bob.decrypt(&msg, b"wrong-ad").is_err());
    }

    #[test]
    fn wrong_shared_secret_fails() {
        let bob_dh = StaticSecret::random_from_rng(OsRng);
        let bob_dh_pub = X25519PublicKey::from(&bob_dh);
        let mut alice = RatchetSession::init_sender(&[0xAAu8; 32], &bob_dh_pub.to_bytes()).unwrap();
        let mut bob = RatchetSession::init_receiver(&[0xBBu8; 32], bob_dh).unwrap();
        let msg = alice.encrypt(b"secret", b"").unwrap();
        assert!(bob.decrypt(&msg, b"").is_err());
    }

    #[test]
    fn tampered_ciphertext_fails() {
        let (mut alice, mut bob) = setup();
        let mut msg = alice.encrypt(b"important", b"").unwrap();
        msg.payload.ciphertext[0] ^= 0xFF;
        assert!(bob.decrypt(&msg, b"").is_err());
    }

    #[test]
    fn tampered_header_fails() {
        let (mut alice, mut bob) = setup();
        let mut msg = alice.encrypt(b"important", b"").unwrap();
        msg.encrypted_header.ciphertext[0] ^= 0xFF;
        assert!(bob.decrypt(&msg, b"").is_err());
    }

    #[test]
    fn exceeding_max_skip_returns_error() {
        let (mut alice, mut bob) = setup();
        let mut messages: Vec<RatchetMessage> = Vec::new();
        for _ in 0..=(MAX_SKIP + 1) {
            messages.push(alice.encrypt(b"x", b"").unwrap());
        }
        let last = messages.len() - 1;
        assert!(matches!(
            bob.decrypt(&messages[last], b""),
            Err(RatchetError::TooManySkipped(_))
        ));
    }
}
