pub mod aead;
pub mod identity;
pub mod ratchet;
use crate::crypto::ratchet::RatchetSession;
use crate::operations::derive_ratchet_key;
use bincode::config;
use ed25519_dalek::ed25519::signature::rand_core::OsRng;
use x25519_dalek::{EphemeralSecret, PublicKey};

use crate::msg::Msg;

#[test]
/// Tests encryption and decryption using Double Ratchet
fn test_cryptography() {
    let msg = Msg::Text("This is a test text".to_string());

    // Simulate handshake: both sides compute shared secret
    let p1_private_key = EphemeralSecret::random_from_rng(OsRng);
    let p2_private_key = EphemeralSecret::random_from_rng(OsRng);
    let p2_public_key = PublicKey::from(&p2_private_key);
    let shared_secret_key = p1_private_key.diffie_hellman(&p2_public_key);
    let shared_secret_bytes = *shared_secret_key.as_bytes();

    // Both sides derive Bob's ratchet key from shared secret
    let (bob_dh, bob_dh_pub) = derive_ratchet_key(&shared_secret_bytes);

    // Initialize ratchet sessions
    let mut alice =
        RatchetSession::init_sender(&shared_secret_bytes, &bob_dh_pub.to_bytes()).unwrap();
    let mut bob = RatchetSession::init_receiver(&shared_secret_bytes, bob_dh).unwrap();

    // Alice sends a message
    let serialized_msg = bincode::serde::encode_to_vec(&msg, config::standard()).unwrap();
    let ratchet_msg = alice.encrypt(&serialized_msg, b"").unwrap();

    // Bob receives and decrypts
    let decrypted_bytes = bob.decrypt(&ratchet_msg, b"").unwrap();
    let (deserialized, _) =
        bincode::serde::decode_from_slice(&decrypted_bytes, config::standard()).unwrap();
    assert_eq!(msg, deserialized);
}
