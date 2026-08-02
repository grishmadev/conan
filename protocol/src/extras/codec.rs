use serde::{Serialize, de::DeserializeOwned};
use std::io;

#[derive(Default, Clone, Copy, Debug)]
pub struct BincodeCodec;

impl openmls_sqlite_storage::Codec for BincodeCodec {
    type Error = io::Error;

    fn to_vec<T: Serialize>(value: &T) -> Result<Vec<u8>, Self::Error> {
        bincode::serde::encode_to_vec(value, bincode::config::standard())
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
    }

    fn from_slice<T: DeserializeOwned>(slice: &[u8]) -> Result<T, Self::Error> {
        let (data, _) =
            bincode::serde::decode_from_slice::<T, _>(slice, bincode::config::standard())
                .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        Ok(data)
    }
}
