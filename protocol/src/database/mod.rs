use rusqlite::Connection;

use crate::config::parse_config;

pub mod setup;
pub struct DBConnection {
    pub connection: Connection,
}

impl DBConnection {
    /// Used to build a Connection thread to local sqlite Database
    ///
    /// # Errors
    /// Might Error due to io error or from rusqlite crate
    pub fn build() -> Result<Self, Box<dyn std::error::Error>> {
        let config = parse_config()?.db_path;
        Ok(Self {
            connection: Connection::open(config)?,
        })
    }

    pub fn execute(&self, query: &str) -> Result<usize, rusqlite::Error> {
        self.connection.execute(query, ())
    }
}
