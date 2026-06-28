use conanprotocol::PeerConnection;
use std::error::Error;

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let mut peer_connection = PeerConnection::create().await?;
    peer_connection.init_server().await?;
    Ok(())
}
