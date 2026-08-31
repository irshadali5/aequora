//! Stable extension contracts for storage, transport, identity, and domain providers.
//!
//! Implementors receive semantic requests rather than internal cursors, journal transactions, or
//! framework types. Core cursor ordering, authority, idempotency, and identity semantics are not
//! extension points.

pub mod authority;
pub mod capabilities;
pub mod conformance;
pub mod errors;
pub mod fencing;
pub mod journal;
pub mod ledger;
pub mod local;
pub mod migration;
pub mod object;
pub mod records;
pub mod snapshot;

pub use authority::{
    AuthorityTransaction, AuthorityTransactionStore, DomainRepositoryFactory,
    SupportsAtomicAuthorityCommit,
};
pub use capabilities::{
    AdapterCapabilities, AdapterCapability, AdapterDescriptor, AdapterId, AdapterManifest,
    AdapterRequirements, AdapterRole, AdapterSupport, AdapterVersion, CapabilityId,
    CapabilityLevel, CapabilityVersion, CertifiedEnvironment, ConcurrencyModel, SnapshotLevel,
    StoreKind,
};
pub use errors::{AdapterDiagnostic, RetryDisposition};
pub use fencing::{FencingLease, FencingStore, SupportsFencing};
pub use journal::JournalStore;
pub use ledger::OperationLedgerStore;
pub use local::{
    CursorStore, LocalBackupProvider, LocalTransaction, LocalTransactionStore, OutboxStore,
    SupportsAtomicLocalOutbox,
};
pub use migration::{
    AdapterMigration, AdapterSchemaVersion, DomainSchemaVersion, MigrationHook, MigrationId,
    MigrationPlan, MigrationStore,
};
pub use object::ObjectStore;
pub use records::{
    AuditRecord, AuthorityMutation, Digest, DomainMutation, JournalRecord, LedgerRecord,
    LocalOperationSequence, OutboxRecord, OutboxState, SnapshotChunk, SnapshotGeneration,
    SnapshotManifest,
};
pub use snapshot::{SnapshotStore, SupportsAtomicSnapshotActivation};

use aequora_operation::{EncodedOperation, LocalCommitStatus, OperationKind, OperationState};
use aequora_types::{DeviceId, OperationId, SyncScopeId, TenantId};
use async_trait::async_trait;
use std::{fmt, sync::Arc, time::Duration};
use thiserror::Error;

/// Application registry/domain identity required during client construction.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct DomainId(Arc<str>);

impl DomainId {
    /// Creates a non-empty domain identity.
    ///
    /// # Errors
    ///
    /// Returns [`AdapterError::invalid_configuration`] for an empty or whitespace-only value.
    pub fn new(value: impl Into<Arc<str>>) -> Result<Self, AdapterError> {
        let value = value.into();
        if value.trim().is_empty() {
            Err(AdapterError::invalid_configuration(
                "domain identity cannot be empty",
            ))
        } else {
            Ok(Self(value))
        }
    }

    /// Stable application registry/domain name.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for DomainId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

/// Non-secret credential material consumed by a transport implementation.
#[derive(Clone)]
pub struct Credential(Arc<str>);

impl Credential {
    /// Wraps credential material without making it serializable.
    #[must_use]
    pub fn new(value: impl Into<Arc<str>>) -> Self {
        Self(value.into())
    }

    /// Borrows the credential for a single transport call.
    #[must_use]
    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for Credential {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("Credential([REDACTED])")
    }
}

/// Explicit client identity. Tenant context is never hidden in a global.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ClientIdentity {
    /// Tenant whose data this client accesses.
    pub tenant_id: TenantId,
    /// Stable installation identity.
    pub device_id: DeviceId,
}

/// Stable adapter error category. New categories may be added compatibly.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum AdapterErrorKind {
    /// Adapter or physical store cannot currently be reached.
    Unavailable,
    /// A compare-and-swap or transaction precondition failed.
    Conflict,
    /// The physical store cannot accept additional durable bytes.
    DiskFull,
    /// The physical store is currently read-only.
    ReadOnly,
    /// Stored state failed integrity or decoding checks.
    Corruption,
    /// A logical or physical uniqueness/constraint rule was violated.
    ConstraintViolation,
    /// A serializable transaction must be retried from a deterministic plan.
    SerializationFailure,
    /// A bounded operation timed out.
    Timeout,
    /// Commit may have succeeded, so the idempotency layer must resolve the outcome.
    CommitOutcomeUnknown,
    /// Durable storage is unavailable or rejected a commit.
    Storage,
    /// Transport exchange failed before a semantic response was available.
    Transport,
    /// Credential acquisition or authentication failed.
    Authentication,
    /// Configuration or application input is invalid.
    Validation,
    /// A bounded resource limit rejected work.
    Backpressure,
    /// The configured adapter does not provide the requested capability.
    UnsupportedCapability,
    /// Adapter failed in an unclassified way.
    Internal,
}

