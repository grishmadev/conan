use crate::{
    config::parse_config,
    database::FromConnection,
    extras::{codec::BincodeCodec, mls_provider::ConanMlsProvider},
};
use ed25519_dalek::SigningKey;
use openmls::{
    group::{MlsGroup, MlsGroupCreateConfig, MlsGroupJoinConfig, StagedWelcome},
    prelude::{
        BasicCredential, Ciphersuite, CredentialWithKey, KeyPackage, KeyPackageBundle,
        KeyPackageNewError, LeafNodeIndex, MlsMessageOut, RatchetTreeIn, SignatureScheme, Welcome,
        group_info::GroupInfo,
    },
};
use openmls_basic_credential::SignatureKeyPair;
use openmls_rust_crypto::OpenMlsRustCrypto;
use openmls_sqlite_storage::SqliteStorageProvider;
use rusqlite::Connection;
use std::{collections::HashSet, error::Error};
use tor_llcrypto::pk::ed25519::ExpandedKeypair;

pub struct ConanGroup {
    pub group: MlsGroup,
    /// This `HashSet` contains the idx's of contacts by peers in [`crate::entities::server::manager::Manager`] struct
    pub members: HashSet<u8>,
    pub signer: SignatureKeyPair,
    pub signing_key: SigningKey,
}

impl ConanGroup {
    /// Builds `GroupInfo`
    /// # Errors
    pub fn build(expanded_key: &ExpandedKeypair) -> Result<Self, Box<dyn Error>> {
        let config = parse_config()?;
        let connection = Connection::open(&config.db_path)?;
        let storage: SqliteStorageProvider<BincodeCodec, Connection> =
            SqliteStorageProvider::from_db(&connection)?;
        let secret_key_bytes = expanded_key.to_secret_key_bytes();
        let signing_key = SigningKey::from_bytes(secret_key_bytes[..32].try_into()?);
        let verifying_key = signing_key.verifying_key();
        let public_key = verifying_key.to_bytes();
        let provider = ConanMlsProvider::new(&connection)?;
        let signer: SignatureKeyPair = SignatureKeyPair::from_raw(
            SignatureScheme::ED25519,
            signing_key.as_bytes().to_vec(),
            public_key.to_vec(),
        );
        signer.store(&storage)?;

        let credential_with_key = CredentialWithKey {
            credential: BasicCredential::new(public_key.into()).into(),
            signature_key: signer.public().into(),
        };

        let grp_config = MlsGroupCreateConfig::default();

        let group = MlsGroup::new(&provider, &signer, &grp_config, credential_with_key)?;

        Ok(Self {
            group,
            members: HashSet::new(),
            signer,
            signing_key,
        })
    }

    /// Adds members to the group
    /// # Errors
    pub fn add_members(
        &mut self,
        package: &KeyPackage,
        provider: &ConanMlsProvider,
    ) -> Result<(MlsMessageOut, MlsMessageOut), Box<dyn Error>> {
        let package_slice = std::slice::from_ref(package);

        let res = self
            .group
            .add_members(provider, &self.signer, package_slice)?;

        self.group.merge_pending_commit(provider)?;

        Ok((res.0, res.1))
    }

    /// Removes members from the group
    /// Returns Commit State and Optional Group Info
    /// # Errors
    pub fn remove_members(
        &mut self,
        idx: u32,
        provider: &ConanMlsProvider,
    ) -> Result<(MlsMessageOut, Option<GroupInfo>), Box<dyn Error>> {
        let res = self
            .group
            .remove_members(provider, &self.signer, &[LeafNodeIndex::new(idx)])?;
        self.group.merge_pending_commit(provider)?;
        Ok((res.0, res.2))
    }

    /// Returns `KeyPackageBundle` from Group
    /// # Errors
    pub fn key_package_bundle(
        &self,
        signer: &SignatureKeyPair,
        provider: &ConanMlsProvider,
    ) -> Result<KeyPackageBundle, KeyPackageNewError> {
        let cipher = Ciphersuite::MLS_128_DHKEMX25519_AES128GCM_SHA256_Ed25519;
        let credential_with_key = CredentialWithKey {
            credential: BasicCredential::new(self.signing_key.verifying_key().to_bytes().into())
                .into(),
            signature_key: signer.public().into(),
        };

        KeyPackage::builder().build(cipher, provider, signer, credential_with_key)
    }

    /// Sets Group from `StagedWelcome`
    /// # Errors
    pub fn join_group(welcome: Welcome, tree: RatchetTreeIn) -> Result<MlsGroup, Box<dyn Error>> {
        let provider = OpenMlsRustCrypto::default();
        let staged_join = StagedWelcome::new_from_welcome(
            &provider,
            &MlsGroupJoinConfig::default(),
            welcome,
            Some(tree),
        )?;
        let group = staged_join.into_group(&provider)?;

        Ok(group)
    }
}
