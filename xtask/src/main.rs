use conandatabase::entities::group::ConnectionGroup;
use conanprotocol::config::parse_config;
use rusqlite::Connection;
use std::error::Error;

// NOTE: This workspace is only for scratchpad codes, testing, trying things out, migrations etc.
/// Not a part of the codebase
#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let config = parse_config()?;
    let conn = Connection::open(config.db_path)?;
    let groups = conn.list_groups()?;
    println!("groups: {groups:#?}");
    Ok(())
}
