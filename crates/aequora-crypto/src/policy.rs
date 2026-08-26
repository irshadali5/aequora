use crate::PolicyError;
use aequora_protocol::Capability;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
pub enum DigestAlgorithm {
    Blake3V1,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
pub enum SignatureAlgorithm {
    Ed25519V1,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
pub enum EncryptionAlgorithm {
    XChaCha20Poly1305V1,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
pub enum CryptoRequirement {
    Required,
    Preferred,
    Disabled,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
pub enum CryptoProfile {
    Standard,
    Enterprise,
    HighAssurance,
    ClientManagedE2E,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
pub enum PayloadProtectionMode {
    ServerReadable,
    TenantEncryptedAtRest,
    ClientManagedE2E,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
pub enum E2eScheme {
    ApplicationManagedV1,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct CryptoPolicyVersion(pub u32);

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct CryptoPolicy {
    pub version: CryptoPolicyVersion,
    pub allowed_digests: Vec<DigestAlgorithm>,
    pub allowed_signatures: Vec<SignatureAlgorithm>,
    pub allowed_encryption: Vec<EncryptionAlgorithm>,
    pub signed_snapshots: CryptoRequirement,
    pub signed_exports: CryptoRequirement,
    pub signed_audit_checkpoints: CryptoRequirement,
    pub device_operation_signatures: CryptoRequirement,
    pub encrypted_snapshots: CryptoRequirement,
    pub encrypted_exports: CryptoRequirement,
    pub encrypted_replay_bundles: CryptoRequirement,
    pub tenant_data_encryption: CryptoRequirement,
}

impl CryptoPolicy {
    #[must_use]
    pub fn standard() -> Self {
        Self {
            version: CryptoPolicyVersion(1),
            allowed_digests: vec![DigestAlgorithm::Blake3V1],
            allowed_signatures: vec![SignatureAlgorithm::Ed25519V1],
            allowed_encryption: vec![EncryptionAlgorithm::XChaCha20Poly1305V1],
            signed_snapshots: CryptoRequirement::Preferred,
            signed_exports: CryptoRequirement::Preferred,
            signed_audit_checkpoints: CryptoRequirement::Preferred,
            device_operation_signatures: CryptoRequirement::Disabled,
            encrypted_snapshots: CryptoRequirement::Disabled,
            encrypted_exports: CryptoRequirement::Disabled,
            encrypted_replay_bundles: CryptoRequirement::Disabled,
            tenant_data_encryption: CryptoRequirement::Disabled,
        }
    }

    #[must_use]
    pub fn enterprise() -> Self {
        Self {
            signed_snapshots: CryptoRequirement::Required,
            signed_exports: CryptoRequirement::Required,
            signed_audit_checkpoints: CryptoRequirement::Required,
            encrypted_snapshots: CryptoRequirement::Required,
            encrypted_exports: CryptoRequirement::Required,
            encrypted_replay_bundles: CryptoRequirement::Required,
            tenant_data_encryption: CryptoRequirement::Required,
            ..Self::standard()
        }
    }

    #[must_use]
    pub fn for_profile(profile: CryptoProfile) -> Self {
        match profile {
            CryptoProfile::Standard | CryptoProfile::ClientManagedE2E => Self::standard(),
            CryptoProfile::Enterprise => Self::enterprise(),
            CryptoProfile::HighAssurance => Self {
                device_operation_signatures: CryptoRequirement::Required,
                ..Self::enterprise()
            },
        }
    }

    /// Validates non-empty algorithm allowlists and a non-zero version.
    ///
    /// # Errors
    ///
    /// Returns [`PolicyError`] for an unusable policy.
    pub const fn validate(&self) -> Result<(), PolicyError> {
        if self.version.0 == 0
            || self.allowed_digests.is_empty()
            || self.allowed_signatures.is_empty()
            || self.allowed_encryption.is_empty()
        {
            return Err(PolicyError::EmptyAlgorithmSet);
        }
        Ok(())
    }

    /// Requires a digest algorithm from the explicit allowlist.
    ///
    /// # Errors
    ///
    /// Returns [`PolicyError`] when `algorithm` is disallowed.
    pub fn require_digest(&self, algorithm: DigestAlgorithm) -> Result<(), PolicyError> {
        self.allowed_digests
            .contains(&algorithm)
            .then_some(())
            .ok_or(PolicyError::DigestDisallowed)
    }

    /// Requires a signature algorithm from the explicit allowlist.
    ///
    /// # Errors
    ///
    /// Returns [`PolicyError`] when `algorithm` is disallowed.
    pub fn require_signature(&self, algorithm: SignatureAlgorithm) -> Result<(), PolicyError> {
        self.allowed_signatures
            .contains(&algorithm)
            .then_some(())
            .ok_or(PolicyError::SignatureDisallowed)
    }

    /// Requires an encryption algorithm from the explicit allowlist.
    ///
    /// # Errors
    ///
    /// Returns [`PolicyError`] when `algorithm` is disallowed.
    pub fn require_encryption(&self, algorithm: EncryptionAlgorithm) -> Result<(), PolicyError> {
        self.allowed_encryption
            .contains(&algorithm)
            .then_some(())
            .ok_or(PolicyError::EncryptionDisallowed)
    }

    /// Enforces mandatory features without best-mutual-support downgrade.
    ///
    /// # Errors
    ///
    /// Returns [`PolicyError`] when the client lacks a required capability.
    pub fn validate_client_capabilities(
        &self,
        capabilities: &[Capability],
    ) -> Result<(), PolicyError> {
        if self.signed_snapshots == CryptoRequirement::Required
            && !capabilities.contains(&Capability::SignedSnapshotV1)
        {
            return Err(PolicyError::RequiredCapabilityMissing);
        }
        if self.device_operation_signatures == CryptoRequirement::Required
            && !capabilities.contains(&Capability::DeviceSignatureV1)
        {
            return Err(PolicyError::RequiredCapabilityMissing);
        }
        if self.encrypted_snapshots == CryptoRequirement::Required
            && !capabilities.contains(&Capability::EncryptedSnapshotV1)
        {
            return Err(PolicyError::RequiredCapabilityMissing);
        }
        Ok(())
    }
}

impl Default for CryptoPolicy {
    fn default() -> Self {
        Self::standard()
    }
}
