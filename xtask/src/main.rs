use conanprotocol::{database::DBConnection, extras::generate_name};
use std::error::Error;

// NOTE: This workspace is only for scratchpad codes, testing, trying things out, migrations etc.
fn main() -> Result<(), Box<dyn Error>> {
    let conn = DBConnection::build()?;
    let f = conn.execute("DELETE FROM peer WHERE 1 = 1;")?;
    println!("f: {}", f);
    Ok(())
}
