use bincode::config;
use ed25519_dalek::ed25519::signature::rand_core::OsRng;
use x25519_dalek::{EphemeralSecret, PublicKey};

use crate::{
    PeerConnection,
    msg::Msg,
    operations::{decrypt, encrypt},
};

#[test]
/// Tests excryption and decryption algorithms
fn test_encryption() {
    let msg = Msg::Text("This is a test text".to_string());
    let p1_private_key = EphemeralSecret::random_from_rng(OsRng);
    let p2_private_key = EphemeralSecret::random_from_rng(OsRng);
    let p2_public_key = PublicKey::from(&p2_private_key);
    let shared_secret_key = p1_private_key.diffie_hellman(&p2_public_key);
    let serialized_msg = bincode::serde::encode_to_vec(&msg, config::legacy()).unwrap();
    let encrypted_text = encrypt(shared_secret_key.as_bytes(), &serialized_msg).unwrap();
    let decrypted_text = decrypt(shared_secret_key.as_bytes(), &encrypted_text).unwrap();
    assert_eq!(msg, decrypted_text);
}
