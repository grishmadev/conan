#[cfg(test)]
use crate::crypto::ratchet::RatchetSession;
use crate::{
    constants::MAX_SKIP,
    crypto::ratchet::{RatchetError, RatchetMessage},
};
use ed25519_dalek::ed25519::signature::rand_core::OsRng;
use x25519_dalek::{PublicKey, StaticSecret};
fn setup() -> (RatchetSession, RatchetSession) {
    let shared_secret = [0xABu8; 32];
    let local_private_key = StaticSecret::random_from_rng(OsRng);
    let local_public_key = PublicKey::from(&local_private_key);
    let remote = RatchetSession::init_sender(&shared_secret, &local_public_key.to_bytes()).unwrap();
    let local = RatchetSession::init_receiver(&shared_secret, local_private_key).unwrap();
    (remote, local)
}

#[test]
fn remote_sends_local_receives() {
    let (mut remote, mut local) = setup();
    let msg = remote.encrypt(b"hello local", b"").unwrap();
    assert_eq!(local.decrypt(&msg, b"").unwrap(), b"hello local");
}

#[test]
fn bidirectional_conversation() {
    let (mut remote, mut local) = setup();
    let msg = remote.encrypt(b"hello local", b"").unwrap();
    assert_eq!(local.decrypt(&msg, b"").unwrap(), b"hello local");
    let msg = local.encrypt(b"hello remote", b"").unwrap();
    assert_eq!(remote.decrypt(&msg, b"").unwrap(), b"hello remote");
    let msg = remote.encrypt(b"second message", b"").unwrap();
    assert_eq!(local.decrypt(&msg, b"").unwrap(), b"second message");
}

#[test]
fn multiple_sequential_messages() {
    let (mut remote, mut local) = setup();
    for m in &[b"one" as &[u8], b"two", b"three", b"four", b"five"] {
        let msg = remote.encrypt(m, b"").unwrap();
        assert_eq!(&local.decrypt(&msg, b"").unwrap(), m);
    }
}

#[test]
fn ciphertexts_differ_for_same_plaintext() {
    let (mut remote, _) = setup();
    let msg1 = remote.encrypt(b"repeated", b"").unwrap();
    let msg2 = remote.encrypt(b"repeated", b"").unwrap();
    assert_ne!(msg1.payload.ciphertext, msg2.payload.ciphertext);
}

#[test]
fn out_of_order_delivery() {
    let (mut remote, mut local) = setup();
    let msg1 = remote.encrypt(b"message 1", b"").unwrap();
    let msg2 = remote.encrypt(b"message 2", b"").unwrap();
    let msg3 = remote.encrypt(b"message 3", b"").unwrap();
    assert_eq!(local.decrypt(&msg3, b"").unwrap(), b"message 3");
    assert_eq!(local.skipped_keys_count(), 2);
    assert_eq!(local.decrypt(&msg1, b"").unwrap(), b"message 1");
    assert_eq!(local.decrypt(&msg2, b"").unwrap(), b"message 2");
    assert_eq!(local.skipped_keys_count(), 0);
}

#[test]
fn out_of_order_across_dh_ratchet() {
    let (mut remote, mut local) = setup();
    let rem_msg_1 = remote.encrypt(b"a1", b"").unwrap();
    let rem_msg_2 = remote.encrypt(b"a2", b"").unwrap();
    assert_eq!(local.decrypt(&rem_msg_2, b"").unwrap(), b"a2");
    assert_eq!(local.skipped_keys_count(), 1);
    let loc_msg_1 = local.encrypt(b"b1", b"").unwrap();
    assert_eq!(remote.decrypt(&loc_msg_1, b"").unwrap(), b"b1");
    assert_eq!(local.decrypt(&rem_msg_1, b"").unwrap(), b"a1");
    assert_eq!(local.skipped_keys_count(), 0);
}

#[test]
fn header_prev_chain_length_tracks_previous_chain_length() {
    let (mut remote, mut local) = setup();
    let m1 = remote.encrypt(b"m1", b"").unwrap();
    let m2 = remote.encrypt(b"m2", b"").unwrap();
    let m3 = remote.encrypt(b"m3", b"").unwrap();
    local.decrypt(&m1, b"").unwrap();
    local.decrypt(&m2, b"").unwrap();
    local.decrypt(&m3, b"").unwrap();
    let reply = local.encrypt(b"reply", b"").unwrap();
    remote.decrypt(&reply, b"").unwrap();
    let m4 = remote.encrypt(b"m4", b"").unwrap();
    local.decrypt(&m4, b"").unwrap();
}

#[test]
fn session_ad_is_authenticated() {
    let (mut remote, mut local) = setup();
    let ad = b"remote-id:local-id";
    let msg = remote.encrypt(b"secret", ad).unwrap();
    assert!(local.decrypt(&msg, ad).is_ok());
    assert!(local.decrypt(&msg, b"wrong-ad").is_err());
}

#[test]
fn wrong_shared_secret_fails() {
    let local_private_key = StaticSecret::random_from_rng(OsRng);
    let local_dh_pub = PublicKey::from(&local_private_key);
    let mut remote = RatchetSession::init_sender(&[0xAAu8; 32], &local_dh_pub.to_bytes()).unwrap();
    let mut local = RatchetSession::init_receiver(&[0xBBu8; 32], local_private_key).unwrap();
    let msg = remote.encrypt(b"secret", b"").unwrap();
    assert!(local.decrypt(&msg, b"").is_err());
}

#[test]
fn tampered_ciphertext_fails() {
    let (mut remote, mut local) = setup();
    let mut msg = remote.encrypt(b"important", b"").unwrap();
    msg.payload.ciphertext[0] ^= 0xFF;
    assert!(local.decrypt(&msg, b"").is_err());
}

#[test]
fn tampered_header_fails() {
    let (mut remote, mut local) = setup();
    let mut msg = remote.encrypt(b"important", b"").unwrap();
    msg.encrypted_header.ciphertext[0] ^= 0xFF;
    assert!(local.decrypt(&msg, b"").is_err());
}

#[test]
fn exceeding_max_skip_returns_error() {
    let (mut remote, mut local) = setup();
    let mut messages: Vec<RatchetMessage> = Vec::new();
    for _ in 0..=(MAX_SKIP + 1) {
        messages.push(remote.encrypt(b"x", b"").unwrap());
    }
    let last = messages.len() - 1;
    assert!(matches!(
        local.decrypt(&messages[last], b""),
        Err(RatchetError::TooManySkipped(_))
    ));
}
