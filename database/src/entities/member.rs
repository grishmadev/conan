use rusqlite::{Connection, params};

use crate::entities::{
    group::{ConnectionGroup, DBGroup},
    peer::{Peer, PeerData},
};

pub struct GroupToPeer {
    pub id: u16,
    pub peer_id: u16,
    pub group_id: u16,
    pub is_admin: bool,
}

pub trait GroupMember {
    /// Lists members of a group with their admin status
    /// # Errors
    fn list_members(&self, id: u16) -> Result<Vec<(Peer, bool)>, rusqlite::Error>;
    /// Inserts or replaces a member in a group, registering unknown peers first
    /// # Errors
    fn insert_member(
        &self,
        group_idx: u16,
        member: Peer,
        is_admin: bool,
    ) -> Result<Peer, rusqlite::Error>;
    /// Lists all groups a specific peer is a member of
    /// # Errors
    fn list_groups_with_member(&self, peer_id: u16) -> Result<Vec<DBGroup>, rusqlite::Error>;
    /// Removes a member from a group in local database
    /// # Errors
    fn remove_member(&self, peer_id: u16, group_id: u16) -> Result<(), rusqlite::Error>;
    /// Promotes a member to admin status in a group
    /// # Errors
    fn promote_member(&self, peer_id: u16, group_id: u16) -> Result<(), rusqlite::Error>;
    /// Demotes an admin to normal member status in a group
    /// # Errors
    fn demote_member(&self, peer_id: u16, group_id: u16) -> Result<(), rusqlite::Error>;
}

impl GroupMember for Connection {
    fn list_members(&self, group_id: u16) -> Result<Vec<(Peer, bool)>, rusqlite::Error> {
        let mut stmt = self.prepare(
            "
            SELECT
            p.id,
            p.name,
            p.address,
            p.is_friend,
            g.is_admin
            FROM peer p
            JOIN group_to_peer g ON p.id = g.peer_id
            WHERE g.group_id = ?1",
        )?;

        let rows = stmt.query_map([group_id], |r| {
            Ok((
                Peer {
                    id: r.get("id")?,
                    name: r.get("name")?,
                    address: r.get("address")?,
                    is_friend: r.get("is_friend")?,
                    connected: false,
                },
                r.get("is_admin")?,
            ))
        })?;

        rows.collect()
    }

    fn list_groups_with_member(&self, peer_id: u16) -> Result<Vec<DBGroup>, rusqlite::Error> {
        let mut stmt = self.prepare("SELECT * FROM group_to_peer WHERE peer_id = ?1")?;
        let rows = stmt.query_map([peer_id], |r| r.get::<_, u16>("group_id"))?;
        let mut result = vec![];
        for r in rows {
            let r = r?;
            let group = self.get_group_by_idx(r)?;
            result.push(group);
        }
        Ok(result)
    }

    fn insert_member(
        &self,
        group_idx: u16,
        member: Peer,
        is_admin: bool,
    ) -> Result<Peer, rusqlite::Error> {
        // check whether the provided peer is already in the database
        let member = match self.get_peer_from_addr(&member.address)? {
            // already known: reuse the stored row so its id is used
            Some(peer) => peer,
            // unknown: add it as a peer first
            None => self.insert_peer(member)?,
        };
        // `INSERT OR REPLACE` keeps a single membership row per (group, peer)
        let res: usize = self.execute(
            "INSERT OR REPLACE INTO group_to_peer (group_id, peer_id, is_admin) VALUES (?1, ?2, ?3)",
            params![group_idx, member.id, is_admin],
        )?;
        if res >= 1 {
            Ok(member)
        } else {
            Err(rusqlite::Error::QueryReturnedNoRows)
        }
    }

    fn remove_member(&self, peer_id: u16, group_id: u16) -> Result<(), rusqlite::Error> {
        let mut stmt =
            self.prepare("DELETE FROM group_to_peer WHERE peer_id = ?1 AND group_id = ?2")?;
        stmt.execute(params![peer_id, group_id])?;
        Ok(())
    }

    fn demote_member(&self, peer_id: u16, group_id: u16) -> Result<(), rusqlite::Error> {
        let mut stmt = self.prepare(
            "UPDATE group_to_peer SET is_admin = FALSE WHERE peer_id = ?1 AND group_id = ?2",
        )?;
        if stmt.execute(params![peer_id, group_id])? == 0 {
            return Err(rusqlite::Error::QueryReturnedNoRows);
        }
        Ok(())
    }

    fn promote_member(&self, peer_id: u16, group_id: u16) -> Result<(), rusqlite::Error> {
        let mut stmt = self.prepare(
            "UPDATE group_to_peer SET is_admin = TRUE WHERE peer_id = ?1 AND group_id = ?2",
        )?;
        if stmt.execute(params![peer_id, group_id])? == 0 {
            return Err(rusqlite::Error::QueryReturnedNoRows);
        }
        Ok(())
    }
}
