use std::error::Error;

use bincode::{Decode, Encode, config};
use serde::{Serialize, de::DeserializeOwned};

use crate::entities::database::{chat::Chat, peer::Peer};

#[derive(Debug, Clone, PartialEq, Eq, Decode, Encode)]
#[non_exhaustive]
pub enum IPCCmd {
    StartServer,
    Connect(String, u16),
    Text(u8, String),
    PeerList,
    ChatList { peer_id: u8, msg_amount: u8 },
    Tick,
}

#[derive(Debug, Clone, PartialEq, Eq, Decode, Encode)]
#[non_exhaustive]
pub enum IPCRes {
    ServerStarted,
    Connected(String, u16),
    Text(u8, String),
    Notification(String),
    Error(String),
    PeerList(Vec<Peer>),
    ChatList { peer_id: u8, chats: Vec<Chat> },
    Tock,
}

/// # Panics
pub fn to_bytes<T>(msg: T) -> Vec<u8>
where
    T: Serialize,
{
    bincode::serde::encode_to_vec(msg, config::standard()).unwrap()
}

/// # Panics
pub fn encode<T>(msg: T) -> Vec<u8>
where
    T: Encode,
{
    bincode::encode_to_vec(msg, config::standard()).unwrap()
}
/// # Errors
pub fn from_bytes<T>(bytes: &[u8]) -> Result<T, Box<dyn Error>>
where
    T: DeserializeOwned,
{
    let d = bincode::serde::decode_from_slice::<T, _>(bytes, config::standard())?;
    Ok(d.0)
}
