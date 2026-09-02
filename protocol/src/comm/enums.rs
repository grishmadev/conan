use std::error::Error;

use bincode::{Decode, Encode, config};
use serde::{Deserialize, Serialize, de::DeserializeOwned};

use crate::entities::database::{chat::Chat, group::DBGroup, peer::Peer};

pub struct Chats {}

#[derive(Debug, Clone, PartialEq, Eq, Decode, Encode)]
#[non_exhaustive]
pub enum IPCCmd {
    /// Command to start server
    StartServer,
    /// Get list of all chats with peers
    PingChat,
    /// Basic Tick
    Tick,
    /// Connect with given peer (peer address, port)
    Connect(String, u16),
    /// Send Text to Peer (peer index, text)
    Text(u8, String),
    /// Get list of all peers
    PeerList,
    /// Get Chat List for a given contact
    ChatList {
        peer_id: u8,
        msg_amount: u8,
    },
    /// Get Group Chats for a given group
    GroupChatList {
        /// Group id
        group_idx: u8,
        /// Last amount of messages to get
        msg_amount: u8,
    },
    /// Renames a Peer with provided name
    RenamePeer(u8, String),
    /// Send Text to Group (group index, text)
    GroupText(u8, String),
    /// Deletes peer with provided database idx
    DeletePeer(u32),
    /// Deletes group with provided database idx
    DeleteGroup(u32),
    /// Creates a new group with provided name or generate a random name
    NewGroup(Option<String>),
    /// Add member to a group (group index, peer index)
    AddToGroup(u32, u32),
    /// Get List of groups
    GroupList,
    /// Connect to a group given database index
    GroupConnect(u16),
    InitiateGroup(Vec<u8>),
}

#[derive(Debug, Clone, PartialEq, Eq, Decode, Encode, Serialize, Deserialize)]
#[non_exhaustive]
pub enum IPCRes {
    ServerStarted(bool),
    Connected(String, u16),
    Text(u8, String),
    Notification(String),
    Error(String),
    PeerList(Vec<Peer>),
    ChatList { peer_id: u8, chats: Vec<Chat> },
    Tock,
    DeletedPeer(u32),
    DeletedGroup(u32),
    RenamedPeer(u32),
    GroupList(Vec<DBGroup>),
    GroupChatList { group_idx: u8, chats: Vec<Chat> },
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
