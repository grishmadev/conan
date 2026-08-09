use conanprotocol::{
    comm::enums::{IPCCmd, IPCRes},
    config::parse_config,
    entities::{
        database::{
            chat::{Chat, ChatData},
            group::{ConnectionGroup, DBGroup},
            peer::PeerData,
        },
        server::{manager::Manager, master::Master},
    },
    extras::generate_name,
    mls::ConanGroup,
    msg::{Msg, SlaveCmd},
    operations::signing_key,
};
use std::{
    error::Error,
    sync::{Arc, atomic::Ordering},
};

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let config = parse_config()?;
    let (worker_sender, worker_receiver) = std::sync::mpsc::channel::<IPCCmd>();
    let (msg_sender, msg_receiver) = tokio::sync::broadcast::channel::<IPCRes>(100);
    let mut master = Master::build(None, worker_sender, msg_receiver);
    let signing_key = signing_key(config.arti_key_store.clone()).await?;
    println!("Starting Master...");
    master.setup_communication(&config)?;
    let mut manager = Manager::create(msg_sender.clone(), config.clone()).await?;
    println!("Starting Manager..");
    manager.init_server()?;
    println!("Manager Started. Establishing Message Routes..");
    manager.setup_slave_communication()?;
    println!("All Set.");
    loop {
        if let Ok(s) = worker_receiver.recv() {
            match s {
                IPCCmd::Tick => {
                    manager.msg_sender.send(IPCRes::Tock)?;
                }
                IPCCmd::StartServer => {
                    let started = manager.server_ready.load(Ordering::SeqCst);
                    msg_sender.send(IPCRes::ServerStarted(started))?;
                }
                IPCCmd::Connect(addr, port) => {
                    if let Err(e) = manager.connect_as_dialer((addr.clone(), port)) {
                        return Err(format!("Cannot connect as Dialer:\n{e}").into());
                    }
                }
                IPCCmd::Text(idx, text) => {
                    if idx == 1 {
                        let chat = Chat::chat_to_send(&text, 1);
                        manager.dbconn.insert_chat(chat)?;
                        manager.msg_sender.send(IPCRes::Text(idx, text))?;
                        continue;
                    }
                    let peers = Arc::clone(&manager.peers);
                    let mut peers = peers.write().unwrap();
                    let Some(target) = peers.get_mut(&idx) else {
                        println!("Cannot find target peer.");
                        continue;
                    };
                    let chat = Chat::chat_to_send(&text, u32::from(idx));
                    manager.dbconn.insert_chat(chat)?;
                    let msg = Msg::Text(text);
                    target.command_sender.send(SlaveCmd::Msg(msg)).unwrap();
                }
                IPCCmd::PeerList => {
                    let mut peers = manager.dbconn.list_all_peers(true)?;
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
                IPCCmd::DeleteGroup(idx) => {
                    #[allow(clippy::cast_possible_truncation)]
                    let idx = idx as u8;
                    manager.dbconn.delete_group(idx)?;
                    manager.groups.write().unwrap().remove(&idx);
                    manager
                        .msg_sender
                        .send(IPCRes::DeletedGroup(u32::from(idx)))?;
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
                IPCCmd::GroupList => {
                    let groups = manager.dbconn.list_groups()?;
                    manager.msg_sender.send(IPCRes::GroupList(groups))?;
                }

                IPCCmd::NewGroup(name) => {
                    let new_group = ConanGroup::build(&signing_key)?;
                    let name = if let Some(name) = name {
                        name
                    } else {
                        generate_name(3..8)
                    };
                    let dbgroup = DBGroup::new(new_group.group.group_id().to_vec(), name);
                    // let groups = Arc::clone(&manager.groups);
                    // let mut groups = groups.write().unwrap();
                    manager.dbconn.insert_group(dbgroup)?;
                    // groups.insert(group.id, new_group);
                }

                IPCCmd::AddToGroup(group_idx, peer_idx) => {
                    _ = manager.make_peer_join_group(peer_idx, group_idx).await;
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
