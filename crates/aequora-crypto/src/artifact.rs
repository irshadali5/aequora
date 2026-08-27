use crate::{
    ArtifactDigest, CanonicalDigest, CanonicalError, CryptoPolicyVersion, KeyProvider,
    SignatureEnvelope, SigningError, SigningKeyId, TrustContext, VerificationError,
    canonical_bytes, domain_digest,
    key::{CryptoTimestamp, KeyPurpose},
    policy::{DigestAlgorithm, SignatureAlgorithm},
    sign_digest, verify_digest,
};
use aequora_types::{Sequence, SyncScopeId, TenantId};
use serde::{Deserialize, Serialize, de::DeserializeOwned};

pub trait ArtifactContent: Serialize + DeserializeOwned {}
impl<T: Serialize + DeserializeOwned> ArtifactContent for T {}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
pub enum ArtifactType {
    Snapshot,
    Export,
    ReplayBundle,
    AuditCheckpoint,
    MigrationManifest,
    GovernanceVerification,
    OfflineSyncBundle,
}

impl ArtifactType {
    #[must_use]
    pub const fn required_purpose(self) -> KeyPurpose {
        match self {
            Self::AuditCheckpoint => KeyPurpose::AuditCheckpointSigning,
            Self::Snapshot
            | Self::Export
            | Self::ReplayBundle
            | Self::MigrationManifest
            | Self::GovernanceVerification
            | Self::OfflineSyncBundle => KeyPurpose::ServerArtifactSigning,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct UnsignedArtifactManifest<T> {
    pub artifact_type: ArtifactType,
    pub format_version: u16,
    pub crypto_policy_version: CryptoPolicyVersion,
    pub tenant_id: TenantId,
    pub content: T,
    pub digest_algorithm: DigestAlgorithm,
    pub content_digest: ArtifactDigest,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct SignedArtifactManifest<T> {
    pub manifest: UnsignedArtifactManifest<T>,
    pub signature: SignatureEnvelope,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ArtifactSigningContext {
    pub artifact_type: ArtifactType,
    pub format_version: u16,
    pub policy_version: CryptoPolicyVersion,
    pub tenant_id: TenantId,
}

fn artifact_signing_digest<T: Serialize>(
    manifest: &UnsignedArtifactManifest<T>,
) -> Result<CanonicalDigest, CanonicalError> {
    canonical_bytes(manifest)
        .map(|bytes| CanonicalDigest(domain_digest("AEQUORA:SIGNED-ARTIFACT-MANIFEST:v1", &bytes)))
}

/// Signs canonical artifact content with an authorized provider key.
///
/// # Errors
///
/// Returns [`SigningError`] when canonical encoding or provider signing fails.
pub async fn sign_artifact<T: ArtifactContent, P: KeyProvider + ?Sized>(
    provider: &P,
    signing: ArtifactSigningContext,
    content: T,
    key_id: SigningKeyId,
    signed_at: CryptoTimestamp,
) -> Result<SignedArtifactManifest<T>, SigningError> {
    let content_bytes = canonical_bytes(&content).map_err(|_| SigningError::CanonicalEncoding)?;
    let manifest = UnsignedArtifactManifest {
        artifact_type: signing.artifact_type,
        format_version: signing.format_version,
        crypto_policy_version: signing.policy_version,
        tenant_id: signing.tenant_id,
        content,
        digest_algorithm: DigestAlgorithm::Blake3V1,
        content_digest: ArtifactDigest::of(&content_bytes),
    };
    let digest = artifact_signing_digest(&manifest)
        .map_err(|_| SigningError::CanonicalEncoding)?
        .0;
    let signature = sign_digest(
        provider,
        signing.artifact_type.required_purpose(),
        key_id,
        signed_at,
        digest,
    )
    .await?;
    Ok(SignedArtifactManifest {
        manifest,
        signature,
    })
}

/// Verifies tenant binding, policy, content digest, key authorization, and signature.
///
/// # Errors
///
/// Returns [`VerificationError`] on any trust, policy, digest, or signature mismatch.
pub fn verify_artifact<T: ArtifactContent>(
    artifact: &SignedArtifactManifest<T>,
    trust: &TrustContext<'_>,
) -> Result<(), VerificationError> {
    if artifact.manifest.tenant_id != trust.tenant_id {
        return Err(VerificationError::TenantMismatch);
    }
    if artifact.manifest.artifact_type != trust.artifact_type {
        return Err(VerificationError::ArtifactTypeMismatch);
    }
    if artifact.manifest.artifact_type.required_purpose() != trust.expected_purpose
        || artifact.signature.purpose != trust.expected_purpose
    {
        return Err(VerificationError::PurposeMismatch);
    }
    trust
        .policy
        .require_digest(artifact.manifest.digest_algorithm)
        .map_err(|_| VerificationError::AlgorithmDisallowed)?;
    trust
        .policy
        .require_signature(artifact.signature.algorithm)
        .map_err(|_| VerificationError::AlgorithmDisallowed)?;
    if artifact.manifest.crypto_policy_version > trust.policy.version {
        return Err(VerificationError::AlgorithmDisallowed);
    }
    let content = canonical_bytes(&artifact.manifest.content)
        .map_err(|_| VerificationError::CanonicalEncoding)?;
    if ArtifactDigest::of(&content) != artifact.manifest.content_digest {
        return Err(VerificationError::DigestMismatch);
    }
    let key = trust
        .registry
        .key(artifact.signature.key_id)
        .ok_or(VerificationError::KeyUnknown)?;
    key.verify_authorization(
        trust.tenant_id,
        trust.expected_purpose,
        artifact.signature.signed_at,
    )?;
    let digest = artifact_signing_digest(&artifact.manifest)
        .map_err(|_| VerificationError::CanonicalEncoding)?;
    verify_digest(key.public_key, digest.0, &artifact.signature)
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
pub enum CheckpointScope {
    Journal(SyncScopeId),
    Audit(u16),
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct SignedCheckpoint {
    pub tenant_id: TenantId,
    pub scope: CheckpointScope,
    pub sequence: Sequence,
    pub root_hash: CanonicalDigest,
    pub signature: SignatureEnvelope,
}

fn checkpoint_digest(
    tenant_id: TenantId,
    scope: CheckpointScope,
    sequence: Sequence,
    root_hash: CanonicalDigest,
) -> Result<[u8; 32], CanonicalError> {
    canonical_bytes(&(tenant_id, scope, sequence, root_hash))
        .map(|bytes| domain_digest("AEQUORA:SIGNED-CHECKPOINT:v1", &bytes))
}

/// Signs a tenant-bound journal or audit checkpoint.
///
/// # Errors
///
/// Returns [`SigningError`] when canonical encoding or provider signing fails.
pub async fn sign_checkpoint<P: KeyProvider + ?Sized>(
    provider: &P,
    tenant_id: TenantId,
    scope: CheckpointScope,
    sequence: Sequence,
    root_hash: CanonicalDigest,
    key_id: SigningKeyId,
    signed_at: CryptoTimestamp,
) -> Result<SignedCheckpoint, SigningError> {
    let purpose = match scope {
        CheckpointScope::Audit(_) => KeyPurpose::AuditCheckpointSigning,
        CheckpointScope::Journal(_) => KeyPurpose::ServerArtifactSigning,
    };
    let digest = checkpoint_digest(tenant_id, scope, sequence, root_hash)
        .map_err(|_| SigningError::CanonicalEncoding)?;
    Ok(SignedCheckpoint {
        tenant_id,
        scope,
        sequence,
        root_hash,
        signature: sign_digest(provider, purpose, key_id, signed_at, digest).await?,
    })
}

/// Verifies a signed checkpoint against its explicit trust context.
///
/// # Errors
///
/// Returns [`VerificationError`] when tenant, purpose, key, time, or signature is invalid.
pub fn verify_checkpoint(
    checkpoint: &SignedCheckpoint,
    trust: &TrustContext<'_>,
) -> Result<(), VerificationError> {
    if checkpoint.tenant_id != trust.tenant_id {
        return Err(VerificationError::TenantMismatch);
    }
    if checkpoint.signature.purpose != trust.expected_purpose {
        return Err(VerificationError::PurposeMismatch);
    }
    if checkpoint.signature.algorithm != SignatureAlgorithm::Ed25519V1 {
        return Err(VerificationError::AlgorithmDisallowed);
    }
    let key = trust
        .registry
        .key(checkpoint.signature.key_id)
        .ok_or(VerificationError::KeyUnknown)?;
    key.verify_authorization(
        checkpoint.tenant_id,
        trust.expected_purpose,
        checkpoint.signature.signed_at,
    )?;
    let digest = checkpoint_digest(
        checkpoint.tenant_id,
        checkpoint.scope,
        checkpoint.sequence,
        checkpoint.root_hash,
    )
    .map_err(|_| VerificationError::CanonicalEncoding)?;
    verify_digest(key.public_key, digest, &checkpoint.signature)
}
