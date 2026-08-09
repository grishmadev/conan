use openmls::prelude::OpenMlsProvider;
use openmls_rust_crypto::OpenMlsRustCrypto;
use openmls_sqlite_storage::SqliteStorageProvider;
use rusqlite::Connection;

use crate::{database::FromConnection, extras::codec::BincodeCodec};

pub struct ConanMlsProvider {
    crypto: OpenMlsRustCrypto,
    storage: SqliteStorageProvider<BincodeCodec, Connection>,
}

impl ConanMlsProvider {
    pub fn new(conn: &Connection) -> Result<Self, Box<dyn std::error::Error>> {
        let storage = SqliteStorageProvider::from_db(conn)?;
        let crypto = OpenMlsRustCrypto::default();
        Ok(Self { storage, crypto })
    }

    pub fn storage(&self) -> &SqliteStorageProvider<BincodeCodec, Connection> {
        &self.storage
    }
}

impl OpenMlsProvider for ConanMlsProvider {
    type CryptoProvider = <OpenMlsRustCrypto as OpenMlsProvider>::CryptoProvider;
    type StorageProvider = SqliteStorageProvider<BincodeCodec, Connection>;
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
