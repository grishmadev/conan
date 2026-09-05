use crate::entities::database::group::ConnectionGroup;
use bincode::{Decode, Encode};
use rusqlite::{Connection, params};
use serde::{Deserialize, Serialize};

#[derive(Debug, Encode, Decode, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GroupChat {
    pub id: u16,
    pub group_id: u16,
    pub sender_id: u16,
    pub data: String,
    pub time: String,
}

pub trait ConnectionGroupChat {
    /// Lists all the chats in all the group chats
    /// # Errors
    fn list_all_group_chat(&self) -> Result<Vec<GroupChat>, rusqlite::Error>;
    /// Use this function to get chats for groups by database group indexes
    /// # Errors
    fn get_chats_by_group_idx(
        &self,
        idx: u16,
        limit: u8,
    ) -> Result<Vec<GroupChat>, rusqlite::Error>;
    /// Use this function to get chats for groups by openmls group ids
    /// # Errors
    fn get_chats_by_group_id(
        &self,
        id: Vec<u8>,
        limit: u8,
    ) -> Result<Vec<GroupChat>, rusqlite::Error>;
    /// Inserts group chat into database
    /// # Errors
    fn insert_group_chat(&self, chat: GroupChat) -> Result<GroupChat, rusqlite::Error>;
}

impl ConnectionGroupChat for Connection {
    fn list_all_group_chat(&self) -> Result<Vec<GroupChat>, rusqlite::Error> {
        let mut stmt = self.prepare("SELECT * FROM group_chat ORDER BY time DESC")?;
        let rows = stmt.query_map([], |r| {
            Ok(GroupChat {
                id: r.get(0)?,
                group_id: r.get(1)?,
                sender_id: r.get(2)?,
                data: r.get(3)?,
                time: r.get(4)?,
            })
        })?;
        let mut result = vec![];
        for r in rows {
            let r = r?;
            result.push(r);
        }
        Ok(result)
    }
    fn get_chats_by_group_idx(
        &self,
        idx: u16,
        limit: u8,
    ) -> Result<Vec<GroupChat>, rusqlite::Error> {
        let mut stmt = self
            .prepare("SELECT * FROM group_chat WHERE group_id = ?1 ORDER BY time DESC LIMIT ?2")?;
        let rows = stmt.query_map(params![idx, limit], |r| {
            Ok(GroupChat {
                id: r.get(0)?,
                group_id: r.get(1)?,
                sender_id: r.get(2)?,
                data: r.get(3)?,
                time: r.get(4)?,
            })
        })?;
        let mut result = vec![];
        for r in rows {
            let r = r?;
            result.push(r);
        }
        Ok(result)
    }

    fn get_chats_by_group_id(
        &self,
        id: Vec<u8>,
        limit: u8,
    ) -> Result<Vec<GroupChat>, rusqlite::Error> {
        let group = self.get_group_by_group_id(&id)?;
        let group_idx = group.id;
        let gc = self.get_chats_by_group_idx(group_idx, limit)?;
        Ok(gc)
    }

    fn insert_group_chat(&self, chat: GroupChat) -> Result<GroupChat, rusqlite::Error> {
        println!("inserting chat");
        let mut stmt =
            self.prepare("INSERT INTO group_chat (group_id, sender_id, data) VALUES (?1, ?2, ?3) RETURNING id, group_id, sender_id, data, time")?;
        let row = stmt.query_one(params![&chat.group_id, chat.sender_id, &chat.data], |r| {
            Ok(GroupChat {
                id: r.get(0)?,
                group_id: r.get(1)?,
                sender_id: r.get(2)?,
                data: r.get(3)?,
                time: r.get(4)?,
            })
        })?;
        Ok(row)
    }
}
