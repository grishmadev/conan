//! Identity management utilities for conan.

use crate::constants::INFO_ED_TO_X;

use super::aead::{KeyMaterial, hkdf_derive};
use ed25519_dalek::{
    Signature, Signer, SigningKey, Verifier, VerifyingKey, ed25519::signature::rand_core::OsRng,
};
use serde::{Deserialize, Serialize};
use thiserror::Error;
use x25519_dalek::StaticSecret;
use zeroize::ZeroizeOnDrop;

// Errors

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

// Public identity

/// The public half of an identity: a stable address and human-readable fingerprint.
///
/// The address is a base58-encoded Ed25519 verifying key.
///
/// The fingerprint is the first 16 hex characters of the key in `XXXX-XXXX-XXXX-XXXX`
/// format, intended for out-of-band verification (e.g. reading aloud or comparing
/// on screen when establishing first contact).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PublicIdentity {
    /// Ed25519 verifying key.
    pub address: String,
    /// Human-readable fingerprint of the public key.
    pub fingerprint: String,
}

impl PublicIdentity {
    #[must_use]
    /// Constructs a [`PublicIdentity`] from a [`VerifyingKey`].
    pub fn from_verifying_key(key: &VerifyingKey) -> Self {
        let bytes = key.to_bytes();
        let address = format!("conan:{}", bs58::encode(bytes).into_string());
        let fingerprint = Self::compute_fingerprint(&bytes);
        Self {
            address,
            fingerprint,
        }
    }

    /// Recovers the [`VerifyingKey`] from the address string.
    /// # Errors
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

    /// `XXXX-XXXX-XXXX-XXXX` — first 16 hex chars of the verifying key bytes.
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

// Identity

/// A full identity: Ed25519 signing key (private) + [`PublicIdentity`].
///
/// The signing key is zeroized on drop. It is never exposed directly;
/// use [`Identity::to_secret_key`] when persistence is required, and
/// zeroize the result as soon as it has been stored.
#[derive(ZeroizeOnDrop)]
pub struct Identity {
    #[zeroize(skip)]
    pub public: PublicIdentity,
    signing_key: SigningKey,
}

impl Identity {
    /// Generates a new [`Identity`] using the OS random number generator.
    /// # Errors
    pub fn generate() -> Result<Self, IdentityError> {
        let signing_key = SigningKey::generate(&mut OsRng);
        let verifying_key = signing_key.verifying_key();
        let public = PublicIdentity::from_verifying_key(&verifying_key);
        Ok(Self {
            public,
            signing_key,
        })
    }

    /// Restores an [`Identity`] from a previously exported secret key.
    /// # Errors
    pub fn from_secret_key(bytes: KeyMaterial) -> Result<Self, IdentityError> {
        let signing_key = SigningKey::from_bytes(&bytes);
        let verifying_key = signing_key.verifying_key();
        let public = PublicIdentity::from_verifying_key(&verifying_key);
        Ok(Self {
            public,
            signing_key,
        })
    }

    #[must_use]
    /// Exports the signing key bytes, wrapped in [`zeroize::Zeroizing`].
    ///
    /// The returned value will be zeroed when dropped. Store immediately
    /// and do not keep the value in scope longer than necessary.
    pub fn to_secret_key(&self) -> zeroize::Zeroizing<KeyMaterial> {
        zeroize::Zeroizing::new(self.signing_key.to_bytes())
    }

    #[must_use]
    /// Signs `message` with this identity's private key.
    pub fn sign(&self, message: &[u8]) -> Signature {
        self.signing_key.sign(message)
    }

    /// Verifies `signature` over `message` against `public`.
    /// # Errors
    pub fn verify(
        public: &PublicIdentity,
        message: &[u8],
        signature: &Signature,
    ) -> Result<(), IdentityError> {
        let key = public.to_verifying_key()?;
        key.verify(message, signature)
            .map_err(|_| IdentityError::InvalidSignature)
    }

    #[must_use]
    /// Derives an X25519 static secret from this Ed25519 signing key.
    ///
    /// # Why HKDF here?
    ///
    /// Ed25519 and X25519 keys live in the same underlying curve group, but
    /// using the Ed25519 private scalar directly as an X25519 secret is unsafe:
    /// it would create cross-protocol linkage between signing and key-agreement
    /// operations. HKDF provides a clean domain separation.
    ///
    /// # HKDF parameters
    ///
    /// - `ikm`  = Ed25519 signing key bytes (32 bytes of strong uniform entropy → no external salt needed; the IKM itself is the entropy source).
    /// - `salt` = `b""` (empty → HKDF uses a zero-filled block internally, per RFC 5869 §2.2, which is correct when the IKM is already uniformly random).
    /// - `info` = `b"conan-v1-ed25519-to-x25519"` (domain label scoping this
    ///
    /// derivation to this protocol and purpose).
    /// # Panics
    pub fn to_x25519_secret(&self) -> x25519_dalek::StaticSecret {
        let signing_key_bytes = self.signing_key.to_bytes();
        let derived = hkdf_derive::<32>(&signing_key_bytes, None, INFO_ED_TO_X)
            .expect("HKDF cannot fail with valid-length output");
        StaticSecret::from(derived)
    }
}
