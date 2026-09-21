use bincode::{Decode, Encode};
use serde::{Deserialize, Serialize};

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
    /// Remove member from a group (group index, peer index)
    RemoveFromGroup(u16, u16),
    /// Renames Selected group (u16)
    RenameGroup(u16, String),
    /// Get List of groups
    GroupList,
    /// Connect to a group given database index
    GroupConnect(u16),
    /// Send Command to other peers to join a group as well
    InitiateGroup(Vec<u8>),
    /// Message to promote a Member to Admin
    Promote(u16, u16),
    /// Message to demote an Admin to Member
    Demote(u16, u16),
}
