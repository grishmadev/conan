use arti_client::DataStream;
use ed25519_dalek::ed25519::signature::rand_core::OsRng;
use std::{
    error::Error,
    sync::{Arc, RwLock},
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt, ReadHalf, WriteHalf},
    sync::broadcast,
};
use x25519_dalek::{EphemeralSecret, PublicKey};

use crate::{
    comm::enums::IPCRes,
    config::parse_config,
    database::DBConnection,
    database_entities::peer::{Peer, PeerData},
    msg::Msg,
    operations::{decrypt, encrypt, signing_key},
};
#[derive(Debug)]
pub struct Slave {
    pub reader: Option<ReadHalf<DataStream>>,
    pub writer: WriteHalf<DataStream>,
    pub response_sender: broadcast::Sender<(u8, Msg)>,
    /// Shared secret key after diffie helmann exchange
    pub shared_secret_key: Arc<RwLock<[u8; 32]>>,
    pub msg_sender: broadcast::Sender<IPCRes>,
}

impl Slave {
    /// Consumes Self to spawn a tokio thread that forwards data from reader to response channel
    /// Forwards as is, with no decryption
    /// Decryptions and filtering is handled by `Manager` entity
    ///
    /// # Errors
    /// # Panics
    pub fn spawn_communication(&mut self) -> Result<(), Box<dyn Error>> {
        let Some(mut reader) = self.reader.take() else {
            return Err("No Reader Associated with Slave.".into());
        };
        let ssk = Arc::clone(&self.shared_secret_key);
        let response_sender = self.response_sender.clone();
        tokio::spawn(async move {
            let mut buf = [0u8; 4096];
            loop {
                match reader.read(&mut buf).await {
                    Ok(0) => {}
                    Ok(n) => {
                        let ssk = ssk.read().unwrap();
                        let Ok(decrypted) = decrypt(&ssk, &buf[..n]) else {
                            eprintln!("Found Corrupted message: {:?}", &buf[..n]);
                            continue;
                        };
                        let msg = Msg::from_bytes(&decrypted);
                        let msg = (0, msg);
                        _ = response_sender.send(msg);
                    }
                    Err(e) => {
                        eprintln!("Error writing to socket.\n{e}");
                    }
                }
            }
        });
        Ok(())
    }

    /// Connects to Peer as listener (Allowing Connections)
    /// # Panics
    /// # Errors
    pub async fn connect_as_listener(&mut self) -> Result<(), Box<dyn Error>> {
        let Some(reader) = self.reader.as_mut() else {
            return Err("No reader found.".into());
        };
        let size = reader.read_u16().await? as usize;
        let mut buf = vec![0u8; size];
        let size = reader.read_exact(&mut buf).await?;
        let recv_msg = Msg::from_bytes(&buf[..size]);

        let Msg::PublicKey(remote_public_key) = recv_msg else {
            return Err("Did not receive remote public key. aborting.".into());
        };
        let config = parse_config()?;
        let local_private_key = EphemeralSecret::random_from_rng(OsRng);
        let local_public_key = PublicKey::from(&local_private_key);
        let signing_key = signing_key(config.arti_key_store)?;
        let mut data = vec![];
        data.extend_from_slice(&remote_public_key);
        data.extend_from_slice(local_public_key.as_bytes());
        let data: [u8; 64] = data.try_into().unwrap();
        let signature = signing_key.sign(&data);
        let msg = Msg::SignedAndPublicKey(
            signature.to_bytes().to_vec(),
            remote_public_key,
            local_public_key.to_bytes(),
        );
        let payload = msg.to_vec();
        println!("Sending Signature & Public Key to peer.");
        #[allow(clippy::cast_possible_truncation)]
        self.writer.write_u16(payload.len() as u16).await?;
        self.writer.write_all(&payload).await?;
        self.writer.flush().await?;
        println!("Reading peer's public key.");
        let size = reader.read_u16().await? as usize;
        let mut buf = vec![0u8; size];
        let size = reader.read_exact(&mut buf).await?;
        println!("Parsing peer's public key.");
        let recv_msg = Msg::from_bytes(&buf[..size]);

        let Msg::PublicKey(remote_public_key) = recv_msg else {
            return Err("Did not receive remote public key. aborting.".into());
        };
        let rpk = PublicKey::from(remote_public_key);
        let shared_secret_key = local_private_key.diffie_hellman(&rpk);
        *self.shared_secret_key.write().unwrap() = *shared_secret_key.as_bytes();
        // reader.read_to_end(&mut vec![]).await?;
        println!("Verification Complete.");
        Ok(())
    }

    /// Encrypts message before writing to writer
    pub async fn send(&mut self, msg: Vec<u8>) -> Result<(), Box<dyn Error>> {
        let ssk = self.shared_secret_key.read().unwrap();
        let encrypted = encrypt(&ssk, &msg)?;
        self.writer.write_all(&encrypted).await?;
        self.writer.flush().await?;
        Ok(())
    }

    /// Decrypts message before returning
    pub async fn recv(&mut self) -> Result<Vec<u8>, Box<dyn Error>> {
        let mut buf = [0u8; 4096];
        let size = self.reader.as_mut().unwrap().read(&mut buf).await?;
        let ssk = self.shared_secret_key.read().unwrap();
        let decrypted = decrypt(&ssk, &buf[..size])?;
        Ok(decrypted)
    }
}
