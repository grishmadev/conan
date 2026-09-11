use std::error::Error;

use bincode::{Decode, Encode, config};
use serde::{Deserialize, Serialize, de::DeserializeOwned};

use database::entities::{chat::Chat, group::DBGroup, peer::Peer};

#[derive(Debug, Clone, PartialEq, Eq, Decode, Encode, Serialize, Deserialize)]
#[non_exhaustive]
pub enum IPCCmd {
    /// Command to start server
    StartServer,
    /// Get list of all chats with peers
    PingChat,
    /// Basic Tick
    Tick,
    /// Add a given peer (peer address, port)
    AddPeer(String, u16),
    /// Connect with given peer (peer id)
    Connect(u16),
    /// Disconnect from given peer (peer idx)
    Disconnect(u16),
    /// Send Text to Peer (peer index, text)
    Text(u16, String),
    /// Get list of all peers
    PeerList,
    /// Get Chat List for a given contact
    ChatList { peer_id: u16, msg_amount: u8 },
    /// Get Group Chats for a given group
    GroupChatList {
        /// Group id
        group_idx: u16,
        /// Last amount of messages to get
        msg_amount: u8,
    },
    /// Renames a Peer with provided name
    RenamePeer(u16, String),
    /// Send Text to Group (group index, text)
    GroupText(u16, String),
    /// Deletes peer with provided database idx
    DeletePeer(u16),
    /// Deletes group with provided database idx
    DeleteGroup(u16),
    /// Creates a new group with provided name or generate a random name
    NewGroup(Option<String>),
    /// Add member to a group (group index, peer index)
    AddToGroup(u16, u16),
    /// Renames Selected group (u16)
    RenameGroup(u16, String),
    /// Get List of groups
    GroupList,
    /// Connect to a group given database index
    GroupConnect(u16),
    /// Send Command to other peers to join a group as well
    InitiateGroup(Vec<u8>),
}

#[derive(Debug, Clone, PartialEq, Eq, Decode, Encode, Serialize, Deserialize)]
#[non_exhaustive]
pub enum IPCRes {
    /// Response for Server Status
    ServerStarted(bool),
    /// Response for added peer (Peer)
    AddedPeer(Peer),
    /// Response for successful connection (peer id, connected)
    Connected(u16, bool),
    /// Response for Successful Text Response (peer idx, message)
    Text(u16, String),
    /// Response for Notification
    Notification(String),
    /// Response for general Error
    Error(String),
    /// Response for List of Peers
    PeerList(Vec<Peer>),
    /// Response for Chat List
    ChatList {
        /// Peer id
        peer_id: u16,
        /// List of Chats
        chats: Vec<Chat>,
    },
    /// Response for [`IPCCmd::Tick`]
    Tock,
    /// Response for Deleted Peer
    DeletedPeer(u16),
    /// Response for Group Deleted
    DeletedGroup(u16),
    /// Response for Peer Renamed
    RenamedPeer(u16),
    /// Response for List of groups
    GroupList(Vec<DBGroup>),
    /// Response for Group Chats
    GroupChatList {
        /// Group index
        group_idx: u16,
        /// List of chats
        chats: Vec<Chat>,
    },
    /// Response for Group Renamed (group id)
    RenamedGroup(u16),
    /// Response for Group Connected (message, group id)
    GroupConnected(String, u16),
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
