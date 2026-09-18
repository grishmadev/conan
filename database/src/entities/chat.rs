use crate::entities::tuichat::TuiChat;
use bincode::{Decode, Encode};
use chrono::Utc;
use rusqlite::Connection;
use serde::{Deserialize, Serialize};

#[derive(Debug, Decode, Encode, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Chat {
    pub id: u16,
    pub sender_id: u16,
    pub receiver_id: u16,
    pub data: String,
    pub time: String,
}

impl Chat {
    /// Used to build Chat Struct with given parameters
    #[must_use]
    pub fn build(text: &str, sender: u16, receiver: u16) -> Self {
        let time = Utc::now().to_string();
        Self {
            // we're not really adding the id, thats done by sql itself
            id: 0,
            sender_id: sender,
            receiver_id: receiver,
            data: text.into(),
            time,
        }
    }

    #[must_use]
    /// This is for creating Chat Struct that will be sent to peer.
    pub fn chat_to_send(text: &str, rec: u16) -> Self {
        let time = Utc::now().to_string();
        Self {
            id: 0,
            sender_id: 1,
            receiver_id: rec,
            data: text.into(),
            time,
        }
    }

    #[must_use]
    /// This is for creating Chat Struct that we received from peer.
    pub fn chat_to_rec(text: &str, sen: u16) -> Self {
        let time = Utc::now().to_string();
        Self {
            id: 0,
            sender_id: sen,
            receiver_id: 1,
            data: text.into(),
            time,
        }
    }
}

pub trait ChatData {
    /// Lists Chats from Local Database
    /// # Errors
    fn list_all_chat(&self) -> Result<Vec<Chat>, rusqlite::Error>;
    /// Lists chat from a specific peer
    /// # Errors
    fn list_chat_from(&self, peer_id: u16, chat_amount: u8) -> Result<Vec<Chat>, rusqlite::Error>;
    /// List chat from user in `TuiChat` format
    /// # Errors
    fn list_tuichat_from(
        &self,
        peer_id: u16,
        chat_amount: u8,
    ) -> Result<Vec<TuiChat>, rusqlite::Error>;
    /// Inserts Chat to Local Database
    /// # Errors
    fn insert_chat(&self, chat: Chat) -> Result<(), rusqlite::Error>;
    /// Deletes from Local Database based on chat id
    /// # Errors
    fn delete_chat(&self, idx: u16) -> Result<(), rusqlite::Error>;
}

impl ChatData for Connection {
    fn list_all_chat(&self) -> Result<Vec<Chat>, rusqlite::Error> {
        let mut result = vec![];
        let mut query = self.prepare("SELECT * FROM chat")?;
        let rows = query.query_map([], |c| {
            let time: String = c.get(4)?;
            Ok(Chat {
                id: c.get(0)?,
                sender_id: c.get(1)?,
                receiver_id: c.get(2)?,
                data: c.get(3)?,
                time,
            })
        })?;
        for r in rows {
            let r = r?;
            result.push(r);
        }
        Ok(result)
    }

    fn list_chat_from(&self, peer_id: u16, limit: u8) -> Result<Vec<Chat>, rusqlite::Error> {
        let stmt = if peer_id == 1 {
            "SELECT * FROM chat
            WHERE chat.receiver_id = ?1 AND chat.sender_id = ?1
            ORDER BY chat.time DESC
            LIMIT ?2"
        } else {
            "SELECT * FROM chat
            WHERE chat.receiver_id = ?1 OR chat.sender_id = ?1
            ORDER BY chat.time DESC
            LIMIT ?2"
        };
        let mut stmt = self.prepare(stmt)?;
        let rows = stmt.query_map((peer_id, limit), |r| {
            Ok(Chat {
                id: r.get("id")?,
                sender_id: r.get("sender_id")?,
                receiver_id: r.get("receiver_id")?,
                data: r.get("data")?,
                time: r.get("time")?,
            })
        })?;
        let mut result = vec![];
        for r in rows {
            let r = r?;
            result.push(r);
        }
        Ok(result)
    }

    fn list_tuichat_from(&self, peer_id: u16, limit: u8) -> Result<Vec<TuiChat>, rusqlite::Error> {
        let stmt = if peer_id == 1 {
            "SELECT *, 'Me' AS name FROM chat
            WHERE (chat.receiver_id = ?1 AND chat.sender_id = ?1)
            ORDER BY chat.time DESC
            LIMIT ?2"
        } else {
            "SELECT chat.id, peer.name, chat.data, chat.time FROM chat
            LEFT JOIN peer ON chat.sender_id = peer.id
            WHERE (chat.receiver_id = 1 AND chat.sender_id = ?1)
            OR (chat.receiver_id = ?1 AND chat.sender_id = 1)
            ORDER BY chat.time DESC LIMIT ?2"
        };
        let mut stmt = self.prepare(stmt)?;
        let rows = stmt.query_map((peer_id, limit), |r| {
            Ok(TuiChat {
                id: r.get("id")?,
                sender_name: r.get("name")?,
                data: r.get("data")?,
                time: r.get("time")?,
            })
        })?;
        let mut result = vec![];
        for r in rows {
            let r = r?;
            result.push(r);
        }
        Ok(result)
    }

    fn insert_chat(&self, chat: Chat) -> Result<(), rusqlite::Error> {
        let mut stmt =
            self.prepare("INSERT INTO chat (receiver_id, sender_id, data) VALUES (?1, ?2, ?3)")?;
        match stmt.execute((chat.receiver_id, chat.sender_id, chat.data)) {
            Ok(s) => {
                if s == 0 {
                    return Err(rusqlite::Error::QueryReturnedNoRows);
                }
                Ok(())
            }
            Err(e) => Err(e),
        }
    }

    fn delete_chat(&self, id: u16) -> Result<(), rusqlite::Error> {
        let mut stmt = self.prepare("DELETE FROM chat WHERE id = ?1")?;
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
}
