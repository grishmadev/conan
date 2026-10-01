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
    /// Lists group members along with if they're admin
    /// # Errors
    fn list_members(&self, id: u16) -> Result<Vec<(Peer, bool)>, rusqlite::Error>;
    /// Insert a member in a group
    /// # Errors
    fn insert_member(
        &self,
        group_idx: u16,
        member: Peer,
        known: bool,
        is_admin: bool,
    ) -> Result<Peer, rusqlite::Error>;
    /// List the groups that a member is part of
    /// # Errors
    fn list_groups_with_member(&self, peer_id: u16) -> Result<Vec<DBGroup>, rusqlite::Error>;
    /// Remove a member from a group in database
    /// # Errors
    fn remove_member(&self, peer_id: u16, group_id: u16) -> Result<(), rusqlite::Error>;
    /// Promote a member to admin
    /// # Errors
    fn promote_member(&self, peer_id: u16) -> Result<(), rusqlite::Error>;
    /// Demote an admin to member
    /// # Errors
    fn demote_member(&self, peer_id: u16) -> Result<(), rusqlite::Error>;
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
        mut member: Peer,
        known: bool,
        is_admin: bool,
    ) -> Result<Peer, rusqlite::Error> {
        if !known {
            member = self.insert_peer(member)?;
        }
        let mut stmt = self.prepare(
            "INSERT INTO group_to_peer (group_id, peer_id, is_admin) VALUES (?1, ?2, ?3)",
        )?;
        let res = stmt.execute(params![group_idx, member.id, is_admin])?;
        if res == 1 {
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

    fn demote_member(&self, peer_id: u16) -> Result<(), rusqlite::Error> {
        let mut stmt =
            self.prepare("UPDATE group_to_peer SET is_admin = FALSE WHERE peer_id = ?1")?;
        stmt.execute(params![peer_id])?;
        Ok(())
    }

    fn promote_member(&self, peer_id: u16) -> Result<(), rusqlite::Error> {
        let mut stmt =
            self.prepare("UPDATE group_to_peer SET is_admin = TRUE WHERE peer_id = ?1")?;
        stmt.execute(params![peer_id])?;
        Ok(())
    }
}
