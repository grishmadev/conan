use bincode::Encode;
use bincode::config;
use serde::Serialize;
use serde::de::DeserializeOwned;

pub mod enums;
pub mod notification;

/// # Panics
pub fn to_bytes<T>(msg: T) -> Result<Vec<u8>, bincode::error::EncodeError>
where
    T: Serialize,
{
    let res = bincode::serde::encode_to_vec(msg, config::standard())?;
    Ok(res)
}

/// # Panics
pub fn encode<T>(msg: T) -> Vec<u8>
where
    T: Encode,
{
    bincode::encode_to_vec(msg, config::standard()).unwrap()
}
/// # Errors
pub fn from_bytes<T>(bytes: &[u8]) -> Result<T, bincode::error::DecodeError>
where
    T: DeserializeOwned,
{
    let d = bincode::serde::decode_from_slice::<T, _>(bytes, config::standard())?;
    Ok(d.0)
}
