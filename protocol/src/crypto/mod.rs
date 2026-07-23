//! Cryptographic primitives for conan.
//!
//! This module implements the cryptographic layer of the conan E2E messaging
//! protocol, following the [Signal Double Ratchet specification][dr] (rev. 4, 2025).
//!
//! # Modules
//!
//! - [`aead`] — ChaCha20-Poly1305 authenticated encryption and HKDF-SHA256
//!   key derivation. These are the low-level building blocks used by all other
//!   modules.
//! - [`identity`] — Ed25519 identity keys: key generation, signing, verification,
//!   address encoding (`conan:<base58>`), and X25519 key derivation for DH.
//! - [`ratchet`] — Double Ratchet session state. Provides forward secrecy and
//!   break-in recovery for an established session.
//!
//! # Security properties
//!
//! All secret key material is zeroized on drop via the [`zeroize`] crate.
//! See individual type documentation for details on what is and is not zeroized.
//!
//! # `no_std`
//!
//! This crate requires `std`. It uses [`std::collections::HashMap`],
//! [`rand::rngs::OsRng`], and [`thiserror`], none of which are available in
//! `no_std` environments. `no_std` support is not a planned goal for conan,
//! which targets desktop terminals exclusively.
//!
//! [dr]: https://signal.org/docs/specifications/doubleratchet/
//! [snow]: https://docs.rs/snow
//! [arti]: https://gitlab.torproject.org/tpo/core/arti

pub mod aead;
pub mod identity;
pub mod ratchet;
