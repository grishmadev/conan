use crate::comm::enums::{ipccmd::IPCCmd, ipcres::IPCRes, msg::Msg};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[non_exhaustive]
pub enum Internal {
    HsId,
    Msg(Msg),
    IPCRes(IPCRes),
    IPCCmd(IPCCmd),
    RemovePeer(u16),
    ChatSent(u16, String),
}
