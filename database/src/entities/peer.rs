use bincode::{Decode, Encode};
use rusqlite::{Connection, OptionalExtension};
use serde::{Deserialize, Serialize};

#[derive(Debug, Decode, Encode, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct Peer {
    pub id: u16,
    pub name: String,
    pub address: String,
    pub connected: bool,
    pub is_friend: bool,
}

impl Peer {
    /// Builds a Peer struct with given parameters
    #[must_use]
    pub fn build(name: &str, address: &str, is_friend: bool) -> Self {
        let address = address.into();
        let name = name.into();
        Self {
            id: 0,
            name,
            address,
            is_friend,
            connected: false,
        }
    }
}

pub trait PeerData {
    /// Lists peers from local database, `only_friends` filters out non-friends
    /// # Errors
    fn list_all_peers(&self, only_friends: bool) -> Result<Vec<Peer>, rusqlite::Error>;
    /// Retrieves a peer by its onion address if it exists
    /// # Errors
    fn get_peer_from_addr(&self, addr: &str) -> Result<Option<Peer>, rusqlite::Error>;
    /// Retrieves a peer by its local database id
    /// # Errors
    fn get_peer_from_id(&self, id: u16) -> Result<Option<Peer>, rusqlite::Error>;
    /// Inserts a new peer into local database
    /// # Errors
    fn insert_peer(&self, peer: Peer) -> Result<Peer, rusqlite::Error>;
    /// Deletes a peer from local database by id
    /// # Errors
    fn delete_peer(&self, id: u16) -> Result<(), rusqlite::Error>;
    /// Renames a peer in local database by id
    /// # Errors
    fn rename_peer(&self, id: u16, new_name: String) -> Result<(), rusqlite::Error>;
}

impl PeerData for Connection {
    fn list_all_peers(&self, only_friends: bool) -> Result<Vec<Peer>, rusqlite::Error> {
        let mut result = vec![];
        let query = if only_friends {
            "SELECT * FROM peer WHERE is_friend = TRUE"
        } else {
            "SELECT * FROM peer"
        };
        let mut query = self.prepare(query)?;
        let rows = query.query_map([], |p| {
            Ok(Peer {
                id: p.get(0)?,
                name: p.get(1)?,
                address: p.get(2)?,
                is_friend: p.get(3)?,
                connected: false,
            })
        })?;
        for r in rows {
            let r = r?;
            result.push(r);
        }
        Ok(result)
    }

    fn get_peer_from_addr(&self, addr: &str) -> Result<Option<Peer>, rusqlite::Error> {
        let mut stmt = self.prepare("SELECT * FROM peer WHERE address = ?1")?;
        let peer = stmt
            .query_row([&addr], |r| {
                Ok(Peer {
                    id: r.get(0)?,
                    name: r.get(1)?,
                    address: r.get(2)?,
                    is_friend: r.get(3)?,
                    connected: false,
                })
            })
            .optional()?;
        Ok(peer)
    }

    fn get_peer_from_id(&self, id: u16) -> Result<Option<Peer>, rusqlite::Error> {
        let mut stmt = self.prepare("SELECT * FROM peer WHERE id = ?1")?;
        let peer = stmt
            .query_row([id], |r| {
                Ok(Peer {
                    id: r.get(0)?,
                    name: r.get(1)?,
                    address: r.get(2)?,
                    is_friend: r.get(3)?,
                    connected: false,
                })
            })
            .optional()?;
        Ok(peer)
    }

    fn insert_peer(&self, peer: Peer) -> Result<Peer, rusqlite::Error> {
        self.prepare(
            "
            INSERT INTO peer (name, address, is_friend)
            VALUES (?1, ?2, ?3)
            RETURNING id, name, address, is_friend
        ",
        )?
        .query_one((&peer.name, &peer.address, &peer.is_friend), |r| {
            Ok(Peer {
                id: r.get(0)?,
                name: r.get(1)?,
                address: r.get(2)?,
                is_friend: r.get(3)?,
                connected: false,
            })
        })
    }

    fn delete_peer(&self, id: u16) -> Result<(), rusqlite::Error> {
        let mut stmt = self.prepare("DELETE FROM peer WHERE id = ?1")?;
        match stmt.execute([id]) {
            Ok(s) => {
                if s == 0 {
                    return Err(rusqlite::Error::QueryReturnedNoRows);
                }
                Ok(())
            }
            Err(e) => Err(e),
        }
    }

    fn rename_peer(&self, id: u16, new_name: String) -> Result<(), rusqlite::Error> {
        let mut stmt = self.prepare("UPDATE peer SET name = ?1 WHERE id = ?2")?;
        if stmt.execute((new_name, id))? == 0 {
            return Err(rusqlite::Error::QueryReturnedNoRows);
        }
        Ok(())
    }
}
