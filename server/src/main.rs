use conanprotocol::{
    comm::enums::{IPCCmd, IPCRes},
    config::parse_config,
    entities::{
        database::{
            chat::{Chat, ChatData},
            peer::PeerData,
        },
        server::{manager::Manager, master::Master},
    },
    msg::Msg,
    operations::send,
};
use std::{error::Error, sync::Arc};

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let config = parse_config()?;
    let (worker_sender, worker_receiver) = std::sync::mpsc::channel::<IPCCmd>();
    let (msg_sender, msg_receiver) = tokio::sync::broadcast::channel::<IPCRes>(100);
    let mut master = Master::build(None, worker_sender, msg_receiver);
    println!("Starting Master...");
    master.setup_communication(&config)?;
    let mut manager = Manager::create(msg_sender.clone(), config).await?;
    println!("Starting Manager..");
    manager.init_server()?;
    println!("Manager Started. Establishing Message Routes..");
    if manager.setup_slave_communication().is_ok() {
        manager.msg_sender.send(IPCRes::ServerStarted)?;
    } else {
        manager
            .msg_sender
            .send(IPCRes::Error("Could not Start Server.".into()))?;
    }
    println!("All Set.");
    loop {
        if let Ok(s) = worker_receiver.recv() {
            match s {
                IPCCmd::Tick => {
                    manager.msg_sender.send(IPCRes::Tock)?;
                }
                IPCCmd::Connect(addr, port) => {
                    if let Err(e) = manager.connect_as_dialer((addr.clone(), port)) {
                        return Err(format!("Cannot connect as Dialer:\n{e}").into());
                    }
                }
                IPCCmd::Text(idx, text) => {
                    let peers = Arc::clone(&manager.peers);
                    let mut peers = peers.write().unwrap();
                    let Some(target) = peers.get_mut(&idx) else {
                        println!("Cannot find target peer.");
                        continue;
                    };
                    let chat = Chat::chat_to_send(&text, u32::from(idx));
                    manager.dbconn.insert_chat(chat)?;
                    let encoded = Msg::Text(text).to_vec();
                    send(
                        &mut target.writer,
                        encoded,
                        Arc::clone(&target.shared_secret_key),
                    )
                    .await?;
                }
                IPCCmd::PeerList => {
                    let mut peers = manager.dbconn.list_all_peers()?;
                    if let Ok(mem_slaves) = Arc::clone(&manager.peers).read() {
                        let iter = peers.iter_mut();
                        #[allow(clippy::cast_possible_truncation)]
                        for p in iter {
                            p.connected = mem_slaves.contains_key(&(p.id as u8));
                        }
                    }
                    manager.msg_sender.send(IPCRes::PeerList(peers))?;
                }
                IPCCmd::DeletePeer(idx) => {
                    manager.dbconn.delete_peer(idx)?;
                    manager.msg_sender.send(IPCRes::DeletedPeer(idx))?;
                }
                IPCCmd::RenamePeer(idx, new_name) => {
                    let idx = u32::from(idx);
                    manager.dbconn.rename_peer(idx, new_name)?;
                    manager.msg_sender.send(IPCRes::RenamedPeer(idx))?;
                }
                IPCCmd::ChatList {
                    peer_id,
                    msg_amount,
                } => {
                    let chats = manager.dbconn.list_chat_from(peer_id, msg_amount)?;
                    manager
                        .msg_sender
                        .send(IPCRes::ChatList { peer_id, chats })?;
                }
                _ => unimplemented!(),
            }
        } else {
            manager.msg_sender.send(IPCRes::Error(
                "Could not parse or reply to message.".to_string(),
            ))?;
        }
    }
}
