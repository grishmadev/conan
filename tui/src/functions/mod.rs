pub mod keys;
pub mod tui_handler;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InputMode {
    NewPeer,
    RenamePeer,
    RenameGroup,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConfirmMode {
    Exit,
    DeletePeer,
    DisconnectPeer,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LoadingMode {
    PeerConnect(u16),
    StartServer,
    GroupConnect(u16),
}
