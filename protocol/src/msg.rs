use serde::{Deserialize, Serialize};

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

#[non_exhaustive]
#[derive(Serialize, Deserialize, Debug)]
pub enum Msg {
    Text(String),
    PublicKey([u8; 32]),
    SignedAndPublicKey(Vec<u8>, [u8; 32]),
    Begin,
    End,
}

impl From<&str> for Msg {
    fn from(value: &str) -> Self {
        Msg::Text(value.to_string())
    }
}
