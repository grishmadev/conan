use conanprotocol::config::parse_config;
use database::entities::group_chat::ConnectionGroupChat;
use rusqlite::Connection;
use std::error::Error;

// NOTE: This workspace is only for scratchpad codes, testing, trying things out, migrations etc.
/// Not a part of the codebase
#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let config = parse_config()?;
    let conn = Connection::open(config.db_path)?;
    let chats = conn.list_all_group_chat()?;
    println!("chats: {chats:#?}");
    // conn.execute("DELETE FROM group_chat ", [])?;
    Ok(())
}
