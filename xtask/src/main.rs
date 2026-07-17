use conanprotocol::config::parse_config;
use rusqlite::Connection;
use std::error::Error;

// NOTE: This workspace is only for scratchpad codes, testing, trying things out, migrations etc.
/// Not a part of the codebase
#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let dbpath = parse_config()?;
    let conn = Connection::open(dbpath.db_path)?;
    conn.execute("DELETE FROM chat WHERE 1 = 1", ())?;
    Ok(())
}
