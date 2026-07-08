use std::error::Error;

use conanprotocol::config::parse_config;
use rusqlite::Connection;

#[derive(Debug)]
pub struct Person {
    id: i32,
    name: String,
    age: u8,
}
// NOTE: This workspace is only for scratchpad codes, testing, trying things out, migrations etc.
fn main() -> Result<(), Box<dyn Error>> {
    let config = parse_config()?.db_path;
    let handler = Connection::open(config)?;
    handler.execute(
        "
    CREATE TABLE person (
        id INTEGER PRIMARY KEY,
        name TEXT NOT NULL,
        age INTEGER NOT NULL
    )
    ",
        (),
    )?;

    let person = Person {
        id: 1,
        name: "Kishor".to_string(),
        age: 19,
    };

    handler.execute(
        "INSERT INTO PERSON (name, age) VALUES (?1, ?2)",
        (&person.name, &person.age),
    )?;

    let mut stmt = handler.prepare("SELECT id, name, age FROM person")?;

    let person = stmt.query_map([], |r| {
        Ok(Person {
            id: r.get(0)?,
            name: r.get(1)?,
            age: r.get(2)?,
        })
    })?;

    for p in person {
        println!("Res: {:?}", p?);
    }
    Ok(())
}
