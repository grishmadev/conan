use conanprotocol::{
    comm::enums::{IPCCmd, IPCRes},
    config::parse_config,
    entities::{
        database::{
            chat::{Chat, ChatData},
            group::{ConnectionGroup, DBGroup},
            group_chat::ConnectionGroupChat,
            peer::PeerData,
        },
        server::{manager::Manager, master::Master},
    },
    extras::generate_name,
    mls::ConanGroup,
    msg::{Msg, SlaveCmd},
    operations::signing_key,
};
use openmls::group::MlsGroup;
use std::{
    error::Error,
    sync::{Arc, atomic::Ordering},
};

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    color_eyre::install()?;
    let config = parse_config()?;
    let (worker_sender, worker_receiver) = std::sync::mpsc::channel::<IPCCmd>();
    let (msg_sender, msg_receiver) = tokio::sync::broadcast::channel::<IPCRes>(100);
    let mut master = Master::build(None, worker_sender.clone(), msg_receiver);
    println!("Starting Master...");
    master.setup_communication(&config)?;
    let mut manager = Manager::create(msg_sender.clone(), config.clone(), worker_sender).await?;
    println!("Starting Manager..");
    manager.init_server()?;
    println!("Manager Started. Establishing Message Routes..");
    manager.setup_slave_communication()?;
    println!("All Set.");
    let signing_key = signing_key().await?;
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
                    println!("addr to connect: {addr}:{port}");
                    if let Err(e) = manager.connect_as_dialer(addr, port, false) {
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
                    let chat = Chat::chat_to_send(&text, idx);
                    manager.dbconn.insert_chat(chat)?;
                    let msg = Msg::Text(text);
                    target.command_sender.send(SlaveCmd::Msg(msg)).unwrap();
                }
                IPCCmd::Disconnect(idx) => {
                    if idx == 1 {
                        continue;
                    }
                    let mut peers = manager.peers.write().unwrap();
                    let Some(target) = peers.get_mut(&idx) else {
                        continue;
                    };
                    target.command_sender.send(SlaveCmd::Shutdown)?;
                }
                IPCCmd::PeerList => {
                    let mut peers = manager.dbconn.list_all_peers(true)?;
                    if let Ok(mem_slaves) = Arc::clone(&manager.peers).read() {
                        let iter = peers.iter_mut();
                        #[allow(clippy::cast_possible_truncation)]
                        for p in iter {
                            p.connected = mem_slaves.contains_key(&p.id);
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
                    manager.dbconn.delete_group(idx)?;
                    manager.groups.write().unwrap().remove(&idx);
                    manager.msg_sender.send(IPCRes::DeletedGroup(idx))?;
                }
                IPCCmd::RenamePeer(idx, new_name) => {
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

                IPCCmd::NewGroup(name) => {
                    let self_link = manager
                        .dbconn
                        .get_peer_from_id(1)?
                        .ok_or("Cannot find self link")?
                        .address;
                    let new_group = MlsGroup::build(&signing_key, &self_link)?;
                    let name = if let Some(name) = name {
                        name
                    } else {
                        generate_name(3..8)
                    };
                    let dbgroup = DBGroup::new(new_group.group_id().to_vec(), name);
                    manager.dbconn.insert_group(dbgroup)?;
                }

                IPCCmd::AddToGroup(group_idx, peer_idx) => {
                    if let Err(err) = manager.make_peer_join_group(peer_idx, group_idx) {
                        eprintln!("Cannot Join. {err}");
                    }
                }

                IPCCmd::GroupConnect(idx) => {
                    let mut group = manager.get_mls_group_from_idx(idx)?;
                    manager.connect_to_group(&mut group)?;
                    manager.groups.write().unwrap().insert(idx, group);
                }

                IPCCmd::InitiateGroup(grp_id) => {
                    println!("initiating group from this side");
                    let dbgroup = manager.dbconn.get_group_by_group_id(&grp_id)?;
                    let mut mlsgrp = manager.get_mls_group_from_idx(dbgroup.id)?;
                    manager.connect_to_group(&mut mlsgrp)?;
                    manager.groups.write().unwrap().insert(dbgroup.id, mlsgrp);
                }

                IPCCmd::GroupList => {
                    let mut groups = manager.dbconn.list_groups()?;
                    if let Ok(grouplist) = Arc::clone(&manager.groups).read() {
                        let groups = groups.iter_mut();
                        for g in groups {
                            if grouplist.contains_key(&g.id) {
                                g.connected = true;
                            }
                        }
                    }
                    manager.msg_sender.send(IPCRes::GroupList(groups))?;
                }

                IPCCmd::GroupChatList {
                    group_idx,
                    msg_amount,
                } => {
                    let chats = manager
                        .dbconn
                        .get_chats_by_group_idx(group_idx, msg_amount)?;
                    let mut res = vec![];
                    for c in chats {
                        let chat = Chat {
                            id: c.id,
                            sender_id: c.sender_id,
                            receiver_id: 1,
                            data: c.data,
                            time: c.time,
                        };
                        res.push(chat);
                    }
                    manager.msg_sender.send(IPCRes::GroupChatList {
                        group_idx,
                        chats: res,
                    })?;
                }

                IPCCmd::GroupText(grp_idx, text) => {
                    println!("Group Chat to send");
                    let mut groups = manager.groups.write().unwrap();
                    groups.iter().for_each(|g| {
                        println!("group idx: {}", g.0);
                    });
                    println!("target group idx: {grp_idx}");
                    let Some(group) = groups.get_mut(&grp_idx) else {
                        continue;
                    };
                    manager.send_msg(group, &text)?;
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
