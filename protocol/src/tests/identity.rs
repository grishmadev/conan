use crate::crypto::identity::{Identity, PublicIdentity};

#[cfg(test)]
use x25519_dalek::PublicKey;

#[test]
fn generate_returns_valid_address() {
    let id = Identity::generate().unwrap();
    assert!(id.public.address.starts_with("conan:"));
    assert!(!id.public.address.contains(' '));
}

#[test]
fn roundtrip_secret_key() {
    let id1 = Identity::generate().unwrap();
    let bytes = *id1.to_secret_key();
    let id2 = Identity::from_secret_key(bytes).unwrap();
    assert_eq!(id1.public.address, id2.public.address);
}

#[test]
fn sign_verify_roundtrip() {
    let id = Identity::generate().unwrap();
    let msg = b"test message";
    let sig = id.sign(msg);
    assert!(Identity::verify(&id.public, msg, &sig).is_ok());
}

#[test]
fn verify_with_wrong_key_fails() {
    let id1 = Identity::generate().unwrap();
    let id2 = Identity::generate().unwrap();
    let sig = id1.sign(b"message");
    assert!(Identity::verify(&id2.public, b"message", &sig).is_err());
}

#[test]
fn x25519_derivation_is_deterministic() {
    let id = Identity::generate().unwrap();
    let s1 = id.to_x25519_secret();
    let s2 = id.to_x25519_secret();
    // comparation via public key bytes
    assert_eq!(
        PublicKey::from(&s1).to_bytes(),
        PublicKey::from(&s2).to_bytes()
    );
}

#[test]
fn x25519_differs_from_ed25519_bytes() {
    let id = Identity::generate().unwrap();
    let x_secret = id.to_x25519_secret();
    let x_pub = PublicKey::from(&x_secret).to_bytes();
    let ed_pub = id.public.to_verifying_key().unwrap().to_bytes();
    assert_ne!(x_pub, ed_pub);
}

#[test]
fn rejects_missing_prefix() {
    let fake = PublicIdentity {
        address: "notconan:abc123".into(),
        fingerprint: String::new(),
    };
    assert!(fake.to_verifying_key().is_err());
}
