use std::error::Error;

use rusqlite::{Connection, params};

use crate::entities::database::{
    group::{ConnectionGroup, DBGroup},
    peer::{Peer, PeerData},
};

pub struct GroupToPeer {
    pub id: u16,
    pub peer_id: u16,
    pub group_id: u16,
}

pub trait GroupMember {
    fn list_members(&self, id: u8) -> Result<Vec<Peer>, Box<dyn Error>>;
    fn insert_member(
        &self,
        group_idx: u8,
        member: Peer,
        known: bool,
    ) -> Result<Peer, Box<dyn Error>>;
    fn list_groups_with_member(&self, peer_id: u16) -> Result<Vec<DBGroup>, Box<dyn Error>>;
    fn remove_member(&self, peer_id: u32, group_id: u32) -> Result<(), Box<dyn Error>>;
}

impl GroupMember for Connection {
    fn list_members(&self, group_id: u8) -> Result<Vec<Peer>, Box<dyn Error>> {
        let mut stmt = self.prepare("SELECT * FROM group_to_peer WHERE group_id = ?1")?;
        let rows = stmt.query_map([group_id], |r| {
            Ok(GroupToPeer {
                id: r.get("id")?,
                peer_id: r.get("peer_id")?,
                group_id: r.get("group_id")?,
            })
        })?;
        let mut result = vec![];
        for r in rows {
            let r = r?;
            let peer = self.get_peer_from_id(r.peer_id)?;
            if let Some(peer) = peer {
                result.push(peer);
            }
        }

        Ok(result)
    }

    fn list_groups_with_member(&self, peer_id: u16) -> Result<Vec<DBGroup>, Box<dyn Error>> {
        let mut stmt = self.prepare("SELECT * FROM group_to_peer WHERE peer_id = ?1")?;
        let rows = stmt.query_map([peer_id], |r| r.get::<_, u8>("group_id"))?;
        let mut result = vec![];
        for r in rows {
            let r = r?;
            let group = self.get_group_by_idx(r as u8)?;
            result.push(group);
        }
        Ok(result)
    }

    fn insert_member(
        &self,
        group_idx: u8,
        mut member: Peer,
        known: bool,
    ) -> Result<Peer, Box<dyn Error>> {
        if !known {
            member = self.insert_peer(member)?;
        }
        let mut stmt =
            self.prepare("INSERT INTO group_to_peer (group_id, peer_id) VALUES (?1, ?2)")?;
        let res = stmt.execute(params![group_idx, member.id])?;
        if res == 1 {
            Ok(member)
        } else {
            Err("Could not insert to Database".into())
        }
    }

    fn remove_member(&self, peer_id: u32, group_id: u32) -> Result<(), Box<dyn Error>> {
        let mut stmt =
            self.prepare("DELETE FROM group_to_peer WHERE peer_id = ?1 AND group_id = ?2")?;
        stmt.execute(params![peer_id, group_id])?;
        Ok(())
    }
}
