//! Operation schema upcasting, retry-only admission, and immutable retry semantics.

use crate::{CompatibilityError, OperationSchemaVersion, SupportStatus};
use aequora_types::{OperationId, SchemaVersion};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Stable operation kind ID governed by the compatibility registry.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct OperationKindId(pub u32);

/// Canonical semantic payload digest retained by the operation ledger.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(transparent)]
pub struct SemanticPayloadHash(pub [u8; 32]);

impl SemanticPayloadHash {
    #[must_use]
    pub fn of(kind: OperationKindId, schema: SchemaVersion, payload: &[u8]) -> Self {
        let mut hasher = blake3::Hasher::new();
        hasher.update(b"aequora.operation.semantic-payload.v1\0");
        hasher.update(&kind.0.to_be_bytes());
        hasher.update(&schema.0.to_be_bytes());
        hasher.update(payload);
        Self(*hasher.finalize().as_bytes())
    }
}

/// Original wire form of an operation that may have reached the authority.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct PossiblySentOperation {
    operation_id: OperationId,
    kind: OperationKindId,
    schema: OperationSchemaVersion,
    payload: Vec<u8>,
    semantic_hash: SemanticPayloadHash,
}

impl PossiblySentOperation {
    #[must_use]
    pub fn new(
        operation_id: OperationId,
        kind: OperationKindId,
        schema: OperationSchemaVersion,
        payload: Vec<u8>,
    ) -> Self {
        let semantic_hash = SemanticPayloadHash::of(kind, schema, &payload);
        Self {
            operation_id,
            kind,
            schema,
            payload,
            semantic_hash,
        }
    }

    #[must_use]
    pub const fn operation_id(&self) -> OperationId {
        self.operation_id
    }

    #[must_use]
    pub const fn kind(&self) -> OperationKindId {
        self.kind
    }

    #[must_use]
    pub const fn schema(&self) -> OperationSchemaVersion {
        self.schema
    }

    #[must_use]
    pub fn payload(&self) -> &[u8] {
        &self.payload
    }

    #[must_use]
    pub const fn semantic_hash(&self) -> SemanticPayloadHash {
        self.semantic_hash
    }

    /// Detects corrupt or manually altered durable retry metadata.
    ///
    /// # Errors
    ///
    /// Returns an immutability error when stored bytes do not match their semantic hash.
    pub fn validate(&self) -> Result<(), CompatibilityError> {
        if SemanticPayloadHash::of(self.kind, self.schema, &self.payload) != self.semantic_hash {
            return Err(CompatibilityError::PossiblySentOperationImmutable);
        }
        Ok(())
    }

    /// Confirms a retry is byte-for-byte and semantically identical to the possibly sent form.
    ///
    /// # Errors
    ///
    /// Returns an immutability error for a corrupt record or changed retry schema/payload.
    pub fn verify_retry(
        &self,
        schema: SchemaVersion,
        payload: &[u8],
    ) -> Result<(), CompatibilityError> {
        self.validate()?;
        if schema != self.schema
            || SemanticPayloadHash::of(self.kind, schema, payload) != self.semantic_hash
        {
            return Err(CompatibilityError::PossiblySentOperationImmutable);
        }
        Ok(())
    }
}

/// Pure deterministic one-step operation migration.
pub trait OperationUpcaster: Send + Sync {
    fn kind(&self) -> OperationKindId;
    fn from(&self) -> OperationSchemaVersion;
    fn to(&self) -> OperationSchemaVersion;
    /// Produces the next canonical schema payload without side effects.
    ///
    /// # Errors
    ///
    /// Returns a compatibility error when the old payload is not valid for this deterministic step.
    fn upcast(&self, payload: &[u8]) -> Result<Vec<u8>, CompatibilityError>;
}

/// Typed upcaster chain. The registry never downcasts authority execution payloads.
#[derive(Default)]
pub struct UpcasterRegistry {
    steps: BTreeMap<(OperationKindId, u16), Box<dyn OperationUpcaster>>,
}

impl UpcasterRegistry {
    /// Registers one exact adjacent-version step.
    ///
    /// # Errors
    ///
    /// Returns an invalid-step or duplicate error for an unsafe chain entry.
    pub fn register(
        &mut self,
        upcaster: impl OperationUpcaster + 'static,
    ) -> Result<(), CompatibilityError> {
        if upcaster.kind().0 == 0
            || upcaster.from().0 == 0
            || upcaster.to().0 != upcaster.from().0.saturating_add(1)
        {
            return Err(CompatibilityError::InvalidUpcastStep);
        }
        let key = (upcaster.kind(), upcaster.from().0);
        if self.steps.insert(key, Box::new(upcaster)).is_some() {
            return Err(CompatibilityError::DuplicateRegistryId);
        }
        Ok(())
    }

    /// Upcasts an old payload to canonical current semantics through every registered step.
    ///
    /// # Errors
    ///
    /// Returns an unsupported-schema or invalid-step error when the chain is incomplete.
    pub fn upcast(
        &self,
        kind: OperationKindId,
        from: OperationSchemaVersion,
        current: OperationSchemaVersion,
        payload: &[u8],
    ) -> Result<Vec<u8>, CompatibilityError> {
        if from.0 == 0 || current.0 == 0 || from > current {
            return Err(CompatibilityError::OperationSchemaUnsupported);
        }
        let mut version = from;
        let mut canonical = payload.to_vec();
        while version < current {
            let step = self
                .steps
                .get(&(kind, version.0))
                .ok_or(CompatibilityError::OperationSchemaUnsupported)?;
            if step.kind() != kind
                || step.from() != version
                || step.to().0 != version.0.saturating_add(1)
            {
                return Err(CompatibilityError::InvalidUpcastStep);
            }
            canonical = step.upcast(&canonical)?;
            version = step.to();
        }
        Ok(canonical)
    }
}

/// Whether an old schema attempt can enter authority processing.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OperationAdmission {
    AcceptCurrent,
    AcceptSupported,
    AcceptDeprecatedWithWarning,
    AcceptHistoricalRetry,
    RejectNewCreation,
    RejectRemoved,
    RejectPayloadMismatch,
}

/// Applies the retry-only distinction using ledger first-seen evidence.
#[must_use]
pub fn classify_operation_attempt(
    status: SupportStatus,
    known_retry_hash: Option<SemanticPayloadHash>,
    offered_hash: SemanticPayloadHash,
) -> OperationAdmission {
    match status {
        SupportStatus::Experimental | SupportStatus::Current => OperationAdmission::AcceptCurrent,
        SupportStatus::Supported => OperationAdmission::AcceptSupported,
        SupportStatus::Deprecated => OperationAdmission::AcceptDeprecatedWithWarning,
        SupportStatus::RetryOnly => match known_retry_hash {
            Some(expected) if expected == offered_hash => OperationAdmission::AcceptHistoricalRetry,
            Some(_) => OperationAdmission::RejectPayloadMismatch,
            None => OperationAdmission::RejectNewCreation,
        },
        SupportStatus::Removed => OperationAdmission::RejectRemoved,
    }
}
