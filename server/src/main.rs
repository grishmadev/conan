use conanprotocol::{
    comm::enums::{IPCCmd, IPCRes},
    database::DBConnection,
    database_entities::{chat::ChatData, peer::PeerData},
    msg::Msg,
    server_entities::{manager::Manager, master::Master},
};
use std::error::Error;

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let (worker_sender, worker_receiver) = std::sync::mpsc::channel::<IPCCmd>();
    let (msg_sender, msg_receiver) = tokio::sync::broadcast::channel::<IPCRes>(100);
    let mut master = Master::build(None, worker_sender, msg_receiver);
    let mut manager = Manager::create(msg_sender.clone()).await?;
    println!("Starting Manager..");
    manager.init_server()?;
    println!("Manager Started. Establishing Message Routes..");
    manager.setup_slave_communication()?;
    println!("Starting Master...");
    master.setup_communication()?;
    println!("All Set.");
    loop {
        if let Ok(s) = worker_receiver.recv() {
            match s {
                IPCCmd::Tick => {
                    manager.msg_sender.send(IPCRes::Tock)?;
                }
                IPCCmd::Connect(addr, port) => {
                    for _ in 0..5 {
                        match manager.connect_as_dialer((addr.clone(), port)) {
                            Ok(_) => {
                                break;
                            }
                            Err(e) => eprintln!("Cannot connect as Dialer:\n{e}"),
                        }
                    }
                }
                IPCCmd::Text(idx, text) => {
                    let mut peers = manager.peers.lock().unwrap();
                    let Some(target) = peers.get_mut(&idx) else {
                        eprintln!("Could not find peer.");
                        continue;
                    };
                    let encoded = Msg::Text(text).to_vec();
                    target.send(encoded).await?;
                }
                IPCCmd::PeerList => {
                    let peers = manager.dbconn.list_all_peers()?;
                    manager.msg_sender.send(IPCRes::PeerList(peers))?;
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
