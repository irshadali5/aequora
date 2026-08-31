//! Stable, storage- and transport-neutral application operation contracts.
//!
//! This crate is the manual API behind any operation derive macro. It deliberately does not
//! expose journal records, cursors, database transactions, or protocol envelopes.

use aequora_types::{EntityId, OperationId};
use serde::{Deserialize, Serialize};
use std::fmt;
use thiserror::Error;

/// Stable registry identity for an application operation kind. Zero is reserved.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct OperationKind(u32);

impl OperationKind {
    /// Creates a compile-time operation kind and panics when zero is supplied.
    ///
    /// Prefer [`Self::new`] when the value is not a compile-time registry constant.
    ///
    /// # Panics
    ///
    /// Panics when `value` is zero.
    #[must_use]
    pub const fn from_static(value: u32) -> Self {
        assert!(value != 0, "operation kind zero is reserved");
        Self(value)
    }

    /// Creates an operation kind.
    ///
    /// # Errors
    ///
    /// Returns [`OperationContractError::ReservedKind`] for zero.
    pub const fn new(value: u32) -> Result<Self, OperationContractError> {
        if value == 0 {
            Err(OperationContractError::ReservedKind)
        } else {
            Ok(Self(value))
        }
    }

    /// Returns the stable registry value.
    #[must_use]
    pub const fn get(self) -> u32 {
        self.0
    }
}

impl fmt::Display for OperationKind {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}

/// Version of one application operation's payload schema. Zero is reserved.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct OperationSchemaVersion(u16);

impl OperationSchemaVersion {
    /// Creates a compile-time schema version and panics when zero is supplied.
    ///
    /// Prefer [`Self::new`] when the value is not a compile-time registry constant.
    ///
    /// # Panics
    ///
    /// Panics when `value` is zero.
    #[must_use]
    pub const fn from_static(value: u16) -> Self {
        assert!(value != 0, "operation schema version zero is reserved");
        Self(value)
    }

    /// Creates an operation schema version.
    ///
    /// # Errors
    ///
    /// Returns [`OperationContractError::ReservedSchemaVersion`] for zero.
    pub const fn new(value: u16) -> Result<Self, OperationContractError> {
        if value == 0 {
            Err(OperationContractError::ReservedSchemaVersion)
        } else {
            Ok(Self(value))
        }
    }

    /// Returns the stable registry value.
    #[must_use]
    pub const fn get(self) -> u16 {
        self.0
    }
}

impl fmt::Display for OperationSchemaVersion {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}

/// Application operation submitted through the high-level SDK.
///
/// Implementations choose their payload codec explicitly. This keeps the manual API usable
/// without a proc macro and prevents a Serde derive from silently promising wire stability.
pub trait Operation: Send + Sync + 'static {
    /// Typed authoritative outcome returned by operation inspection APIs.
    type Outcome: Send + Sync + 'static;

    /// Stable operation registry identity.
    const KIND: OperationKind;
    /// Application payload schema version, independent from crate and protocol versions.
    const SCHEMA_VERSION: OperationSchemaVersion;

    /// Encodes the application payload for durable local submission.
    ///
    /// # Errors
    ///
    /// Returns a bounded, non-sensitive encoding error. Implementations must not include secret
    /// payload values in the error message.
    fn encode(&self) -> Result<Vec<u8>, OperationEncodingError>;
}

/// Encoded operation accepted by adapter SDK implementations.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EncodedOperation {
    operation_id: OperationId,
    kind: OperationKind,
    schema_version: OperationSchemaVersion,
    payload: Vec<u8>,
}

impl EncodedOperation {
    /// Encodes a typed operation and assigns its permanent idempotency identity.
    ///
    /// # Errors
    ///
    /// Returns the operation's encoding failure.
    pub fn from_operation<O: Operation>(operation: &O) -> Result<Self, OperationEncodingError> {
        Ok(Self {
            operation_id: OperationId::new(),
            kind: O::KIND,
            schema_version: O::SCHEMA_VERSION,
            payload: operation.encode()?,
        })
    }

    /// Permanent idempotency identity.
    #[must_use]
    pub const fn operation_id(&self) -> OperationId {
        self.operation_id
    }

