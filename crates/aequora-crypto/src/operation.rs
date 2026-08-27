use crate::{
    CryptoPolicy, KeyProvider, KeyPurpose, KeyStatus, OperationDigest, SignatureEnvelope,
    SigningError, SigningKeyId, VerificationError, canonical_bytes, sign_digest, verify_digest,
};
use crate::{CryptoTimestamp, DeviceKeyRecord};
use aequora_protocol::OperationEnvelope;
use aequora_types::DeviceId;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Serialize)]
struct SemanticOperation<'a> {
    protocol_version: aequora_types::ProtocolVersion,
    operation_id: aequora_types::OperationId,
    tenant_id: aequora_types::TenantId,
    actor_id: aequora_types::ActorId,
    device_id: DeviceId,
    entity: aequora_types::EntityRef,
    base_version: Option<aequora_types::EntityVersion>,
    created_at: aequora_types::HybridTimestamp,
    schema_version: aequora_types::SchemaVersion,
    operation_kind: aequora_protocol::OperationKind,
    payload: &'a [u8],
    dependencies: &'a [aequora_types::OperationId],
    lineage: aequora_types::LineageContext,
}

fn operation_digest(operation: &OperationEnvelope) -> Result<OperationDigest, VerificationError> {
    let semantic = SemanticOperation {
        protocol_version: operation.protocol_version,
        operation_id: operation.operation_id,
        tenant_id: operation.tenant_id,
        actor_id: operation.actor_id,
        device_id: operation.device_id,
        entity: operation.entity,
        base_version: operation.base_version,
        created_at: operation.created_at,
        schema_version: operation.schema_version,
        operation_kind: operation.operation_kind,
        payload: &operation.payload,
        dependencies: &operation.metadata.dependencies,
        lineage: operation
            .metadata
            .lineage
            .resolved_for_operation(operation.operation_id),
    };
    canonical_bytes(&semantic)
        .map(|bytes| OperationDigest::of(&bytes))
        .map_err(|_| VerificationError::CanonicalEncoding)
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct SignedOperation {
    pub operation: OperationEnvelope,
    pub signature: SignatureEnvelope,
}

/// Signs the retry-stable semantic operation envelope, excluding volatile trace metadata.
///
/// # Errors
///
/// Returns [`SigningError`] for canonical encoding or provider signing failure.
pub async fn sign_operation<P: KeyProvider + ?Sized>(
    provider: &P,
    operation: OperationEnvelope,
    key_id: SigningKeyId,
    signed_at: CryptoTimestamp,
) -> Result<SignedOperation, SigningError> {
    let digest = operation_digest(&operation)
        .map_err(|_| SigningError::CanonicalEncoding)?
        .0;
    let signature = sign_digest(
        provider,
        KeyPurpose::DeviceOperationSigning,
        key_id,
        signed_at,
        digest,
    )
    .await?;
    Ok(SignedOperation {
        operation,
        signature,
    })
}

#[derive(Clone, Debug, Default)]
pub struct DeviceKeyRegistry {
    keys: HashMap<SigningKeyId, DeviceKeyRecord>,
}

impl DeviceKeyRegistry {
    /// Registers a unique, structurally valid device verification key.
    ///
    /// # Errors
    ///
    /// Returns [`VerificationError`] for duplicates, unsupported algorithms, or destroyed keys.
    pub fn register(&mut self, record: DeviceKeyRecord) -> Result<(), VerificationError> {
        if self.keys.contains_key(&record.key_id)
            || record.algorithm != crate::SignatureAlgorithm::Ed25519V1
            || record.status == KeyStatus::Destroyed
        {
            return Err(VerificationError::DeviceKeyUnavailable);
        }
        self.keys.insert(record.key_id, record);
        Ok(())
    }

    #[must_use]
    pub fn key(&self, key_id: SigningKeyId) -> Option<&DeviceKeyRecord> {
        self.keys.get(&key_id)
    }
}

/// Verifies device binding, key lifecycle, policy, semantic digest, and signature.
///
/// # Errors
///
/// Returns [`VerificationError`] on any policy, identity, lifecycle, or signature mismatch.
pub fn verify_operation_signature(
    signed: &SignedOperation,
    expected_device: DeviceId,
    registry: &DeviceKeyRegistry,
    policy: &CryptoPolicy,
) -> Result<OperationDigest, VerificationError> {
    policy
        .require_signature(signed.signature.algorithm)
        .map_err(|_| VerificationError::AlgorithmDisallowed)?;
    if signed.signature.purpose != KeyPurpose::DeviceOperationSigning {
        return Err(VerificationError::PurposeMismatch);
    }
    if signed.operation.device_id != expected_device {
        return Err(VerificationError::DeviceMismatch);
    }
    let record = registry
        .key(signed.signature.key_id)
        .ok_or(VerificationError::KeyUnknown)?;
    if record.device_id != expected_device {
        return Err(VerificationError::DeviceMismatch);
    }
    if signed.signature.signed_at < record.not_before
        || record
            .not_after
            .is_some_and(|end| signed.signature.signed_at > end)
    {
        return Err(VerificationError::KeyExpired);
    }
    if record
        .revoked_at
        .is_some_and(|revoked| signed.signature.signed_at >= revoked)
        || record.status == KeyStatus::Destroyed
    {
        return Err(VerificationError::KeyRevoked);
    }
    let digest = operation_digest(&signed.operation)?;
    verify_digest(record.public_key, digest.0, &signed.signature)?;
    Ok(digest)
}
