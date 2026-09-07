use thiserror::Error;

#[derive(Debug, Error)]
pub enum ConanError {
    #[error("Could not connect.")]
    ConnectionError,
    #[error("Cannot be found.")]
    NotFound,
}
