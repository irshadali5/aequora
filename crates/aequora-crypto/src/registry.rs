use crate::{
    CryptoPolicy, KeyProvider, RegistryDigest, RegistryError, SignatureEnvelope, SigningError,
    VerificationError,
    artifact::ArtifactType,
    canonical_bytes,
    key::{CryptoTimestamp, KeyPurpose, KeyStatus, PublicKeyBytes, SigningKeyId},
    signature::{sign_digest, verify_digest},
};
use aequora_types::TenantId;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct KeyRegistryGeneration(pub u64);

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct KeyRecord {
    pub key_id: SigningKeyId,
    pub tenant_id: Option<TenantId>,
    pub purpose: KeyPurpose,
    pub public_key: PublicKeyBytes,
    pub status: KeyStatus,
    pub not_before: CryptoTimestamp,
    pub not_after: Option<CryptoTimestamp>,
    pub created_at: CryptoTimestamp,
    pub revoked_at: Option<CryptoTimestamp>,
    pub compromise_time: Option<CryptoTimestamp>,
    pub revocation_reason: Option<String>,
}

impl KeyRecord {
    fn validate(&self) -> Result<(), RegistryError> {
        if !self.purpose.is_signing()
            || self.not_after.is_some_and(|end| end < self.not_before)
            || (self.status == KeyStatus::Revoked && self.revoked_at.is_none())
            || (self.status != KeyStatus::Revoked && self.revoked_at.is_some())
        {
            return Err(RegistryError::InvalidKey);
        }
        Ok(())
    }

