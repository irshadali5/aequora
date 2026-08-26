use crate::{
    CryptoBuildError, CryptoPolicy, CryptoRequirement, EncryptionKeyProvider, KeyProvider,
};
use std::sync::Arc;

/// Validated provider-neutral cryptographic composition for one application boundary.
pub struct AequoraCrypto {
    policy: CryptoPolicy,
    signing: Option<Arc<dyn KeyProvider>>,
    encryption: Option<Arc<dyn EncryptionKeyProvider>>,
}

impl AequoraCrypto {
    #[must_use]
    pub fn builder() -> AequoraCryptoBuilder {
        AequoraCryptoBuilder::default()
    }

    #[must_use]
    pub const fn policy(&self) -> &CryptoPolicy {
        &self.policy
    }

    #[must_use]
    pub fn signing_provider(&self) -> Option<&dyn KeyProvider> {
        self.signing.as_deref()
    }

    #[must_use]
    pub fn encryption_provider(&self) -> Option<&dyn EncryptionKeyProvider> {
        self.encryption.as_deref()
    }
}

#[derive(Default)]
pub struct AequoraCryptoBuilder {
    policy: Option<CryptoPolicy>,
    signing: Option<Arc<dyn KeyProvider>>,
    encryption: Option<Arc<dyn EncryptionKeyProvider>>,
}

impl AequoraCryptoBuilder {
    #[must_use]
    pub fn policy(mut self, policy: CryptoPolicy) -> Self {
        self.policy = Some(policy);
        self
    }

    #[must_use]
    pub fn key_provider<P: KeyProvider + 'static>(mut self, provider: P) -> Self {
        self.signing = Some(Arc::new(provider));
        self
    }

    #[must_use]
    pub fn encryption_provider<P: EncryptionKeyProvider + 'static>(mut self, provider: P) -> Self {
        self.encryption = Some(Arc::new(provider));
        self
    }

    /// Validates policy/provider composition.
    ///
    /// # Errors
    ///
    /// Returns [`CryptoBuildError`] for invalid policy or a missing required signer.
    pub fn build(self) -> Result<AequoraCrypto, CryptoBuildError> {
        let policy = self.policy.unwrap_or_default();
        policy
            .validate()
            .map_err(|_| CryptoBuildError::InvalidPolicy)?;
        let signing_required = [
            policy.signed_snapshots,
            policy.signed_exports,
            policy.signed_audit_checkpoints,
            policy.device_operation_signatures,
        ]
        .contains(&CryptoRequirement::Required);
        if signing_required && self.signing.is_none() {
            return Err(CryptoBuildError::SigningProviderRequired);
        }
        let encryption_required = [
            policy.encrypted_snapshots,
            policy.encrypted_exports,
            policy.encrypted_replay_bundles,
            policy.tenant_data_encryption,
        ]
        .contains(&CryptoRequirement::Required);
        if encryption_required && self.encryption.is_none() {
            return Err(CryptoBuildError::EncryptionProviderRequired);
        }
        Ok(AequoraCrypto {
            policy,
            signing: self.signing,
            encryption: self.encryption,
        })
    }
}
