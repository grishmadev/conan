#[cfg(test)]
use crate::crypto::aead::{MessageKey, encrypt};
use crate::crypto::aead::{decrypt, decrypt_with_aad, encrypt_with_aad, hkdf_derive};

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
    // RFC 5869: empty salt is treated as zero-filled
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
