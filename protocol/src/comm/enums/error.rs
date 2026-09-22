use conandatabase::error::DatabaseError;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ConanError {
    #[error("Could not connect.")]
    ConnectionError,
    #[error("Cannot be found.")]
    NotFound,
    #[error("Error while parsing")]
    ParseError,
    #[error("Error in Database")]
    Database(#[from] DatabaseError),
    #[error("Session Locked. Try again later.")]
    Locked,
    #[error("Other: {0:?}")]
    Other(String),
}
