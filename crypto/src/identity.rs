use crate::aead::{KeyMaterial, hkdf_derive};
use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};
use rand_core::OsRng;
use serde::{Deserialize, Serialize};
use thiserror::Error;
use zeroize::ZeroizeOnDrop;

#[derive(Error, Debug)]
pub enum IdentityError {
    #[error("error while generating identity: {0}")]
    Generation(String),
    #[error("invalid public key: {0}")]
    InvalidPublicKey(String),
    #[error("invalid signature")]
    InvalidSignature,
    #[error("error while serializing: {0}")]
    Serialization(#[from] serde_json::Error),
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PublicIdentity {
    pub address: String,
    pub fingerprint: String,
}

impl PublicIdentity {
    pub fn from_verifying_key(key: &VerifyingKey) -> Self {
        let bytes = key.to_bytes();
        let address = format!("conan:{}", bs58::encode(bytes).into_string());
        let fingerprint = Self::compute_fingerprint(&bytes);
        Self {
            address,
            fingerprint,
        }
    }

    pub fn to_verifying_key(&self) -> Result<VerifyingKey, IdentityError> {
        let encoded = self
            .address
            .strip_prefix("conan:")
            .ok_or_else(|| IdentityError::InvalidPublicKey("missing conan: prefix".into()))?;

        let bytes = bs58::decode(encoded)
            .into_vec()
            .map_err(|e| IdentityError::InvalidPublicKey(e.to_string()))?;

        let arr: KeyMaterial = bytes
            .try_into()
            .map_err(|_| IdentityError::InvalidPublicKey("invalid key length".into()))?;

        VerifyingKey::from_bytes(&arr).map_err(|e| IdentityError::InvalidPublicKey(e.to_string()))
    }

    fn compute_fingerprint(bytes: &KeyMaterial) -> String {
        let hex = hex::encode(bytes);
        let chars: Vec<char> = hex.to_uppercase().chars().collect();
        chars
            .chunks(4)
            .take(4)
            .map(|c| c.iter().collect::<String>())
            .collect::<Vec<_>>()
            .join("-")
    }
}

#[derive(ZeroizeOnDrop)]
pub struct Identity {
    #[zeroize(skip)]
    pub public: PublicIdentity,
    signing_key: SigningKey,
}

impl Identity {
    pub fn generate() -> Result<Self, IdentityError> {
        let signing_key = SigningKey::generate(&mut OsRng);
        let verifying_key = signing_key.verifying_key();
        let public = PublicIdentity::from_verifying_key(&verifying_key);
        Ok(Self {
            public,
            signing_key,
        })
    }

    pub fn from_secret_key(bytes: KeyMaterial) -> Result<Self, IdentityError> {
        let signing_key = SigningKey::from_bytes(&bytes);
        let verifying_key = signing_key.verifying_key();
        let public = PublicIdentity::from_verifying_key(&verifying_key);
        Ok(Self {
            public,
            signing_key,
        })
    }

    pub fn to_secret_key(&self) -> zeroize::Zeroizing<KeyMaterial> {
        zeroize::Zeroizing::new(self.signing_key.to_bytes())
    }

    pub fn sign(&self, message: &[u8]) -> Signature {
        self.signing_key.sign(message)
    }

    pub fn verify(
        public: &PublicIdentity,
        message: &[u8],
        signature: &Signature,
    ) -> Result<(), IdentityError> {
        let key = public.to_verifying_key()?;
        key.verify(message, signature)
            .map_err(|_| IdentityError::InvalidSignature)
    }

    pub fn to_x25519_secret(&self) -> x25519_dalek::StaticSecret {
        let ed_bytes = self.signing_key.to_bytes();
        let derived = hkdf_derive::<32>(&ed_bytes, None, b"conan-v1-ed25519-to-x25519")
            .expect("HKDF cannot fail with valid-length output");
        x25519_dalek::StaticSecret::from(derived)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
        use x25519_dalek::PublicKey;
        assert_eq!(
            PublicKey::from(&s1).to_bytes(),
            PublicKey::from(&s2).to_bytes()
        );
    }

    #[test]
    fn x25519_differs_from_ed25519_bytes() {
        let id = Identity::generate().unwrap();
        let x_secret = id.to_x25519_secret();
        use x25519_dalek::PublicKey;
        let x_pub = PublicKey::from(&x_secret).to_bytes();
        let ed_pub = id.public.to_verifying_key().unwrap().to_bytes();
        assert_ne!(x_pub, ed_pub);
    }

    #[test]
    fn rejects_missing_prefix() {
        let fake = PublicIdentity {
            address: "notconan:abc123".into(),
            fingerprint: "".into(),
        };
        assert!(fake.to_verifying_key().is_err());
    }
}
