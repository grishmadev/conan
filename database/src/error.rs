use thiserror::Error;

#[derive(Debug, Error)]
pub enum DatabaseError {
    #[error("Error in Sqlite Library: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("Not Found")]
    NotFound,
}
