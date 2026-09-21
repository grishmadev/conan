use bincode::{Decode, Encode};
use database::entities::{group::DBGroup, peer::Peer, tuichat::TuiChat};
use serde::{Deserialize, Serialize};

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
        chats: Vec<TuiChat>,
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
        chats: Vec<TuiChat>,
    },
    /// Response for Group Renamed (group id)
    RenamedGroup(u16),
    /// Response for Group Connected (message, group id)
    GroupConnected(String, u16),
}
