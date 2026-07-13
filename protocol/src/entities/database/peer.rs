use std::error::Error;

use bincode::{Decode, Encode};
use rusqlite::Connection;
use serde::{Deserialize, Serialize};

#[derive(Debug, Decode, Encode, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct Peer {
    pub id: u32,
    pub name: String,
    pub address: String,
}

impl Peer {
    /// Used to build Peer Struct with given parameters
    #[must_use]
    pub fn build(name: &str, address: &str) -> Self {
        let address = address.into();
        let name = name.into();
        Self {
            id: 0,
            name,
            address,
        }
    }
}

pub trait PeerData {
    /// Lists Peers from Local Database
    /// # Errors
    fn list_all_peers(&self) -> Result<Vec<Peer>, Box<dyn Error>>;
    /// Get's name of Peer if exists else None
    /// # Errors
    fn get_peer_from_addr(&self, addr: &str) -> Result<Option<Peer>, Box<dyn Error>>;
    fn get_peer_from_id(&self, id: u32) -> Result<Option<Peer>, Box<dyn Error>>;
    /// Inserts peers to Local Database
    /// # Errors
    fn insert_peer(&self, peer: Peer) -> Result<Peer, Box<dyn Error>>;
    /// Deletes from Local Database based on peer id
    /// # Errors
    fn delete_peer(&self, id: u32) -> Result<(), Box<dyn Error>>;
}

impl PeerData for Connection {
    fn list_all_peers(&self) -> Result<Vec<Peer>, Box<dyn Error>> {
        let mut result = vec![];
        let mut query = self.prepare("SELECT * FROM peer")?;
        let rows = query.query_map([], |p| {
            Ok(Peer {
                id: p.get(0)?,
                name: p.get(1)?,
                address: p.get(2)?,
            })
        })?;
        for r in rows {
            let r = r?;
            result.push(r);
        }
        Ok(result)
    }

    fn get_peer_from_addr(&self, addr: &str) -> Result<Option<Peer>, Box<dyn Error>> {
        let mut stmt = self.prepare("SELECT * FROM peer WHERE address = ?1")?;
        let result = stmt.query_row([&addr], |r| {
            Ok(Peer {
                id: r.get(0)?,
                name: r.get(1)?,
                address: r.get(2)?,
            })
        });
        let peer = match result {
            Ok(s) => Some(s),
            Err(e) => {
                eprintln!("Could not get peer: {e}");
                // possible that multiple rows are present.
                self.execute("DELETE FROM peer WHERE address = ?1", [&addr])?;
                None
            }
        };
        Ok(peer)
    }

    fn get_peer_from_id(&self, id: u32) -> Result<Option<Peer>, Box<dyn Error>> {
        let mut stmt = self.prepare("SELECT * FROM peer WHERE id = ?1")?;
        let result = stmt.query_row([&id], |r| {
            Ok(Peer {
                id: r.get(0)?,
                name: r.get(1)?,
                address: r.get(2)?,
            })
        });
        let peer = match result {
            Ok(s) => Some(s),
            Err(e) => {
                eprintln!("Could not get peer: {e}");
                // possible that multiple rows are present.
                self.execute("DELETE FROM peer WHERE id = ?1", [&id])?;
                None
            }
        };
        Ok(peer)
    }

    fn insert_peer(&self, peer: Peer) -> Result<Peer, Box<dyn Error>> {
        let mut stmt = self.prepare("INSERT INTO peer (name, address) VALUES (?1, ?2)")?;
        match stmt.execute((&peer.name, &peer.address)) {
            Ok(s) => {
                if s == 0 {
                    return Err("Nothing was inserted.".into());
                }
                let stmt = self
                    .prepare("SELECT * FROM peer WHERE name = ?1 AND address = ?2")?
                    .query_one((&peer.name, &peer.address), |r| {
                        Ok(Peer {
                            id: r.get(0)?,
                            name: r.get(1)?,
                            address: r.get(2)?,
                        })
                    });
                Ok(stmt?)
            }
            Err(e) => Err(e.into()),
        }
    }

    fn delete_peer(&self, id: u32) -> Result<(), Box<dyn Error>> {
        let mut stmt = self.prepare("DELETE FROM peer WHERE id = ?1")?;
        match stmt.execute([id]) {
            Ok(s) => {
                if s == 0 {
                    return Err("Nothing was inserted.".into());
                }
                Ok(())
            }
            Err(e) => Err(e.into()),
        }
    }
}
