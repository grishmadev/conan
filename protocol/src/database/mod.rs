pub(crate) mod migration;
use openmls_sqlite_storage::SqliteStorageProvider;
use rusqlite::Connection;

use crate::{database::migration::run_is_friend_migration, extras::codec::BincodeCodec};

pub trait ConnectionClone {
    fn try_clone(&self) -> Result<Connection, Box<dyn std::error::Error>>;
}

impl ConnectionClone for Connection {
    fn try_clone(&self) -> Result<Self, Box<dyn std::error::Error>> {
        let path = self.path().ok_or("Cannot get Database path")?;
        let conn = Connection::open(path)?;
        Ok(conn)
    }
}

pub trait FromConnection {
    fn from_db(
        value: &Connection,
    ) -> Result<SqliteStorageProvider<BincodeCodec, Connection>, Box<dyn std::error::Error>>;
}

impl FromConnection for SqliteStorageProvider<BincodeCodec, Connection> {
    fn from_db(value: &Connection) -> Result<Self, Box<dyn std::error::Error>> {
        let conn = value.try_clone()?;
        let conn = Self::new(conn);
        Ok(conn)
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
        sender_id INTEGER NOT NULL REFERENCES peer(id),
        receiver_id INTEGER NOT NULL REFERENCES peer(id),
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
        group_id INTEGER NOT NULL REFERENCES my_group(id),
        sender_id INTEGER NOT NULL REFERENCES group_peer(id),
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
            peer_id INTEGER NOT NULL REFERENCES peer(id),
            group_id INTEGER NOT NULL REFERENCES my_group(id)
        );",
        (),
    )?;

    run_is_friend_migration(&conn)?;

    let mut storage: SqliteStorageProvider<BincodeCodec, Connection> =
        SqliteStorageProvider::new(conn);
    storage.run_migrations()?;
    Ok(())
}
