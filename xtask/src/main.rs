use conanprotocol::{config::parse_config, database::DBConnection};
use std::error::Error;

// NOTE: This workspace is only for scratchpad codes, testing, trying things out, migrations etc.
fn main() -> Result<(), Box<dyn Error>> {
    let config = parse_config()?;
    let conn = DBConnection::build(&config.db_path)?;
    let f = conn.execute("DELETE FROM peer WHERE 1 = 1;")?;
    println!("f: {f}");
    Ok(())
}
