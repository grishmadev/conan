use crate::{
    comm::enums::{from_bytes, to_bytes},
    config::parse_config,
    database::FromConnection,
    extras::{codec::JsonCodec, mls_provider::ConanMlsProvider},
};
use ed25519_dalek::{SigningKey, VerifyingKey};
use openmls::{
    group::{
        CommitMessageBundle, MlsGroup, MlsGroupCreateConfigBuilder, MlsGroupJoinConfig,
        StagedWelcome,
    },
    prelude::{
        BasicCredential, Ciphersuite, CredentialWithKey, KeyPackage, KeyPackageBundle,
        KeyPackageNewError, SignatureScheme, Welcome,
    },
};
use openmls_basic_credential::SignatureKeyPair;
use openmls_sqlite_storage::SqliteStorageProvider;
use rusqlite::Connection;
use std::error::Error;
use thiserror::Error;
use tor_llcrypto::pk::ed25519::ExpandedKeypair;

pub trait ConanGroup {
    /// Get `(SignatureKey, SigningKey, VerifyingKey)` from provided `ExpandedKey`
    /// # Errors
    fn signer_from_expanded_key(
        expanded_key: &ExpandedKeypair,
    ) -> (SignatureKeyPair, SigningKey, VerifyingKey);
    /// Builds `MlsGroup`
    /// # Errors
    fn build(expanded_key: &ExpandedKeypair, self_link: &str) -> Result<MlsGroup, Box<dyn Error>>;
    /// Retrieves the Key Package Bundle
    /// # Errors
    fn key_package_bundle(
        signer: &SignatureKeyPair,
        provider: &ConanMlsProvider,
        self_link: &str,
    ) -> Result<KeyPackageBundle, KeyPackageNewError>;

    /// Joins group from provided Welcome
    /// # Errors
    fn join_group(
        provider: &ConanMlsProvider<JsonCodec>,
        welcome: Welcome,
    ) -> Result<MlsGroup, Box<dyn Error>>;
    /// Gets the list of members of a group in Vector of onion link
    /// # Errors
    fn get_members(&self) -> Result<Vec<String>, Box<dyn Error>>;
}

impl ConanGroup for MlsGroup {
    /// Get Signer, Public and Private key from Expanded Key
    /// # Panics
    fn signer_from_expanded_key(
        expanded_key: &ExpandedKeypair,
    ) -> (SignatureKeyPair, SigningKey, VerifyingKey) {
        let secret_key_bytes = expanded_key.to_secret_key_bytes();
        let signing_key = SigningKey::from_bytes(secret_key_bytes[..32].try_into().unwrap());
        let verifying_key = signing_key.verifying_key();
        let public_key = verifying_key.to_bytes();
        let signer = SignatureKeyPair::from_raw(
            SignatureScheme::ED25519,
            signing_key.as_bytes().to_vec(),
            public_key.to_vec(),
        );
        (signer, signing_key, verifying_key)
    }

    /// Builds `GroupInfo`
    /// # Errors
    fn build(expanded_key: &ExpandedKeypair, self_link: &str) -> Result<Self, Box<dyn Error>> {
        let config = parse_config()?;
        let connection = Connection::open(&config.db_path)?;
        let storage = SqliteStorageProvider::<JsonCodec, _>::from_db(&connection)?;
        let provider = ConanMlsProvider::<JsonCodec>::new(&connection)?;
        let (signer, _, _) = Self::signer_from_expanded_key(expanded_key);
        signer.store(&storage)?;

        let id_bytes = to_bytes(self_link);
        let credential_with_key = CredentialWithKey {
            credential: BasicCredential::new(id_bytes).into(),
            signature_key: signer.public().into(),
        };

        let grp_config = MlsGroupCreateConfigBuilder::default()
            .use_ratchet_tree_extension(true)
            .build();

        let group = MlsGroup::new(&provider, &signer, &grp_config, credential_with_key)?;

        Ok(group)
    }

    // Removes members from the group
    // Returns Commit State and Optional Group Info
    // # Errors
    // TODO: add proper settings to allow other members to decide on kicking a member
    //
    // pub fn remove_members(
    //     &mut self,
    //     idx: u32,
    //     provider: &ConanMlsProvider,
    // ) -> Result<(MlsMessageOut, Option<GroupInfo>), Box<dyn Error>> {
    //     let res = self
    //         .group
    //         .remove_members(provider, &self.signer, &[LeafNodeIndex::new(idx)])?;
    //     self.group.merge_pending_commit(provider)?;
    //     Ok((res.0, res.2))
    // }

    /// Returns `KeyPackageBundle` from Group
    /// # Errors
    fn key_package_bundle(
        signer: &SignatureKeyPair,
        provider: &ConanMlsProvider,
        self_link: &str,
    ) -> Result<KeyPackageBundle, KeyPackageNewError> {
        let cipher = Ciphersuite::MLS_128_DHKEMX25519_AES128GCM_SHA256_Ed25519;
        let id_bytes = to_bytes(self_link);
        let credential_with_key = CredentialWithKey {
            credential: BasicCredential::new(id_bytes).into(),
            signature_key: signer.public().into(),
        };

        KeyPackage::builder().build(cipher, provider, signer, credential_with_key)
    }

    /// Sets Group from `StagedWelcome`
    /// # Errors
    fn join_group(
        provider: &ConanMlsProvider<JsonCodec>,
        welcome: Welcome,
    ) -> Result<MlsGroup, Box<dyn Error>> {
        let staged_join = StagedWelcome::new_from_welcome(
            provider,
            &MlsGroupJoinConfig::default(),
            welcome,
            None,
        )?;
        let group = staged_join.into_group(provider)?;

        Ok(group)
    }

    fn get_members(&self) -> Result<Vec<String>, Box<dyn Error>> {
        let mut result = vec![];
        self.members().for_each(|f| {
            let data = f.credential.serialized_content();
            let onion_str = from_bytes::<String>(data).unwrap();
            result.push(onion_str);
        });
        Ok(result)
    }
}

#[derive(Debug, Error)]
#[non_exhaustive]
pub enum ConanGroupError {
    #[error("Group not found")]
    NotFound,
    #[error("Could not extract group info.")]
    Extraction,
    #[error("{0}")]
    Other(String),
}
