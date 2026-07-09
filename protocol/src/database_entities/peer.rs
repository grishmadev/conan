use std::error::Error;

use bincode::{Decode, Encode};

use crate::database::DBConnection;

#[derive(Debug, Decode, Encode, Clone, PartialEq, Eq)]
pub struct Peer {
    pub id: u32,
    pub name: Option<String>,
    pub address: String,
}

impl Peer {
    /// Used to build Peer Struct with given parameters
    #[must_use]
    pub fn build(name: &str, address: &str) -> Self {
        let name = if name.is_empty() {
            None
        } else {
            Some(name.to_string())
        };
        let address = address.to_string();
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
    /// Inserts peers to Local Database
    /// # Errors
    fn insert_peer(&self, peer: Peer) -> Result<(), Box<dyn Error>>;
    /// Deletes from Local Database based on peer id
    /// # Errors
    fn delete_peer(&self, id: u32) -> Result<(), Box<dyn Error>>;
}

impl PeerData for DBConnection {
    fn list_all_peers(&self) -> Result<Vec<Peer>, Box<dyn Error>> {
        let mut result = vec![];
        let mut query = self.connection.prepare("SELECT * FROM peer")?;
        let rows = query.query_map([], |p| {
            Ok(Peer {
                id: p.get(0)?,
                name: p.get(1).ok(),
                address: p.get(2)?,
            })
        })?;
        for r in rows {
            let r = r?;
            result.push(r);
        }
        Ok(result)
    }

    fn insert_peer(&self, peer: Peer) -> Result<(), Box<dyn Error>> {
        let mut stmt = self
            .connection
            .prepare("INSERT INTO peer (name, address) VALUES (?1, ?2)")?;
        match stmt.execute((peer.name, peer.address)) {
            Ok(s) => {
                if s == 0 {
                    return Err("Nothing was inserted.".into());
                }
                Ok(())
            }
            Err(e) => Err(e.into()),
        }
    }

    fn delete_peer(&self, id: u32) -> Result<(), Box<dyn Error>> {
        let mut stmt = self.connection.prepare("DELETE FROM peer WHERE id = ?1")?;
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
