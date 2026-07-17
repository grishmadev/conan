use bincode::config as cfg;
use std::{fs, sync::mpsc};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::UnixListener,
    sync::broadcast,
};
use tor_hsservice::HsId;

use crate::{
    comm::enums::{IPCCmd, IPCRes, encode},
    config::parse_config,
};

#[derive(Debug)]
pub struct Master {
    /// Self onion Address
    pub self_addr: Option<(HsId, u16)>,
    /// Sender Channel for sending internal Commands to Manager
    pub worker_sender: mpsc::Sender<IPCCmd>,
    /// Receiver Channel for transferring messages to TUI
    pub msg_receiver: broadcast::Receiver<IPCRes>,
}

impl Master {
    #[must_use]
    pub fn build(
        self_addr: Option<(HsId, u16)>,
        worker_sender: mpsc::Sender<IPCCmd>,
        msg_receiver: broadcast::Receiver<IPCRes>,
    ) -> Self {
        Self {
            self_addr,
            worker_sender,
            msg_receiver,
        }
    }

    /// # Errors
    pub fn setup_communication(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        let Ok(config) = parse_config() else {
            eprintln!("Something wrong with config file.");
            return Ok(());
        };
        let sock_path = config.socket_path.clone();
        let mut sock_dir = sock_path.split('/').collect::<Vec<_>>();
        sock_dir.pop();
        _ = fs::create_dir_all(sock_dir.join("/"));
        if let Err(e) = fs::remove_file(&sock_path) {
            println!("fs error: {e}");
        }
        let listener = match UnixListener::bind(&sock_path) {
            Ok(s) => s,
            Err(e) => {
                println!("Unix Error: {e}");
                return Ok(());
            }
        };
        let msg_rec = self.msg_receiver.resubscribe();
        let worker_sen = self.worker_sender.clone();
        tokio::spawn(async move {
            loop {
                let Ok((socket, _)) = listener.accept().await else {
                    continue;
                };
                let (mut sock_reader, mut sock_writer) = tokio::io::split(socket);
                let worker_sen = worker_sen.clone();
                tokio::spawn(async move {
                    let mut buf = [0u8; 4096];
                    loop {
                        match sock_reader.read(&mut buf).await {
                            Ok(0) => {
                                println!("TUI disconnected. Exiting.");
                                std::process::exit(0);
                            }
                            Ok(n) => {
                                let cmd = match bincode::decode_from_slice::<IPCCmd, _>(
                                    &buf[..n],
                                    cfg::standard(),
                                ) {
                                    Ok((cmd, _)) => cmd,
                                    Err(e) => {
                                        eprintln!("Error while decoding message: {e}");
                                        continue;
                                    }
                                };
                                if let Err(e) = worker_sen.send(cmd) {
                                    eprintln!("Error while writing to worker channel. {e}");
                                }
                            }
                            Err(e) => {
                                eprintln!("Error while reading from IPC Socket...\n{e}");
                            }
                        }
                    }
                });

                let msg_rec = msg_rec.resubscribe();
                tokio::spawn(async move {
                    let msg_rec = msg_rec.resubscribe();
                    loop {
                        let mut msg_rec = msg_rec.resubscribe();
                        match msg_rec.recv().await {
                            Ok(res) => {
                                let res_bytes = encode(res);
                                _ = sock_writer.write_all(&res_bytes).await;
                                _ = sock_writer.flush().await;
                            }
                            Err(e) => {
                                eprintln!("Error while writing to IPC Socket...\n{e}");
                            }
                        }
                    }
                });
            }
        });
        Ok(())
    }
}
