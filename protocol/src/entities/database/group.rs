use crate::entities::database::peer::{Peer, PeerData};
use bincode::{Decode, Encode};
use rusqlite::{Connection, params};
use serde::{Deserialize, Serialize};
use std::error::Error;

#[derive(Debug, Encode, Decode, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DBGroup {
    pub id: u8,
    /// This is the group id that will be used by openmls
    /// Not to be confused with relational `group_id`
    pub group_id: Vec<u8>,
    pub name: String,
}
#[derive(Debug, Encode, Decode, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GroupChat {
    pub id: u8,
    pub group_id: u8,
    pub sender_id: u8,
    pub data: String,
    pub time: String,
}

pub struct GroupToPeer {
    pub id: u8,
    pub peer_id: u8,
    pub group_id: u8,
}

impl DBGroup {
    #[must_use]
    pub fn new(group_id: Vec<u8>, name: String) -> Self {
        Self {
            id: 0,
            group_id,
            name,
        }
    }
}

pub trait ConnectionGroup {
    fn list_groups(&self) -> Result<Vec<DBGroup>, Box<dyn Error>>;
    fn list_members(&self, id: u8) -> Result<Vec<Peer>, Box<dyn Error>>;
    fn insert_group(&self, group: DBGroup) -> Result<DBGroup, Box<dyn Error>>;
    fn insert_member(&self, group_idx: u8, member: Peer) -> Result<Peer, Box<dyn Error>>;
}

impl ConnectionGroup for Connection {
    fn list_groups(&self) -> Result<Vec<DBGroup>, Box<dyn Error>> {
        let mut stmt = self.prepare("SELECT * FROM group")?;
        let rows = stmt.query_map([], |r| {
            Ok(DBGroup {
                id: r.get(0)?,
                group_id: r.get(1)?,
                name: r.get(2)?,
            })
        })?;
        let mut result = vec![];
        for r in rows {
            let r = r?;
            result.push(r);
        }
        Ok(result)
    }

    fn list_members(&self, id: u8) -> Result<Vec<Peer>, Box<dyn Error>> {
        let mut stmt = self.prepare("SELECT * FROM group_to_peer WHERE group_id = ?1")?;
        let rows = stmt.query_map([id], |r| {
            Ok(Peer {
                id: r.get(0)?,
                name: r.get(1)?,
                address: r.get(2)?,
                is_friend: r.get(3)?,
                connected: false,
            })
        })?;
        let mut result = vec![];
        for r in rows {
            let r = r?;
            result.push(r);
        }

        Ok(result)
    }

    fn insert_group(&self, group: DBGroup) -> Result<DBGroup, Box<dyn Error>> {
        let mut stmt = self.prepare("INSERT INTO group (group_id, name) VALUES (?1, ?2)")?;
        let res = match stmt.execute(params![group.group_id, group.name]) {
            Ok(s) if s > 0 => {
                let mut stmt =
                    self.prepare("SELECT * FROM group WHERE group_id = ?1 AND name = ?2")?;
                let row = stmt.query_one(params![group.group_id, group.name], |r| {
                    Ok(DBGroup {
                        id: r.get(0)?,
                        group_id: r.get(1)?,
                        name: r.get(2)?,
                    })
                })?;
                Some(row)
            }
            Err(e) => return Err(e.into()),
            _ => None,
        };
        if let Some(res) = res {
            Ok(res)
        } else {
            Err("Could not insert group".into())
        }
    }

    fn insert_member(&self, group_idx: u8, member: Peer) -> Result<Peer, Box<dyn Error>> {
        let peer = self.insert_peer(member)?;
        let mut stmt =
            self.prepare("INSERT INTO group_to_peer (group_id, peer_id) VALUES (?1, ?2)")?;
        let res = stmt.execute(params![group_idx, peer.id])?;
        if res == 1 {
            Ok(peer)
        } else {
            Err("Could not insert to Database".into())
        }
    }
}
