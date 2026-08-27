use crate::{CanonicalDigest, KeyLifecycleError, policy::SignatureAlgorithm};
use aequora_types::{DeviceId, TenantId};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeSet, HashMap};
use uuid::Uuid;

macro_rules! key_id {
    ($(#[$meta:meta])* $name:ident) => {
        $(#[$meta])*
        #[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
        #[serde(transparent)]
        pub struct $name(Uuid);

        impl $name {
            #[must_use]
            pub fn new() -> Self { Self(Uuid::now_v7()) }
            #[must_use]
            pub const fn from_uuid(value: Uuid) -> Self { Self(value) }
            #[must_use]
            pub const fn as_uuid(self) -> Uuid { self.0 }
        }

        impl Default for $name {
            fn default() -> Self { Self::new() }
        }
    };
}

key_id!(/// Identity of one immutable signing-key version.
    SigningKeyId);
key_id!(/// Identity of one immutable tenant encryption-key version.
    TenantKeyId);
key_id!(/// Identity of a registry/root signing key.
    RootKeyId);

#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct CryptoTimestamp(pub i64);

#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
pub enum KeyPurpose {
    ServerArtifactSigning,
    DeviceOperationSigning,
    TenantDataEncryption,
    SnapshotEncryption,
    ExportEncryption,
    ReplayBundleEncryption,
    AuditArchiveEncryption,
    AuditCheckpointSigning,
    RegistrySigning,
}

impl KeyPurpose {
    #[must_use]
    pub const fn is_signing(self) -> bool {
        matches!(
            self,
            Self::ServerArtifactSigning
                | Self::DeviceOperationSigning
                | Self::AuditCheckpointSigning
                | Self::RegistrySigning
        )
    }

    #[must_use]
    pub const fn is_encryption(self) -> bool {
        !self.is_signing()
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
pub enum KeyStatus {
    Pending,
    Active,
    Retiring,
    VerificationOnly,
    Revoked,
    Destroyed,
}

impl KeyStatus {
    #[must_use]
    pub const fn permits_signing(self) -> bool {
        matches!(self, Self::Active)
    }

    #[must_use]
    pub const fn permits_verification(self) -> bool {
        matches!(self, Self::Active | Self::Retiring | Self::VerificationOnly)
    }

    /// Applies the one-way key lifecycle.
    ///
    /// # Errors
    ///
    /// Returns [`KeyLifecycleError`] for rollback, reactivation, or skipped states.
    pub const fn transition(self, next: Self) -> Result<Self, KeyLifecycleError> {
        let valid = matches!(
            (self, next),
            (Self::Pending, Self::Active | Self::Revoked)
                | (Self::Active, Self::Retiring | Self::Revoked)
                | (Self::Retiring, Self::VerificationOnly | Self::Revoked)
                | (Self::VerificationOnly, Self::Revoked | Self::Destroyed)
                | (Self::Revoked, Self::Destroyed)
        );
        if valid {
            Ok(next)
        } else {
            Err(KeyLifecycleError::InvalidTransition)
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(transparent)]
pub struct PublicKeyBytes(pub [u8; 32]);

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(transparent)]
pub struct SignatureBytes(pub Vec<u8>);

impl SignatureBytes {
    pub const ED25519_LENGTH: usize = 64;
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct DeviceKeyRecord {
    pub device_id: DeviceId,
    pub key_id: SigningKeyId,
    pub algorithm: SignatureAlgorithm,
    pub public_key: PublicKeyBytes,
    pub status: KeyStatus,
    pub not_before: CryptoTimestamp,
    pub not_after: Option<CryptoTimestamp>,
    pub revoked_at: Option<CryptoTimestamp>,
}

#[derive(Clone, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
pub struct KeyReference {
    pub tenant_id: TenantId,
    pub key_id: TenantKeyId,
    pub reference_id: Uuid,
    pub data_class: u16,
    pub required: bool,
    pub retention_until: Option<CryptoTimestamp>,
}

#[derive(Clone, Debug, Default)]
pub struct KeyReferenceIndex {
    references: HashMap<TenantKeyId, BTreeSet<KeyReference>>,
}

impl KeyReferenceIndex {
    pub fn insert(&mut self, reference: KeyReference) -> bool {
        self.references
            .entry(reference.key_id)
            .or_default()
            .insert(reference)
    }

    pub fn remove(&mut self, reference: &KeyReference) -> bool {
        self.references
            .get_mut(&reference.key_id)
            .is_some_and(|references| references.remove(reference))
    }

    #[must_use]
    pub fn references(&self, key_id: TenantKeyId) -> Vec<&KeyReference> {
        self.references.get(&key_id).into_iter().flatten().collect()
    }

    #[must_use]
    pub fn required_references(&self, key_id: TenantKeyId) -> usize {
        self.references(key_id)
            .into_iter()
            .filter(|reference| reference.required)
            .count()
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct KeyDestructionEvidence {
    pub required_ciphertext_references: u64,
    pub usable_backup_copies: u32,
    pub usable_recovery_wraps: u32,
    pub plaintext_caches: u32,
    pub intentional_erasure_authorized: bool,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum KeyDestructionDecision {
    SafeRotationCleanup,
    SafeIntentionalErasure,
}

impl KeyDestructionEvidence {
    /// Determines whether destruction is safe and accurately classified.
    ///
    /// # Errors
    ///
    /// Returns [`KeyLifecycleError`] while required data, key copies, or caches remain.
    pub fn evaluate(self) -> Result<KeyDestructionDecision, KeyLifecycleError> {
        if self.usable_backup_copies > 0 || self.usable_recovery_wraps > 0 {
            return Err(KeyLifecycleError::UsableKeyCopyRemains);
        }
        if self.plaintext_caches > 0 {
            return Err(KeyLifecycleError::PlaintextCacheRemains);
        }
        if self.required_ciphertext_references > 0 && !self.intentional_erasure_authorized {
            return Err(KeyLifecycleError::RequiredCiphertextRemains);
        }
        Ok(if self.intentional_erasure_authorized {
            KeyDestructionDecision::SafeIntentionalErasure
        } else {
            KeyDestructionDecision::SafeRotationCleanup
        })
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
pub enum KeyIdentity {
    Signing(SigningKeyId),
    Encryption(TenantKeyId),
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
pub enum KeyLifecycleEventKind {
    Created,
    Activated,
    Rotated,
    Retired,
    Revoked,
    RecoveryUsed,
    DestroyedForErasure,
}

/// Secret-free canonical declaration for application audit integration.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct KeyLifecycleEvent {
    pub tenant_id: Option<TenantId>,
    pub key: KeyIdentity,
    pub purpose: KeyPurpose,
    pub kind: KeyLifecycleEventKind,
    pub occurred_at: CryptoTimestamp,
    pub authorization_digest: Option<CanonicalDigest>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct KeyDestructionRequest {
    pub tenant_id: TenantId,
    pub key_id: TenantKeyId,
    pub requested_at: CryptoTimestamp,
    pub authorization_digest: CanonicalDigest,
    pub evidence: KeyDestructionEvidence,
}

impl KeyDestructionRequest {
    /// Verifies all registered key copies/caches and explicit authorization before destruction.
    ///
    /// # Errors
    ///
    /// Returns [`KeyLifecycleError`] for missing authorization or incomplete destruction evidence.
    pub fn verify(self) -> Result<KeyDestructionDecision, KeyLifecycleError> {
        if self.authorization_digest.0 == [0; 32] {
            return Err(KeyLifecycleError::AuthorizationEvidenceMissing);
        }
        self.evidence.evaluate()
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct KeyDestructionReceipt {
    pub tenant_id: TenantId,
    pub key_id: TenantKeyId,
    pub destroyed_at: CryptoTimestamp,
    pub authorization_digest: CanonicalDigest,
    pub decision: KeyDestructionDecision,
}

#[async_trait]
pub trait CryptoKeyStore: Send + Sync {
    /// Atomically destroys every usable provider-side copy authorized by this request.
    async fn destroy_for_erasure(
        &self,
        request: KeyDestructionRequest,
    ) -> Result<KeyDestructionReceipt, KeyLifecycleError>;
}
