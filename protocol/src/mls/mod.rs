use std::error::Error;

use openmls::{
    group::{MlsGroup, MlsGroupCreateConfig, MlsGroupJoinConfig, StagedWelcome},
    prelude::{
        BasicCredential, Ciphersuite, CredentialWithKey, KeyPackage, KeyPackageBundle,
        KeyPackageNewError, LeafNodeIndex, MlsMessageOut, SignatureScheme, Welcome,
        group_info::GroupInfo,
    },
};
use openmls_basic_credential::SignatureKeyPair;
use openmls_rust_crypto::{MemoryStorage, OpenMlsRustCrypto};

use crate::{
    entities::server::slave::Slave,
    msg::{Msg, SlaveCmd},
};

#[derive(Debug)]
pub struct ConanGroup {
    pub group: MlsGroup,
    pub provider: OpenMlsRustCrypto,
    pub signer: SignatureKeyPair,
    pub storage: MemoryStorage,
}

impl ConanGroup {
    /// Builds `GroupInfo`
    /// # Errors
    pub fn build(id: &str) -> Result<Self, Box<dyn Error>> {
        let provider = OpenMlsRustCrypto::default();
        let signer = SignatureKeyPair::new(SignatureScheme::ED25519)?;
        let storage = MemoryStorage::default();
        signer.store(&storage)?;

        // store the signer
        let credential_with_key = CredentialWithKey {
            credential: BasicCredential::new(id.into()).into(),
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
            provider,
            signer,
            storage,
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
        idx: LeafNodeIndex,
    ) -> Result<(MlsMessageOut, Option<GroupInfo>), Box<dyn Error>> {
        let res = self
            .group
            .remove_members(&self.provider, &self.signer, &[idx])?;
        self.group.merge_pending_commit(&self.provider)?;
        Ok((res.0, res.2))
    }

    /// Returns `KeyPackageBundle` from Group
    /// # Errors
    pub fn key_package(&self, id: &str) -> Result<KeyPackageBundle, KeyPackageNewError> {
        let cipher = Ciphersuite::MLS_128_DHKEMX25519_AES128GCM_SHA256_Ed25519;
        let credential_with_key = CredentialWithKey {
            credential: BasicCredential::new(id.into()).into(),
            signature_key: self.signer.public().into(),
        };
        KeyPackage::builder().build(cipher, &self.provider, &self.signer, credential_with_key)
    }

    /// Sets Group from `StagedWelcome`
    /// # Errors
    pub fn join_group(welcome: Welcome) -> Result<Self, Box<dyn Error>> {
        let provider = OpenMlsRustCrypto::default();
        let staged_join = StagedWelcome::new_from_welcome(
            &provider,
            &MlsGroupJoinConfig::default(),
            welcome,
            None,
        )?;
        let group = staged_join.into_group(&provider)?;

        Ok(Self {
            group,
            provider,
            signer: SignatureKeyPair::new(SignatureScheme::ED25519)?,
            storage: MemoryStorage::default(),
        })
    }

    pub fn convert_to_group(&self, slave: &mut Slave) -> Result<(), Box<dyn Error>> {
        slave.command_sender.send(SlaveCmd::Msg(Msg::Convert))?;
        Ok(())
    }
}