    /// Checks purpose, tenant, validity interval, and revocation time for a signature.
    ///
    /// # Errors
    ///
    /// Returns [`VerificationError`] when the key did not authorize the signature.
    pub fn verify_authorization(
        &self,
        tenant_id: TenantId,
        purpose: KeyPurpose,
        signature_time: CryptoTimestamp,
    ) -> Result<(), VerificationError> {
        if self.purpose != purpose {
            return Err(VerificationError::PurposeMismatch);
        }
        if self.tenant_id.is_some_and(|tenant| tenant != tenant_id) {
            return Err(VerificationError::TenantMismatch);
        }
        if signature_time < self.not_before
            || self.not_after.is_some_and(|end| signature_time > end)
        {
            return Err(VerificationError::KeyExpired);
        }
        if self
            .revoked_at
            .is_some_and(|revoked_at| signature_time >= revoked_at)
            || self.status == KeyStatus::Destroyed
        {
            return Err(VerificationError::KeyRevoked);
        }
        if !self.status.permits_verification() && self.status != KeyStatus::Revoked {
            return Err(VerificationError::KeyRevoked);
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct KeyRegistryManifest {
    pub generation: KeyRegistryGeneration,
    pub issued_at: CryptoTimestamp,
    pub keys: Vec<KeyRecord>,
    pub signature: SignatureEnvelope,
}

impl KeyRegistryManifest {
    fn digest(&self) -> Result<RegistryDigest, RegistryError> {
        canonical_bytes(&(self.generation, self.issued_at, &self.keys))
            .map(|bytes| RegistryDigest::of(&bytes))
            .map_err(|_| RegistryError::CanonicalEncoding)
    }
}

/// Signs a complete monotonic key-registry generation with a pinned root key.
///
/// # Errors
///
/// Returns [`SigningError`] for canonical encoding or provider signing failure.
pub async fn sign_key_registry<P: KeyProvider + ?Sized>(
    provider: &P,
    generation: KeyRegistryGeneration,
    issued_at: CryptoTimestamp,
    keys: Vec<KeyRecord>,
    root_key_id: SigningKeyId,
) -> Result<KeyRegistryManifest, SigningError> {
    let placeholder = SignatureEnvelope {
        algorithm: crate::SignatureAlgorithm::Ed25519V1,
        key_id: root_key_id,
        purpose: KeyPurpose::RegistrySigning,
        signed_at: issued_at,
        signature: crate::SignatureBytes(Vec::new()),
    };
    let mut manifest = KeyRegistryManifest {
        generation,
        issued_at,
        keys,
        signature: placeholder,
    };
    let digest = manifest
        .digest()
        .map_err(|_| SigningError::CanonicalEncoding)?;
    manifest.signature = sign_digest(
        provider,
        KeyPurpose::RegistrySigning,
        root_key_id,
        issued_at,
        digest.0,
    )
    .await?;
    Ok(manifest)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RegistryAcceptance {
    pub previous: KeyRegistryGeneration,
    pub accepted: KeyRegistryGeneration,
    pub keys: usize,
}

#[derive(Clone, Debug)]
pub struct TrustedKeyRegistry {
    highest_generation: KeyRegistryGeneration,
    roots: HashMap<SigningKeyId, PublicKeyBytes>,
    keys: HashMap<SigningKeyId, KeyRecord>,
}

impl TrustedKeyRegistry {
    #[must_use]
    pub fn new(root_id: SigningKeyId, root: PublicKeyBytes) -> Self {
        Self {
            highest_generation: KeyRegistryGeneration(0),
            roots: HashMap::from([(root_id, root)]),
            keys: HashMap::new(),
        }
    }

    #[must_use]
    pub const fn highest_generation(&self) -> KeyRegistryGeneration {
        self.highest_generation
    }

    #[must_use]
    pub fn key(&self, key_id: SigningKeyId) -> Option<&KeyRecord> {
        self.keys.get(&key_id)
    }

    /// Resolves the one active, currently valid key for a tenant and purpose at startup.
    ///
    /// # Errors
    ///
    /// Returns [`VerificationError`] if the complete accepted registry lacks a usable active key.
    pub fn active_key(
        &self,
        tenant_id: TenantId,
        purpose: KeyPurpose,
        now: CryptoTimestamp,
    ) -> Result<&KeyRecord, VerificationError> {
        let key = self
            .keys
            .values()
            .find(|key| {
                key.status == KeyStatus::Active
                    && key.purpose == purpose
                    && key.tenant_id.is_none_or(|tenant| tenant == tenant_id)
            })
            .ok_or(VerificationError::KeyUnknown)?;
        key.verify_authorization(tenant_id, purpose, now)?;
        Ok(key)
    }

    /// Verifies and atomically accepts a newer complete registry generation.
    ///
    /// # Errors
    ///
    /// Returns [`RegistryError`] for rollback, signature, duplicate, or metadata failure.
    pub fn accept(
        &mut self,
        manifest: &KeyRegistryManifest,
    ) -> Result<RegistryAcceptance, RegistryError> {
        if manifest.generation <= self.highest_generation {
            return Err(RegistryError::Rollback);
        }
        if manifest.signature.purpose != KeyPurpose::RegistrySigning {
            return Err(RegistryError::SignatureInvalid);
        }
        let root = self
            .roots
            .get(&manifest.signature.key_id)
            .copied()
            .ok_or(RegistryError::RootUnknown)?;
        verify_digest(root, manifest.digest()?.0, &manifest.signature)
            .map_err(|_| RegistryError::SignatureInvalid)?;

        let mut identities = HashSet::with_capacity(manifest.keys.len());
        let mut active_purposes = HashSet::new();
        for key in &manifest.keys {
            key.validate()?;
            if !identities.insert(key.key_id) {
                return Err(RegistryError::DuplicateKey);
            }
            if key.status == KeyStatus::Active
                && !active_purposes.insert((key.tenant_id, key.purpose))
            {
                return Err(RegistryError::InvalidKey);
            }
        }

        let previous = self.highest_generation;
        self.highest_generation = manifest.generation;
        self.keys = manifest
            .keys
            .iter()
            .cloned()
            .map(|key| (key.key_id, key))
            .collect();
        Ok(RegistryAcceptance {
            previous,
            accepted: manifest.generation,
            keys: self.keys.len(),
        })
    }
}

pub struct TrustContext<'a> {
    pub tenant_id: TenantId,
    pub artifact_type: ArtifactType,
    pub expected_purpose: KeyPurpose,
    pub registry: &'a TrustedKeyRegistry,
    pub policy: &'a CryptoPolicy,
}
