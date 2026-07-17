use rusqlite::Connection;
pub mod setup;
pub struct DBConnection {
    pub connection: Connection,
}

impl DBConnection {
    /// Used to build a Connection thread to local sqlite Database
    ///
    /// # Errors
    /// Might Error due to io error or from rusqlite crate
    pub fn build(path: &str) -> Result<Self, Box<dyn std::error::Error>> {
        Ok(Self {
            connection: Connection::open(path)?,
        })
    }

    /// Directly Executes SQL commands
    /// # Errors
    pub fn execute(&self, query: &str) -> Result<usize, rusqlite::Error> {
        self.connection.execute(query, ())
    }
}
