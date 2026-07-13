pub mod enums;
pub mod notification;
use bincode::config;
use std::{error::Error, io::Cursor};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{UnixListener, UnixStream},
};

use crate::{
    comm::enums::{IPCCmd, IPCRes, encode},
    constants::DAEMON_SOCKET,
};

pub struct IPC {
    socket: UnixStream,
}

impl IPC {
    /// # Errors
    pub async fn create() -> std::io::Result<Self> {
        let listener = UnixListener::bind(DAEMON_SOCKET)?;
        let (socket, _) = listener.accept().await?;
        Ok(Self { socket })
    }

    /// # Errors
    pub async fn send(&mut self, cmd: IPCRes) -> std::io::Result<()> {
        let bytes = encode(cmd);
        self.socket.write_all(&bytes).await?;
        Ok(())
    }

    /// # Errors
    pub async fn recv(&mut self) -> Result<IPCCmd, Box<dyn Error>> {
        let mut cursor = Cursor::new(Vec::<u8>::new());
        let size = self.socket.read(cursor.get_mut()).await?;
        let (msg, _) = bincode::decode_from_slice(&cursor.get_ref()[..size], config::standard())?;
        Ok(msg)
    }
}