    /// Stable operation kind.
    #[must_use]
    pub const fn kind(&self) -> OperationKind {
        self.kind
    }

    /// Application payload schema version.
    #[must_use]
    pub const fn schema_version(&self) -> OperationSchemaVersion {
        self.schema_version
    }

    /// Opaque application payload.
    #[must_use]
    pub fn payload(&self) -> &[u8] {
        &self.payload
    }
}

/// Local durability result. It never implies authoritative acceptance.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum LocalCommitStatus {
    /// The operation and its outbox intent committed atomically.
    SavedLocally,
    /// Equivalent newer intent replaced this operation under a certified profile.
    Superseded,
}

/// Receipt returned only after the local adapter reports durable intent.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MutationReceipt {
    operation_id: OperationId,
    local_status: LocalCommitStatus,
}

impl MutationReceipt {
    /// Creates a receipt from an adapter's durable commit result.
    #[must_use]
    pub const fn new(operation_id: OperationId, local_status: LocalCommitStatus) -> Self {
        Self {
            operation_id,
            local_status,
        }
    }

    /// Permanent operation identity.
    #[must_use]
    pub const fn operation_id(self) -> OperationId {
        self.operation_id
    }

    /// Local-only commit status.
    #[must_use]
    pub const fn local_status(self) -> LocalCommitStatus {
        self.local_status
    }
}

/// Stable, high-level lifecycle of a submitted operation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum OperationState {
    /// Durable locally and eligible for transmission.
    Pending,
    /// Reserved by a bounded synchronization attempt.
    InFlight,
    /// Accepted by the current authority.
    AuthoritativeAccepted,
    /// Business or validation rejection; not a transport failure.
    Rejected,
    /// Requires an explicit semantic resolution.
    Conflict,
    /// Replaced by equivalent newer intent under a certified profile.
    Superseded,
}

/// Advisory operation-state notification. Durable state remains queryable.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct OperationUpdate {
    /// Operation whose state changed.
    pub operation_id: OperationId,
    /// Latest observed high-level state.
    pub state: OperationState,
}

/// Stable reference to an application entity involved in a conflict.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct EntityRef {
    /// Application-defined entity kind.
    pub kind: u32,
    /// Stable entity identity.
    pub entity_id: EntityId,
}

/// Failure while creating or validating a public operation contract.
#[derive(Clone, Copy, Debug, Error, Eq, PartialEq)]
pub enum OperationContractError {
    /// Operation kind zero is reserved.
    #[error("operation kind zero is reserved")]
    ReservedKind,
    /// Operation schema version zero is reserved.
    #[error("operation schema version zero is reserved")]
    ReservedSchemaVersion,
}

/// Bounded failure returned by an application payload encoder.
#[derive(Clone, Debug, Error, Eq, PartialEq)]
#[error("operation payload encoding failed: {message}")]
pub struct OperationEncodingError {
    message: String,
}

impl OperationEncodingError {
    /// Creates a redacted encoding error.
    #[must_use]
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct RenameStudent;

    impl Operation for RenameStudent {
        type Outcome = ();

        const KIND: OperationKind = OperationKind(1002);
        const SCHEMA_VERSION: OperationSchemaVersion = OperationSchemaVersion(1);

        fn encode(&self) -> Result<Vec<u8>, OperationEncodingError> {
            Ok(b"redacted-example".to_vec())
        }
    }

    #[test]
    fn typed_operation_becomes_opaque_durable_intent() {
        let encoded = EncodedOperation::from_operation(&RenameStudent).unwrap_or_else(|error| {
            panic!("example operation should encode: {error}");
        });
        assert_eq!(encoded.kind().get(), 1002);
        assert_eq!(encoded.schema_version().get(), 1);
        assert_eq!(encoded.payload(), b"redacted-example");
    }

    #[test]
    fn reserved_registry_values_are_rejected() {
        assert_eq!(
            OperationKind::new(0),
            Err(OperationContractError::ReservedKind)
        );
        assert_eq!(
            OperationSchemaVersion::new(0),
            Err(OperationContractError::ReservedSchemaVersion)
        );
    }
}
