//! Provider-neutral cryptographic integrity and confidentiality for Aequora.
//!
//! This crate composes reviewed primitives. It deliberately does not own transport TLS,
//! application authorization, cloud KMS credentials, platform keystores, or domain-specific E2E
//! protocols. Secret key types are non-serializable, redact `Debug`, and zeroize on drop.

mod artifact;
mod digest;
mod encryption;
mod error;
mod key;
mod operation;
mod policy;
mod protected;
mod registry;
mod service;
mod signature;

pub use artifact::{
    ArtifactContent, ArtifactSigningContext, ArtifactType, CheckpointScope, SignedArtifactManifest,
    SignedCheckpoint, UnsignedArtifactManifest, sign_artifact, sign_checkpoint, verify_artifact,
    verify_checkpoint,
};
pub use digest::{
    ArtifactDigest, AuditDigest, BlobDigest, CanonicalDigest, CiphertextDigest, OperationDigest,
    PlaintextDigest, RegistryDigest, SnapshotDigest, domain_digest,
};
pub use encryption::{
    AAD_VERSION_V1, AssociatedData, DataEncryptionKey, EncryptedPayload, EncryptionKeyProvider,
    InMemoryEncryptionKeyProvider, decrypt_payload, derive_export_key, encrypt_payload,
};
pub use error::{
    CanonicalError, CryptoBuildError, EncryptionError, KeyLifecycleError, PolicyError,
    ProtectedPayloadError, RegistryError, SigningError, VerificationError,
};
pub use key::{
    CryptoKeyStore, CryptoTimestamp, DeviceKeyRecord, KeyDestructionDecision,
    KeyDestructionEvidence, KeyDestructionReceipt, KeyDestructionRequest, KeyIdentity,
    KeyLifecycleEvent, KeyLifecycleEventKind, KeyPurpose, KeyReference, KeyReferenceIndex,
    KeyStatus, PublicKeyBytes, RootKeyId, SignatureBytes, SigningKeyId, TenantKeyId,
};
pub use operation::{
    DeviceKeyRegistry, SignedOperation, sign_operation, verify_operation_signature,
};
pub use policy::{
    CryptoPolicy, CryptoPolicyVersion, CryptoProfile, CryptoRequirement, DigestAlgorithm,
    E2eScheme, EncryptionAlgorithm, PayloadProtectionMode, SignatureAlgorithm,
};
pub use protected::{
    E2eKeyEpoch, ProtectedDomainPolicy, ProtectedPayload, ServerPlaintextResponsibility,
    ServerPlaintextUse,
};
pub use registry::{
    KeyRecord, KeyRegistryGeneration, KeyRegistryManifest, RegistryAcceptance, TrustContext,
    TrustedKeyRegistry, sign_key_registry,
};
pub use service::{AequoraCrypto, AequoraCryptoBuilder};
pub use signature::{
    InMemorySigningKeyProvider, KeyProvider, SignatureEnvelope, SigningSecret, sign_digest,
    verify_digest,
};

use serde::Serialize;

/// Canonically encodes a signed/hashed structure using the repository's stable postcard format.
///
/// # Errors
///
/// Returns [`CanonicalError`] when the value cannot be serialized.
pub fn canonical_bytes<T: Serialize>(value: &T) -> Result<Vec<u8>, CanonicalError> {
    postcard::to_stdvec(value).map_err(CanonicalError::Encode)
}

#[cfg(test)]
mod tests;
