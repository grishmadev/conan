use bincode::config;
use openmls::prelude::Welcome;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug, PartialEq, Eq, Clone)]
#[non_exhaustive]
pub enum Msg {
    /// A message for a text from peer (message)
    Text(String),
    /// Public Key for identification of peer (id)
    PublicKey([u8; 32]),
    /// A combination of keys for the verification process
    SignedAndPublicKey(Vec<u8>, [u8; 32], [u8; 32]),
    /// Send signal to verify peer
    Verified,
    /// Send signal to join the group
    JoinGroup,
    /// Send Welcome to peer (welcome)
    Welcome(Welcome),
    /// Sends a Success message signifying complete group verification
    GroupSuccess,
    /// Send `KeyPackage` over to another peer to join (serialized keypackage)
    KeyPackage(Vec<u8>),
    /// Send Error on group error (error message)
    GroupError(String),
    /// This message is sent to a peer when the group is successfully joined (serialized group id)
    GroupVerified(Vec<u8>),
    /// Send message/commits to a group (serialized group id, serialized message/commit)
    GroupAction(Vec<u8>, Vec<u8>),
    /// Send message to connect to group from the other side (serialized group id)
    InitiateGroup(Vec<u8>),
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

#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum SlaveCmd {
    Msg(Msg),
    Shutdown,
}
