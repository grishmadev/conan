use std::error::Error;

use bincode::{Decode, Encode};
use chrono::{DateTime, Utc};
use rusqlite::Connection;
use serde::{Deserialize, Serialize};

#[derive(Debug, Decode, Encode, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Chat {
    pub id: u32,
    pub sender_id: u32,
    pub receiver_id: u32,
    pub data: String,
    pub time: String,
}

impl Chat {
    /// Used to build Chat Struct with given parameters
    #[must_use]
    pub fn build(text: &str, sender: u32, receiver: u32) -> Self {
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
}

pub trait ChatData {
    /// Lists Chats from Local Database
    /// # Errors
    fn list_all_chat(&self) -> Result<Vec<Chat>, Box<dyn Error>>;
    /// Lists chat from a specific peer
    /// # Errors
    fn list_chat_from(&self, peer_id: u8, chat_amount: u8) -> Result<Vec<Chat>, Box<dyn Error>>;
    /// Inserts Chat to Local Database
    /// # Errors
    fn insert_chat(&self, chat: Chat) -> Result<(), Box<dyn Error>>;
    /// Deletes from Local Database based on chat id
    /// # Errors
    fn delete_chat(&self, idx: u32) -> Result<(), Box<dyn Error>>;
}

impl ChatData for Connection {
    fn list_all_chat(&self) -> Result<Vec<Chat>, Box<dyn Error>> {
        let mut result = vec![];
        let mut query = self.prepare("SELECT * FROM chat")?;
        let rows = query.query_map([], |c| {
            let time: String = c.get(4)?;
            let time = DateTime::parse_from_str(&time, "YYYY-MM-DD HH:MM:SS")
                .unwrap()
                .to_utc()
                .to_string();
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

    fn list_chat_from(&self, peer_id: u8, limit: u8) -> Result<Vec<Chat>, Box<dyn Error>> {
        let mut stmt = self.prepare("SELECT * FROM chat WHERE (chat.receiver_id = ?1 OR chat.sender_id = ?1) ORDER BY chat.time DESC LIMIT ?2")?;
        let rows = stmt.query_map((peer_id, limit), |r| {
            let time: String = r.get(4)?;
            // println!("time: {time:?}");
            // let time = DateTime::parse_from_str(&time, "%Y-%m-%d %H:%M:%S")
            //     .unwrap()
            //     .to_utc()
            //     .to_string();
            Ok(Chat {
                id: r.get(0)?,
                sender_id: r.get(1)?,
                receiver_id: r.get(2)?,
                data: r.get(3)?,
                time,
            })
        })?;
        let mut result = vec![];
        for r in rows {
            let r = r?;
            result.push(r);
        }
        Ok(result)
    }

    fn insert_chat(&self, chat: Chat) -> Result<(), Box<dyn Error>> {
        let mut stmt =
            self.prepare("INSERT INTO chat (receiver_id, sender_id, data) VALUES (?1, ?2, ?3)")?;
        match stmt.execute((chat.receiver_id, chat.sender_id, chat.data)) {
            Ok(s) => {
                if s == 0 {
                    return Err("Nothing was inserted.".into());
                }
                Ok(())
            }
            Err(e) => Err(e.into()),
        }
    }

    fn delete_chat(&self, id: u32) -> Result<(), Box<dyn Error>> {
        let mut stmt = self.prepare("DELETE FROM chat WHERE id = ?1")?;
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
