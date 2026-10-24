use conandatabase::error::DatabaseError;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ConanError {
    #[error("Could not connect.")]
    ConnectionError,
    #[error("{0} Cannot be found.")]
    NotFound(String),
    #[error("Error while parsing")]
    ParseError,
    #[error("Error in Database")]
    Database(#[from] DatabaseError),
    #[error("Session Locked. Try again later.")]
    Locked,
    #[error("Other: {0:?}")]
    Other(String),
}
