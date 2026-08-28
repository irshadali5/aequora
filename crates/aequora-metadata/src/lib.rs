//! Canonical logical persistence contracts for Aequora synchronization metadata.
//!
//! This crate deliberately contains no SQL, storage-engine, runtime, or ORM dependency.  It
//! defines the records, identities, ordering rules, transaction boundaries, and adapter-facing
//! capability traits that `PostgreSQL`, Stoolap, `SQLite`, KV, and other stores must implement with
//! equivalent semantics.

#![allow(clippy::missing_errors_doc)]

mod client;
mod export;
mod indexes;
mod invariants;
mod migrations;
mod records;
mod server;
mod version;

pub use client::*;
pub use export::*;
pub use indexes::*;
pub use invariants::*;
pub use migrations::*;
pub use records::*;
pub use server::*;
pub use version::*;

use thiserror::Error;

/// Current logical metadata format.  It is independent of protocol and application schema.
pub const CURRENT_METADATA_SCHEMA_VERSION: MetadataSchemaVersion =
    MetadataSchemaVersion::new_const(1);

/// Classification used by adapters when deciding whether a failed operation may be retried.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RetryClass {
    /// The operation did not commit and may safely be attempted again.
    Retryable,
    /// The operation is known not to have committed and requires caller action.
    NonRetryable,
    /// The response was lost or commit status cannot be proven; use the operation identity to
    /// reconcile before attempting a new logical effect.
    Ambiguous,
}

/// Database-neutral persistence failures shared by all metadata repositories.
#[derive(Clone, Debug, Eq, Error, PartialEq)]
pub enum PersistenceError {
    #[error("metadata constraint violation: {0}")]
    ConstraintViolation(String),
    #[error("metadata optimistic version conflict")]
    VersionConflict,
    #[error("operation already exists in the metadata ledger")]
    DuplicateOperation,
    #[error("operation payload digest differs from the retained digest")]
    PayloadDigestMismatch,
    #[error("metadata storage is unavailable: {0}")]
    StorageUnavailable(String),
    #[error("metadata transaction was aborted")]
    TransactionAborted,
    #[error("metadata migration is required")]
    MigrationRequired,
    #[error("metadata schema is newer than this binary supports")]
    SchemaTooNew,
    #[error("metadata corruption detected: {0}")]
    CorruptionDetected(String),
    #[error("metadata state transition is invalid")]
    InvalidTransition,
    #[error("metadata record is invalid: {0}")]
    InvalidRecord(String),
    #[error("metadata resource limit exceeded")]
    LimitExceeded,
}

impl PersistenceError {
    /// Returns the recovery classification required by the persistence contract.
    #[must_use]
    pub const fn retry_class(&self) -> RetryClass {
        match self {
            Self::StorageUnavailable(_) | Self::TransactionAborted => RetryClass::Retryable,
            Self::CorruptionDetected(_)
            | Self::SchemaTooNew
            | Self::MigrationRequired
            | Self::PayloadDigestMismatch
            | Self::DuplicateOperation
            | Self::ConstraintViolation(_)
            | Self::VersionConflict
            | Self::InvalidTransition
            | Self::InvalidRecord(_)
            | Self::LimitExceeded => RetryClass::NonRetryable,
        }
    }
}

/// Outcome of a commit whose response may have been lost after the native transaction boundary.
#[derive(Debug, Eq, PartialEq)]
pub enum CommitResult<T> {
    /// The transaction definitely committed.
    Committed(T),
    /// The transaction definitely did not commit.
    NotCommitted(PersistenceError),
    /// The adapter cannot determine the outcome; operation-ledger lookup is authoritative.
    Ambiguous(PersistenceError),
}

/// The transaction groups mandated by Part 22.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MetadataTransactionGroup {
    /// Local domain mutation and outbox insertion.
    ClientMutationAndOutbox,
    /// Event apply, entity/version state, membership, cursor, and reconciliation outcome.
    ClientReconciliation,
    /// Business mutation, journal, ledger, audit, and side-effect intent.
    AuthoritativeCommit,
    /// Authority epoch and write-fence transition.
    AuthorityTransition,
    /// Scope descriptor, generation, and policy publication.
    ScopePublication,
    /// Snapshot chunks, manifest, and verified publication state.
    SnapshotPublication,
    /// Legal hold and required audit evidence.
    LegalHold,
    /// Key registry transition and required audit evidence.
    KeyRotation,
}

/// A bounded canonical digest for opaque durable metadata payloads.
#[must_use]
pub fn payload_digest(payload: &[u8]) -> [u8; 32] {
    *blake3::hash(payload).as_bytes()
}
