use std::{collections::HashSet, error::Error};

use ed25519_dalek::SigningKey;
use openmls::{
    group::{MlsGroup, MlsGroupCreateConfig, MlsGroupJoinConfig, StagedWelcome},
    prelude::{
        BasicCredential, Ciphersuite, CredentialWithKey, KeyPackage, KeyPackageBundle,
        KeyPackageNewError, LeafNodeIndex, MlsMessageOut, RatchetTreeIn, SignaturePublicKey,
        SignatureScheme, Welcome, group_info::GroupInfo,
    },
};
use openmls_basic_credential::SignatureKeyPair;
use openmls_rust_crypto::OpenMlsRustCrypto;
use openmls_sqlite_storage::SqliteStorageProvider;
use rusqlite::Connection;
use tor_llcrypto::pk::ed25519::ExpandedKeypair;

use crate::{
    config::parse_config,
    entities::server::slave::Slave,
    extras::{codec::BincodeCodec, generate_name},
    msg::{Msg, SlaveCmd},
};

pub struct ConanGroup {
    pub group: MlsGroup,
    /// This `HashSet` contains the idx's of contacts by peers in [`crate::entities::server::manager::Manager`] struct
    pub members: HashSet<u8>,
    pub provider: OpenMlsRustCrypto,
    pub signer: SignatureKeyPair,
    // pub storage: Arc<RwLock<SqliteStorageProvider<BincodeCodec, Connection>>>,
}

impl ConanGroup {
    /// Builds `GroupInfo`
    /// # Errors
    pub fn build(expanded_key: &ExpandedKeypair) -> Result<Self, Box<dyn Error>> {
        let secret_key_bytes = expanded_key.to_secret_key_bytes();
        let signing_key = SigningKey::from_bytes(secret_key_bytes[..32].try_into()?);
        let verifying_key = signing_key.verifying_key();
        let public_key = verifying_key.to_bytes();
        let provider = OpenMlsRustCrypto::default();
        let signer: SignatureKeyPair = SignatureKeyPair::from_raw(
            SignatureScheme::ED25519,
            signing_key.as_bytes().to_vec(),
            public_key.to_vec(),
        );
        let config = parse_config()?;
        let connection = Connection::open(&config.db_path)?;
        let storage: SqliteStorageProvider<BincodeCodec, Connection> =
            SqliteStorageProvider::new(connection);
        // store the signer
        signer.store(&storage)?;

        let credential_with_key = CredentialWithKey {
            credential: BasicCredential::new(public_key.to_vec()).into(),
            signature_key: signer.public().into(),
        };

        let group = MlsGroup::new(
            &provider,
            &signer,
            &MlsGroupCreateConfig::default(),
            credential_with_key,
        )?;

        Ok(Self {
            group,
            members: HashSet::new(),
            provider,
            signer,
            // storage: Arc::new(RwLock::new(storage)),
        })
    }

    /// Adds members to the group
    /// # Errors
    pub fn add_members(
        &mut self,
        package: &KeyPackage,
    ) -> Result<(MlsMessageOut, MlsMessageOut), Box<dyn Error>> {
        let package_slice = std::slice::from_ref(package);

        let res = self
            .group
            .add_members(&self.provider, &self.signer, package_slice)?;

        self.group.merge_pending_commit(&self.provider)?;

        Ok((res.0, res.1))
    }

    /// Removes members from the group
    /// Returns Commit State and Optional Group Info
    /// # Errors
    pub fn remove_members(
        &mut self,
        idx: u32,
    ) -> Result<(MlsMessageOut, Option<GroupInfo>), Box<dyn Error>> {
        let res =
            self.group
                .remove_members(&self.provider, &self.signer, &[LeafNodeIndex::new(idx)])?;
        self.group.merge_pending_commit(&self.provider)?;
        Ok((res.0, res.2))
    }

    /// Returns `KeyPackageBundle` from Group
    /// # Errors
    pub fn key_package_bundle(&self, id: &str) -> Result<KeyPackageBundle, KeyPackageNewError> {
        let cipher = Ciphersuite::MLS_128_DHKEMX25519_AES128GCM_SHA256_Ed25519;
        let credential_with_key = CredentialWithKey {
            credential: BasicCredential::new(id.into()).into(),
            signature_key: self.signer.public().into(),
        };
        KeyPackage::builder().build(cipher, &self.provider, &self.signer, credential_with_key)
    }

    /// Sets Group from `StagedWelcome`
    /// # Errors
    pub fn join_group(
        provider: &OpenMlsRustCrypto,
        welcome: Welcome,
        tree: RatchetTreeIn,
    ) -> Result<MlsGroup, Box<dyn Error>> {
        let staged_join = StagedWelcome::new_from_welcome(
            provider,
            &MlsGroupJoinConfig::default(),
            welcome,
            Some(tree),
        )?;
        let group = staged_join.into_group(provider)?;

        Ok(group)
    }

    pub fn convert_to_group(&self, slave: &mut Slave) -> Result<(), Box<dyn Error>> {
        slave.command_sender.send(SlaveCmd::Msg(Msg::Convert))?;
        Ok(())
    }
}
