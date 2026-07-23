use std::error::Error;

use openmls::{
    group::{MlsGroup, MlsGroupCreateConfig, MlsGroupJoinConfig, StagedWelcome},
    prelude::{
        BasicCredential, Ciphersuite, CredentialWithKey, KeyPackage, KeyPackageBundle,
        KeyPackageNewError, MlsMessageOut, OpenMlsProvider, RatchetTreeIn, SignatureScheme,
        Welcome, group_info::GroupInfo,
    },
};
use openmls_basic_credential::SignatureKeyPair;
use openmls_rust_crypto::OpenMlsRustCrypto;

pub struct ConanGroup {
    pub group: MlsGroup,
    pub provider: OpenMlsRustCrypto,
    pub signer: SignatureKeyPair,
}

impl ConanGroup {
    /// Builds `GroupInfo`
    /// # Errors
    pub fn build(id: &str) -> Result<Self, Box<dyn Error>> {
        let provider = OpenMlsRustCrypto::default();
        let signer = SignatureKeyPair::new(SignatureScheme::ED25519)?;

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
    pub fn welcome_to_group(
        &mut self,
        welcome: Welcome,
        ratchet_tree: Option<RatchetTreeIn>,
    ) -> Result<(), Box<dyn Error>> {
        let staged_join = StagedWelcome::new_from_welcome(
            &self.provider,
            &MlsGroupJoinConfig::default(),
            welcome,
            ratchet_tree,
        )?;
        let group = staged_join.into_group(&self.provider)?;
        self.group = group;
        Ok(())
    }
}