/// Stable error crossing a public adapter boundary.
#[derive(Clone, Debug, Error, Eq, PartialEq)]
#[error("{kind:?} adapter failure: {message}")]
pub struct AdapterError {
    kind: AdapterErrorKind,
    message: Arc<str>,
    retry: RetryDisposition,
    diagnostic: Option<AdapterDiagnostic>,
}

impl AdapterError {
    /// Creates an adapter error with a stable category.
    #[must_use]
    pub fn new(kind: AdapterErrorKind, message: impl Into<Arc<str>>) -> Self {
        Self {
            kind,
            message: message.into(),
            retry: RetryDisposition::NonRetryable,
            diagnostic: None,
        }
    }

    /// Creates a configuration validation error.
    #[must_use]
    pub fn invalid_configuration(message: impl Into<Arc<str>>) -> Self {
        Self::new(AdapterErrorKind::Validation, message)
    }

    /// Stable category suitable for caller logic.
    #[must_use]
    pub const fn kind(&self) -> AdapterErrorKind {
        self.kind
    }

    /// Adds the adapter's safe retry classification.
    #[must_use]
    pub const fn with_retry(mut self, retry: RetryDisposition) -> Self {
        self.retry = retry;
        self
    }

    /// Adds payload-free adapter diagnostics without exposing a driver error type.
    #[must_use]
    pub fn with_diagnostic(mut self, diagnostic: AdapterDiagnostic) -> Self {
        self.diagnostic = Some(diagnostic);
        self
    }

    /// Returns the safe retry/recovery action selected by the adapter.
    #[must_use]
    pub const fn retry_disposition(&self) -> RetryDisposition {
        self.retry
    }

    /// Returns optional payload-free physical diagnostics.
    #[must_use]
    pub const fn diagnostic(&self) -> Option<&AdapterDiagnostic> {
        self.diagnostic.as_ref()
    }
}

/// Durable operation snapshot returned by public inspection APIs.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct OperationSnapshot {
    /// Permanent operation identity.
    pub operation_id: OperationId,
    /// Current durable lifecycle state.
    pub state: OperationState,
}

/// High-level durable client status. It contains no cursor or transaction state.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct DurableClientStatus {
    /// Number of durable operations still requiring authoritative disposition.
    pub pending_operations: u64,
    /// Number of unresolved semantic conflicts.
    pub conflicts: u64,
    /// Whether the local store requires snapshot bootstrap.
    pub needs_bootstrap: bool,
    /// Whether storage currently rejects additional durable intent.
    pub storage_blocked: bool,
}

/// Opaque conflict identifier, independent from storage representation.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct ConflictId(OperationId);

impl ConflictId {
    /// Creates an opaque conflict identity from its originating operation.
    #[must_use]
    pub const fn from_operation(operation_id: OperationId) -> Self {
        Self(operation_id)
    }
}

impl fmt::Display for ConflictId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}

/// Public conflict summary without local persistence records.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ConflictSummary {
    /// Stable conflict identity.
    pub conflict_id: ConflictId,
    /// Stable operation kind.
    pub operation_kind: OperationKind,
    /// Redacted application-facing reason.
    pub reason: Arc<str>,
    /// Stable resolution options offered by the domain.
    pub available_resolutions: Vec<Arc<str>>,
}

/// Semantic conflict resolution selected by application code.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ConflictResolution {
    /// Stable domain-defined resolution name.
    pub kind: Arc<str>,
    /// Opaque domain payload whose schema is owned by the application registry.
    pub payload: Vec<u8>,
}

/// High-level result of one bounded adapter exchange.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ExchangeSummary {
    /// Operations accepted or business-rejected by the authority.
    pub pushed: u64,
    /// Authoritative changes durably applied locally.
    pub pulled: u64,
    /// Newly materialized conflicts.
    pub conflicts: u64,
    /// Server-requested bounded retry delay.
    pub retry_after: Option<Duration>,
    /// Whether a fresh bootstrap is required.
    pub needs_bootstrap: bool,
    /// Whether a compatible SDK/protocol upgrade is required.
    pub upgrade_required: bool,
}

/// Input to a public transport. Internal protocol framing remains private.
pub struct ExchangeContext<'a> {
    /// Explicit client identity.
    pub identity: ClientIdentity,
    /// Registry/domain identity.
    pub domain: &'a DomainId,
    /// Short-lived credential borrowed for this exchange.
    pub credential: &'a Credential,
}

/// Stable storage extension point used by the high-level client SDK.
///
/// `commit_operation` is a cancellation boundary: once the adapter makes the intent durable, it
/// must finish the atomic domain-mutation/outbox transition even if the caller drops its future.
/// No method permits external cursor advancement or raw sync-metadata mutation.
#[async_trait]
pub trait ClientStore: Send + Sync {
    /// Atomically persists domain mutation and matching outbox intent.
    async fn commit_operation(
        &self,
        operation: EncodedOperation,
    ) -> Result<LocalCommitStatus, AdapterError>;

    /// Queries one durable operation state.
    async fn operation(
        &self,
        operation_id: OperationId,
    ) -> Result<Option<OperationSnapshot>, AdapterError>;

