pub mod keys;
pub mod terminal_control;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InputMode {
    NewPeer,
    RenamePeer,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConfirmMode {
    Exit,
    DeletePeer,
    DisconnectPeer,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LoadingMode {
    PeerConnect,
    StartServer,
    GroupConnect,
}
