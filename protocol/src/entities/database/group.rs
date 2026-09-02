use bincode::{Decode, Encode};
use rusqlite::{Connection, params};
use serde::{Deserialize, Serialize};

#[derive(Debug, Encode, Decode, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DBGroup {
    pub id: u8,
    /// This is the group id that will be used by openmls
    /// Not to be confused with relational `group_id`
    pub group_id: Vec<u8>,
    pub name: String,
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
    fn list_groups(&self) -> Result<Vec<DBGroup>, rusqlite::Error>;
    fn insert_group(&self, group: DBGroup) -> Result<DBGroup, rusqlite::Error>;
    fn delete_group(&self, group_id: u8) -> Result<(), rusqlite::Error>;
    fn get_group_by_group_id(&self, group_id: &[u8]) -> Result<DBGroup, rusqlite::Error>;
    fn get_group_by_idx(&self, idx: u8) -> Result<DBGroup, rusqlite::Error>;
}

impl ConnectionGroup for Connection {
    fn list_groups(&self) -> Result<Vec<DBGroup>, rusqlite::Error> {
        let mut stmt = self.prepare("SELECT * FROM my_group")?;
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

    fn insert_group(&self, group: DBGroup) -> Result<DBGroup, rusqlite::Error> {
        if let Ok(dbgroup) = self.get_group_by_group_id(&group.group_id) {
            return Ok(dbgroup);
        }
        let mut stmt = self.prepare("INSERT INTO my_group (group_id, name) VALUES (?1, ?2)")?;
        let res = match stmt.execute(params![group.group_id, group.name]) {
            Ok(s) if s > 0 => {
                let mut stmt =
                    self.prepare("SELECT * FROM my_group WHERE group_id = ?1 AND name = ?2")?;
                let row = stmt.query_one(params![group.group_id, group.name], |r| {
                    Ok(DBGroup {
                        id: r.get(0)?,
                        group_id: r.get(1)?,
                        name: r.get(2)?,
                    })
                })?;
                Some(row)
            }
            Err(e) => return Err(e),
            _ => None,
        };
        if let Some(res) = res {
            Ok(res)
        } else {
            Err(rusqlite::Error::QueryReturnedNoRows)
        }
    }

    fn delete_group(&self, group_id: u8) -> Result<(), rusqlite::Error> {
        let mut stmt = self.prepare("DELETE FROM my_group WHERE id = ?1")?;
        let res = stmt.execute([group_id])?;
        if res != 1 {
            return Err(rusqlite::Error::QueryReturnedNoRows);
        }
        Ok(())
    }

    fn get_group_by_group_id(&self, group_id: &[u8]) -> Result<DBGroup, rusqlite::Error> {
        println!("getting groups from group id");
        let mut stmt = self.prepare("SELECT * FROM my_group WHERE group_id = ?1")?;
        let row = stmt.query_one(params![group_id], |r| {
            Ok(DBGroup {
                id: r.get("id")?,
                group_id: r.get::<_, Vec<u8>>("group_id")?,
                name: r.get("name")?,
            })
        })?;
        Ok(row)
    }

    fn get_group_by_idx(&self, idx: u8) -> Result<DBGroup, rusqlite::Error> {
        println!("getting groups from idx");
        let mut stmt = self.prepare("SELECT * FROM my_group WHERE id = ?1")?;
        let row = stmt.query_one([idx], |r| {
            Ok(DBGroup {
                id: r.get("id")?,
                group_id: r.get::<_, Vec<u8>>("group_id")?,
                name: r.get("name")?,
            })
        })?;
        Ok(row)
    }
}