    /// Queries current durable state so advisory event loss is recoverable.
    async fn status(&self) -> Result<DurableClientStatus, AdapterError>;

    /// Lists unresolved conflicts with a caller-supplied hard bound.
    async fn conflicts(&self, limit: usize) -> Result<Vec<ConflictSummary>, AdapterError>;

    /// Loads one unresolved conflict.
    async fn conflict(
        &self,
        conflict_id: ConflictId,
    ) -> Result<Option<ConflictSummary>, AdapterError>;

    /// Persists a semantic resolution as new intent; it never edits a ledger row directly.
    async fn resolve_conflict(
        &self,
        conflict_id: ConflictId,
        resolution: ConflictResolution,
    ) -> Result<OperationId, AdapterError>;

    /// Requests a scope subscription through durable semantic state.
    async fn subscribe_scope(&self, scope: SyncScopeId) -> Result<(), AdapterError>;

    /// Requests a scope unsubscription without asserting domain deletion.
    async fn unsubscribe_scope(&self, scope: SyncScopeId) -> Result<(), AdapterError>;

    /// Produces a bounded, redacted diagnostic summary.
    async fn diagnostic_summary(&self) -> Result<Arc<str>, AdapterError>;
}

/// Stable transport extension point. HTTP headers and wire framing remain implementation details.
#[async_trait]
pub trait SyncTransport: Send + Sync {
    /// Performs one bounded exchange and commits any received reconciliation before returning.
    async fn exchange(&self, context: ExchangeContext<'_>)
    -> Result<ExchangeSummary, AdapterError>;
}

/// Stable credential extension point.
#[async_trait]
pub trait CredentialProvider: Send + Sync {
    /// Obtains a credential for one synchronization exchange.
    async fn credential(&self) -> Result<Credential, AdapterError>;
}

/// Stable observability hook. Hook failure must never alter synchronization correctness.
pub trait ObservationSink: Send + Sync {
    /// Receives a payload-free status transition.
    fn on_status(&self, status: &'static str);

    /// Receives a bounded diagnostic event.
    fn on_diagnostic(&self, code: &'static str);
}

/// No-op observability sink used by default.
#[derive(Clone, Copy, Debug, Default)]
pub struct NoopObservationSink;

impl ObservationSink for NoopObservationSink {
    fn on_status(&self, _status: &'static str) {}

    fn on_diagnostic(&self, _code: &'static str) {}
}

/// Server-side authority extension point intentionally independent from HTTP and database types.
#[async_trait]
pub trait AuthorityService: Send + Sync {
    /// Atomically commits one already-authenticated, registry-validated domain outcome.
    async fn commit(
        &self,
        identity: ClientIdentity,
        operation: EncodedOperation,
        outcome: Vec<u8>,
    ) -> Result<Vec<u8>, AdapterError>;
}

/// Validated server-side handler context with no transport or database internals.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DomainContext {
    /// Explicit authenticated tenant/device identity.
    pub identity: ClientIdentity,
    /// Permanent operation identity for correlation and idempotency.
    pub operation_id: OperationId,
}

/// Semantic domain outcome, distinct from transport and storage failure.
#[derive(Clone, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum DomainOutcome {
    /// Handler accepted the operation and produced an opaque application outcome.
    Accepted(Vec<u8>),
    /// Business rules rejected the operation without breaking synchronization.
    BusinessRejected {
        /// Stable application-defined reason code.
        code: Arc<str>,
    },
    /// Handler requires semantic conflict resolution.
    Conflict {
        /// Stable application-defined conflict reason.
        reason: Arc<str>,
    },
}

/// Open, storage-neutral domain-handler extension point.
#[async_trait]
pub trait DomainHandler: Send + Sync {
    /// Stable operation kind owned by this handler.
    fn operation_kind(&self) -> OperationKind;

    /// Stable operation payload schema accepted by this handler.
    fn schema_version(&self) -> u16;

    /// Certified consistency/profile name. Missing profiles are rejected during registration.
    fn profile(&self) -> Option<&'static str>;

    /// Validates and executes one opaque operation payload using explicit context.
    async fn handle(
        &self,
        context: DomainContext,
        payload: &[u8],
    ) -> Result<DomainOutcome, AdapterError>;
}

/// Server authentication extension point. Transports adapt their credentials into this boundary.
#[async_trait]
pub trait Authenticator: Send + Sync {
    /// Authenticates a credential and returns explicit tenant/device identity.
    async fn authenticate(&self, credential: &Credential) -> Result<ClientIdentity, AdapterError>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn credentials_are_always_redacted_in_debug() {
        assert_eq!(
            format!("{:?}", Credential::new("secret")),
            "Credential([REDACTED])"
        );
    }

    #[test]
    fn domain_identity_is_explicit_and_nonempty() {
        assert!(DomainId::new("school").is_ok());
        assert_eq!(
            DomainId::new("  ").map(|domain| domain.to_string()),
            Err(AdapterError::invalid_configuration(
                "domain identity cannot be empty"
            ))
        );
    }
}
