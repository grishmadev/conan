pub(crate) mod migration;
use std::fs;

use openmls_sqlite_storage::{Codec, SqliteStorageProvider};
use rusqlite::Connection;

use crate::{database::migration::run_is_friend_migration, extras::codec::JsonCodec};

pub trait ConnectionClone {
    fn try_clone(&self) -> Result<Connection, Box<dyn std::error::Error>>;
    fn get_openmls_path(&self) -> String;
}

impl ConnectionClone for Connection {
    fn try_clone(&self) -> Result<Self, Box<dyn std::error::Error>> {
        let path = self.path().ok_or("Cannot get Database path")?;
        let conn = Connection::open(path)?;
        Ok(conn)
    }

    fn get_openmls_path(&self) -> String {
        let mut path = self
            .path()
            .unwrap()
            .split('/')
            .map(|s| s.to_string())
            .collect::<Vec<_>>();
        path.pop();
        path.push("openmls".into());
        let path = path.join("/");
        if !fs::exists(&path).unwrap() {
            fs::File::create(&path).unwrap();
        }
        path
    }
}

pub trait FromConnection<C: Codec> {
    fn from_db(
        conn: &Connection,
    ) -> Result<SqliteStorageProvider<C, Connection>, Box<dyn std::error::Error>>;
}

impl<C: Codec> FromConnection<C> for SqliteStorageProvider<C, Connection>
where
    C: Codec,
{
    fn from_db(
        conn: &Connection,
    ) -> Result<SqliteStorageProvider<C, Connection>, Box<dyn std::error::Error>> {
        // Get the DB path from the connection
        let path = conn.path().ok_or("Cannot get path from connection")?;

        // Open a new connection to the same file
        let new_conn = Connection::open(path)?;
        let mut storage = SqliteStorageProvider::<C, Connection>::new(new_conn);
        storage.run_migrations()?;
        Ok(storage)
    }
}

/// # Errors
pub fn setup_db(db_path: &str) -> Result<(), Box<dyn std::error::Error>> {
    let conn = Connection::open(db_path)?;
    conn.execute("PRAGMA foreign_keys = ON;", ())?;

    // create peers
    conn.execute(
        "
        CREATE TABLE IF NOT EXISTS peer (
        id INTEGER PRIMARY KEY AUTOINCREMENT,
        name TEXT,
        address TEXT CHECK(address LIKE '%.onion' AND LENGTH(address) = 62),
        is_friend BOOLEAN DEFAULT FALSE
        );
                ",
        (),
    )?;

    // create chats
    conn.execute(
        "
        CREATE TABLE IF NOT EXISTS chat (
        id INTEGER PRIMARY KEY AUTOINCREMENT,
        sender_id INTEGER NOT NULL REFERENCES peer(id) ON DELETE CASCADE,
        receiver_id INTEGER NOT NULL REFERENCES peer(id) ON DELETE NO ACTION,
        data TEXT NOT NULL,
        time DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP
        );",
        (),
    )?;

    //create groups
    conn.execute(
        "
        CREATE TABLE IF NOT EXISTS my_group (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            group_id BLOB NOT NULL,
            name TEXT NOT NULL,
            created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP
        );",
        (),
    )?;

    // create group chats
    conn.execute(
        "
    CREATE TABLE IF NOT EXISTS group_chat (
        id INTEGER PRIMARY KEY AUTOINCREMENT,
        group_id INTEGER NOT NULL REFERENCES my_group(id) ON DELETE CASCADE,
        sender_id INTEGER NOT NULL REFERENCES peer(id) ON DELETE NO ACTION,
        data TEXT NOT NULL,
        time DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP
    );",
        (),
    )?;

    // create group_to_peer
    conn.execute(
        "
        CREATE TABLE IF NOT EXISTS group_to_peer (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            peer_id INTEGER NOT NULL REFERENCES peer(id) ON DELETE CASCADE,
            group_id INTEGER NOT NULL REFERENCES my_group(id) ON DELETE CASCADE
        );",
        (),
    )?;

    run_is_friend_migration(&conn)?;

    let mut storage = SqliteStorageProvider::<JsonCodec, _>::from_db(&conn)?;
    storage.run_migrations()?;
    Ok(())
}
