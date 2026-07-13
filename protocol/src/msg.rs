use bincode::config;
use serde::{Deserialize, Serialize};

use crate::comm::enums::IPCRes;

pub enum PeerVerified {
    Verified,
    Invalid,
}

#[derive(Default)]
pub enum PeerStatus {
    Connected,
    #[default]
    NotFound,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[non_exhaustive]
pub enum Internal {
    HsId,
    Msg(Msg),
    IPCRes(IPCRes),
    RemovePeer(u8),
    ChatSent(u8, String),
}

#[non_exhaustive]
#[derive(Serialize, Deserialize, Debug, PartialEq, Eq, Clone)]
pub enum Msg {
    Text(String),
    PublicKey([u8; 32]),
    SignedAndPublicKey(Vec<u8>, [u8; 32], [u8; 32]),
    Verified,
    Begin,
    End,
}

impl Msg {
    /// # Panics
    #[must_use]
    pub fn to_vec(&self) -> Vec<u8> {
        bincode::serde::encode_to_vec(self, config::standard()).unwrap()
    }

    /// # Panics
    #[must_use]
    pub fn from_bytes(bytes: &[u8]) -> Self {
        let (msg, _) =
            bincode::serde::decode_from_slice::<Msg, _>(bytes, config::standard()).unwrap();
        msg
    }
}

impl From<&str> for Msg {
    fn from(value: &str) -> Self {
        Msg::Text(value.to_string())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Mode {
    Normal,
    Insert { cursor_pos: usize },
}
