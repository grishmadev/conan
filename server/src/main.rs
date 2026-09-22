use conan_chat_server::ServerHandler;
use conanprotocol::{
    comm::enums::{ipccmd::IPCCmd, ipcres::IPCRes},
    config::parse_config,
    entities::{manager::Manager, master::Master},
    operations::signing_key,
};
use std::error::Error;

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    color_eyre::install()?;
    let config = parse_config()?;
    let (worker_sender, worker_receiver) = std::sync::mpsc::channel::<IPCCmd>();
    let (mut master, msg_sender) = Master::build(None, worker_sender.clone());
    println!("Starting Master...");
    master.setup_communication(&config)?;
    let signing_key = signing_key().await?;
    let manager = Manager::create(msg_sender.clone(), config.clone(), worker_sender).await?;
    let mut handler = ServerHandler::setup(manager, signing_key)?;
    println!("All Set.");
    loop {
        if let Ok(cmd) = worker_receiver.recv() {
            handler.handle_commands(cmd)?;
        } else {
            msg_sender.send(IPCRes::Error(
                "Could not parse or reply to message.".to_string(),
            ))?;
        }
    }
}
