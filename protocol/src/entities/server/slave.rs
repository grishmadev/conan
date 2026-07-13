use arti_client::DataStream;
use rand::random_range;
use rusqlite::Connection;
use std::{
    error::Error,
    sync::{Arc, RwLock},
};
use tokio::{
    io::{ReadHalf, WriteHalf},
    sync::broadcast,
};
use tor_hsservice::RunningOnionService;

use crate::{
    comm::enums::IPCRes,
    config::ConanConfig,
    entities::database::peer::{Peer, PeerData},
    extras::generate_name,
    msg::{Internal, Msg},
    operations::{listener_actor, recv},
};
pub struct Slave {
    pub id: u8,
    pub reader: Option<ReadHalf<DataStream>>,
    pub writer: WriteHalf<DataStream>,
    pub response_sender: broadcast::Sender<(u8, Internal)>,
    /// Shared secret key after diffie helmann exchange
    pub shared_secret_key: Arc<RwLock<[u8; 32]>>,
    pub msg_sender: broadcast::Sender<IPCRes>,
    pub service: Arc<RunningOnionService>,
    pub config: ConanConfig,
    pub dbconn: Connection,
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
        let id = self.id;
        tokio::spawn(async move {
            let mut threshold = 5;
            while threshold != 0 {
                match recv(&mut reader, Arc::clone(&ssk)).await {
                    Ok(data) => {
                        let msg = Msg::from_bytes(&data);
                        println!("msg: {:?}", msg);
                        let msg = (0, Internal::Msg(msg));
                        _ = response_sender.send(msg);
                        threshold = 5;
                        continue;
                    }
                    Err(e) => {
                        eprintln!("Error writing to socket.\n{e}");
                        threshold -= 1;
                    }
                }
                if threshold != 0 {
                    eprintln!("Retrying read...");
                } else {
                    response_sender
                        .send((id, Internal::RemovePeer(id)))
                        .unwrap();
                }
            }
        });
        Ok(())
    }

    /// Connects to Peer as listener (Allowing Connections)
    /// # Panics
    /// # Errors
    pub async fn connect_as_listener(&mut self) -> Result<u8, Box<dyn Error>> {
        let Some(reader) = self.reader.as_mut() else {
            return Err("No reader found.".into());
        };
        let local_hsid = self
            .service
            .onion_address()
            .ok_or("Could not get Onion Address")?;
        let mut shared_secret_key = None;
        let mut remote_onion_key = None;
        listener_actor(
            self.config.arti_key_store.clone(),
            reader,
            &mut self.writer,
            &mut shared_secret_key,
            &mut remote_onion_key,
            local_hsid,
        )
        .await?;
        let Some(shared_secret_key) = shared_secret_key else {
            return Err("Could not parse Shared Secret Key.".into());
        };
        *self.shared_secret_key.write().unwrap() = shared_secret_key;
        let Some(remote_hsid) = remote_onion_key else {
            return Err("No Remote HsId key assigned. Aborting.".into());
        };
        let known = self.dbconn.get_peer_from_addr(&remote_hsid)?;
        let mut idx = 0;
        if known.is_none() {
            let name = generate_name(random_range(4..10));
            let peer = self.dbconn.insert_peer(Peer::build(&name, &remote_hsid))?;
            self.id = peer.id as u8;
            idx = peer.id as u8;
        }
        let name = if let Some(peer) = known {
            peer.name
        } else {
            "A New Peer".into()
        };
        self.msg_sender.send(IPCRes::Notification(format!(
            "{name} just connected to you."
        )))?;
        Ok(idx)
    }
}
