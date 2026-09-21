use crate::comm::enums::error::ConanError;
use crate::constants::MLS_ADMIN_ID;
use crate::extras::mls_provider::ConanMlsProvider;
use crate::{
    comm::{from_bytes, to_bytes},
    config::parse_config,
};
use database::ConnectionClone;
use database::entities::peer::Peer;
use database::{FromConnection, rusqlite::Connection};
use ed25519_dalek::{SigningKey, VerifyingKey};
use extras::codec::JsonCodec;
use openmls::prelude::{Credential, Extension, Extensions, MlsMessageOut, UnknownExtension};
use openmls::{
    group::{MlsGroup, MlsGroupCreateConfigBuilder, MlsGroupJoinConfig, StagedWelcome},
    prelude::{
        BasicCredential, Ciphersuite, CredentialWithKey, KeyPackage, KeyPackageBundle,
        KeyPackageNewError, SignatureScheme, Welcome,
    },
};
use openmls_basic_credential::SignatureKeyPair;
use openmls_sqlite_storage::SqliteStorageProvider;
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
    /// Promote a member to admin and return a Commit
    /// # Errors
    fn promote_to_admin(
        &mut self,
        peer: &Peer,
        provider: &ConanMlsProvider,
        signer: &SignatureKeyPair,
    ) -> Result<MlsMessageOut, Box<dyn Error>>;
    /// Demote an admin to member and return a Commit
    /// # Errors
    fn demote_to_member(
        &mut self,
        peer: &Peer,
        provider: &ConanMlsProvider,
        signer: &SignatureKeyPair,
    ) -> Result<MlsMessageOut, Box<dyn Error>>;
    /// Adds member if called by an admin
    /// # Errors
    fn add_member(
        &mut self,
        provider: &ConanMlsProvider,
        signer: &SignatureKeyPair,
        key_package: &KeyPackage,
    ) -> Result<(MlsMessageOut, MlsMessageOut), Box<dyn Error>>;
    /// Removes members from the group
    /// Returns Commit State
    /// # Errors
    fn remove_member(
        &mut self,
        peer: &Peer,
        provider: &ConanMlsProvider,
        signer: &SignatureKeyPair,
    ) -> Result<MlsMessageOut, Box<dyn Error>>;
    /// List Admins of this group
    /// # Errors
    fn admins(&self) -> Result<Vec<String>, ConanGroupError>;
    /// Returns whether a peer is admin or not
    /// # Errors
    fn is_admin(&self, address: &str) -> Result<bool, ConanGroupError>;
    /// Returns whether self is admin or not
    /// # Errors
    fn am_i_admin(&self) -> Result<(), ConanGroupError>;
    /// Returns `KeyPackageBundle` from Group
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
        let openmls_store = connection.openmls();
        let provider = ConanMlsProvider::<JsonCodec>::new(&openmls_store)?;
        let openmls_store = SqliteStorageProvider::<JsonCodec, _>::from_db(&openmls_store)?;
        let (signer, _, _) = Self::signer_from_expanded_key(expanded_key);
        signer.store(&openmls_store)?;

        let id_bytes = to_bytes(self_link)?;
        let credential_with_key = CredentialWithKey {
            credential: BasicCredential::new(id_bytes).into(),
            signature_key: signer.public().into(),
        };

        let admins = vec![self_link.to_string()];
        let admins_ser = to_bytes(&admins)?;
        let mut extensions = Extensions::empty();
        extensions.add(Extension::Unknown(
            MLS_ADMIN_ID,
            UnknownExtension(admins_ser),
        ))?;
        let grp_config = MlsGroupCreateConfigBuilder::default()
            .use_ratchet_tree_extension(true)
            .with_group_context_extensions(extensions)
            .build();

        let group = MlsGroup::new(&provider, &signer, &grp_config, credential_with_key)?;

        Ok(group)
    }

    fn promote_to_admin(
        &mut self,
        peer: &Peer,
        provider: &ConanMlsProvider,
        signer: &SignatureKeyPair,
    ) -> Result<MlsMessageOut, Box<dyn Error>> {
        // NOTE: Now, don't be a smart ass and edit this line to bypass admin privilege
        // Every Commit is verified by every member
        // And they WILL find out if you pass an illegal action in the group
        self.am_i_admin()?;
        let mut admins = self.admins()?;
        admins.push(peer.address.clone());
        let mut extensions = Extensions::empty();
        extensions.add(Extension::Unknown(
            MLS_ADMIN_ID,
            UnknownExtension(to_bytes(admins)?),
        ))?;
        let (commit, _, _) = self.update_group_context_extensions(provider, extensions, signer)?;
        Ok(commit)
    }

    fn demote_to_member(
        &mut self,
        peer: &Peer,
        provider: &ConanMlsProvider,
        signer: &SignatureKeyPair,
    ) -> Result<MlsMessageOut, Box<dyn Error>> {
        self.am_i_admin()?;
        let mut admins = self.admins()?;
        admins.retain(|x| x != &peer.address);
        let mut extensions = Extensions::empty();
        extensions.add(Extension::Unknown(
            MLS_ADMIN_ID,
            UnknownExtension(to_bytes(admins)?),
        ))?;
        let (commit, _, _) = self.update_group_context_extensions(provider, extensions, signer)?;
        Ok(commit)
    }

    fn add_member(
        &mut self,
        provider: &ConanMlsProvider,
        signer: &SignatureKeyPair,
        key_package: &KeyPackage,
    ) -> Result<(MlsMessageOut, MlsMessageOut), Box<dyn Error>> {
        self.am_i_admin()?;
        let key_packages = core::slice::from_ref(key_package);
        let (commit, welcome, _) = self.add_members(provider, signer, key_packages)?;
        Ok((commit, welcome))
    }

    fn remove_member(
        &mut self,
        peer: &Peer,
        provider: &ConanMlsProvider,
        signer: &SignatureKeyPair,
    ) -> Result<MlsMessageOut, Box<dyn Error>> {
        self.am_i_admin()?;
        let id_bytes = to_bytes(peer.address.clone())?;
        let leaf_idx = self
            .member_leaf_index(&BasicCredential::new(id_bytes).into())
            .ok_or(ConanError::NotFound)?;
        let leaf_idx_slice = std::slice::from_ref(&leaf_idx);
        let (commit, _, _) = self.remove_members(provider, signer, leaf_idx_slice)?;
        self.merge_pending_commit(provider)?;
        Ok(commit)
    }

    fn admins(&self) -> Result<Vec<String>, ConanGroupError> {
        let Some(UnknownExtension(data)) = self.extensions().unknown(MLS_ADMIN_ID) else {
            return Err(ConanGroupError::AdminsNotFound);
        };
        let Ok(data_des) = serde_json::from_slice::<Vec<String>>(data) else {
            return Err(ConanGroupError::Extraction);
        };
        Ok(data_des)
    }

    fn is_admin(&self, addr: &str) -> Result<bool, ConanGroupError> {
        let admins = self.admins()?;
        if !admins.contains(&addr.into()) {
            return Ok(false);
        }
        Ok(true)
    }

    fn am_i_admin(&self) -> Result<(), ConanGroupError> {
        let d = self
            .member_at(self.own_leaf_index())
            .ok_or(ConanGroupError::NotFound)?;
        let cred = d.credential.serialized_content();
        let addr = from_bytes::<String>(cred).unwrap();
        if !self.is_admin(&addr)? {
            return Err(ConanGroupError::NotAdmin);
        }
        Ok(())
    }

    fn key_package_bundle(
        signer: &SignatureKeyPair,
        provider: &ConanMlsProvider,
        self_link: &str,
    ) -> Result<KeyPackageBundle, KeyPackageNewError> {
        let cipher = Ciphersuite::MLS_128_DHKEMX25519_AES128GCM_SHA256_Ed25519;
        let id_bytes = to_bytes(self_link).unwrap();
        let credential_with_key = CredentialWithKey {
            credential: BasicCredential::new(id_bytes).into(),
            signature_key: signer.public().into(),
        };

        KeyPackage::builder().build(cipher, provider, signer, credential_with_key)
    }

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

pub trait Addr {
    fn to_addr(&self) -> String;
}

impl Addr for Credential {
    fn to_addr(&self) -> String {
        let data = self.serialized_content();
        from_bytes::<String>(data).unwrap()
    }
}

#[derive(Debug, Error)]
#[non_exhaustive]
pub enum ConanGroupError {
    #[error("Admin Data not found")]
    AdminsNotFound,
    #[error("Not Admin")]
    NotAdmin,
    #[error("Group not found")]
    NotFound,
    #[error("Could not extract group info.")]
    Extraction,
    #[error("{0}")]
    Other(String),
}
