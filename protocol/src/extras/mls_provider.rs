use openmls::prelude::OpenMlsProvider;
use openmls_rust_crypto::OpenMlsRustCrypto;
use openmls_sqlite_storage::{Codec, SqliteStorageProvider};
use rusqlite::Connection;

use crate::{database::FromConnection, extras::codec::JsonCodec};

pub struct ConanMlsProvider<C = JsonCodec>
where
    C: Codec,
{
    crypto: OpenMlsRustCrypto,
    storage: SqliteStorageProvider<C, Connection>,
}

impl<C: Codec> ConanMlsProvider<C> {
    /// # Errors
    pub fn new(conn: &Connection) -> Result<Self, Box<dyn std::error::Error>> {
        let storage = SqliteStorageProvider::from_db(conn)?;
        let crypto = OpenMlsRustCrypto::default();
        Ok(Self { crypto, storage })
    }

    pub fn storage(&self) -> &SqliteStorageProvider<C, Connection> {
        &self.storage
    }
}

impl<C: Codec> OpenMlsProvider for ConanMlsProvider<C> {
    type CryptoProvider = <OpenMlsRustCrypto as OpenMlsProvider>::CryptoProvider;
    type StorageProvider = SqliteStorageProvider<C, Connection>;
    type RandProvider = <OpenMlsRustCrypto as openmls::prelude::OpenMlsProvider>::RandProvider;

    fn crypto(&self) -> &Self::CryptoProvider {
        self.crypto.crypto()
    }

    fn storage(&self) -> &Self::StorageProvider {
        &self.storage
    }

    fn rand(&self) -> &Self::RandProvider {
        self.crypto.rand()
    }
}
