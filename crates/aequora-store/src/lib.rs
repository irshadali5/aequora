//! Capability-based persistence contracts.
//!
//! Implementations must preserve the atomicity requirements on each method. No database
//! transaction type appears in this public API.

use aequora_executor::CurrentEntity;
use aequora_protocol::{
    BootstrapResponse, OperationAck, OperationEnvelope, Partition, RemoteChange, SnapshotEntity,
    SyncResponse,
};
use aequora_types::{
    ActorId, Cursor, DeviceId, EntityRef, EntityVersion, HybridTimestamp, OperationId, Sequence,
    SnapshotId, SyncScopeId, TenantId,
};
use async_trait::async_trait;
use thiserror::Error;

/// Stored authoritative entity state.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EntitySnapshot {
    /// Entity identity.
    pub entity: EntityRef,
    /// Tenant that owns the entity.
    pub tenant_id: TenantId,
    /// Current application snapshot.
    pub current: CurrentEntity,
}

/// Atomic authoritative commit requested after validation and execution.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CommitOperation {
    /// Original permanent operation ID.
    pub operation_id: OperationId,
    /// Authenticated actor responsible for the command.
    pub actor_id: ActorId,
    /// Authenticated device from which the command originated.
    pub device_id: DeviceId,
    /// Registered application operation kind.
    pub operation_kind: u16,
    /// Tenant owning the effect.
    pub tenant_id: TenantId,
    /// Journal scope.
    pub scope_id: SyncScopeId,
    /// Target entity.
    pub entity: EntityRef,
    /// Version observed before application. The store must compare it atomically.
    pub expected_version: Option<EntityVersion>,
    /// Resulting version selected by the server.
    pub next_version: EntityVersion,
    /// Authoritative state transition payload.
    pub payload: Vec<u8>,
    /// Upsert or tombstone.
    pub change_kind: aequora_protocol::ChangeKind,
    /// Event timestamp.
    pub timestamp: HybridTimestamp,
    /// BLAKE3 digest of the command payload. The audit log never stores the command payload.
    pub command_digest: [u8; 32],
}

/// Monotonic position in the immutable accountability log.
#[derive(Clone, Copy, Debug, Default, Eq, Ord, PartialEq, PartialOrd)]
pub struct AuditOffset(pub u64);

/// Payload-free immutable evidence of one committed authoritative command.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AuditRecord {
    /// Monotonic audit position, independent of synchronization cursors.
    pub offset: AuditOffset,
    /// Authenticated tenant boundary.
    pub tenant_id: TenantId,
    /// Permanent idempotency key.
    pub operation_id: OperationId,
    /// Authenticated actor responsible for the command.
    pub actor_id: ActorId,
    /// Authenticated originating device.
    pub device_id: DeviceId,
    /// Registered application command kind.
    pub operation_kind: u16,
    /// Entity affected by the authoritative transition.
    pub entity: EntityRef,
    /// Resulting authoritative version.
    pub entity_version: EntityVersion,
    /// BLAKE3 command digest for accountability without retaining sensitive payloads.
    pub command_digest: [u8; 32],
    /// Authoritative commit timestamp.
    pub timestamp: HybridTimestamp,
}

/// One bounded immutable audit page.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AuditPage {
    /// Records ordered by ascending audit offset.
    pub records: Vec<AuditRecord>,
    /// Last returned offset, or the input offset for an empty page.
    pub next_offset: AuditOffset,
    /// Whether another retained record follows this page.
    pub has_more: bool,
}

/// Result of an authoritative atomic commit.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CommitOutcome {
    /// This operation was newly applied.
    Applied(OperationAck),
    /// The operation already existed; the stored deterministic result is returned.
    Duplicate(OperationAck),
    /// State changed after validation. Nothing was committed.
    VersionChanged { current: Option<EntityVersion> },
}

/// One bounded page of authoritative journal entries.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ChangePage {
    /// Ordered events after the requested sequence.
    pub changes: Vec<RemoteChange>,
    /// Greatest complete sequence represented by this page.
    pub next_sequence: Sequence,
    /// Whether more events remain.
    pub has_more: bool,
}

/// Identity and journal boundary of a newly captured consistent snapshot.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SnapshotDescriptor {
    /// Stable identity used to resume pages.
    pub snapshot_id: SnapshotId,
    /// Incremental synchronization starts strictly after this cursor.
    pub cursor: Cursor,
}

/// Bounded page read from an immutable consistent snapshot.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SnapshotPage {
    /// Identity and cursor shared by every page.
    pub descriptor: SnapshotDescriptor,
    /// Snapshot entities beginning at the requested offset.
    pub entities: Vec<SnapshotEntity>,
    /// Offset for the next page.
    pub next_offset: u64,
    /// True when another page remains.
    pub has_more: bool,
}

/// Persistence failure at a library boundary.
#[derive(Clone, Debug, Error, Eq, PartialEq)]
#[error("storage {kind:?}: {message}")]
pub struct StoreError {
    /// Stable failure category used to decide retry behavior.
    pub kind: StoreErrorKind,
    /// Non-sensitive implementation explanation.
    pub message: String,
}

impl StoreError {
    /// Creates a transient storage failure.
    #[must_use]
    pub fn transient(message: impl Into<String>) -> Self {
        Self {
            kind: StoreErrorKind::Transient,
            message: message.into(),
        }
    }

    /// Creates a permanent/corruption storage failure.
    #[must_use]
    pub fn permanent(message: impl Into<String>) -> Self {
        Self {
            kind: StoreErrorKind::Permanent,
            message: message.into(),
        }
    }
}

/// Retry significance of a persistence error.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StoreErrorKind {
    /// Operation may succeed unchanged later.
    Transient,
    /// Operation requires intervention or repair.
    Permanent,
}

/// Durable lifecycle of one local outbox operation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OutboxState {
    /// Appended atomically with the optimistic local mutation.
    Pending,
    /// Selected for an exchange. A restart must treat this state as replayable.
    Sending,
    /// The server acknowledged the authoritative logical effect.
    Acknowledged,
    /// The server permanently rejected the operation.
    Rejected,
    /// The operation requires application conflict handling.
    Conflict,
    /// Delivery or reconciliation failed before a terminal result was durable.
    Retry,
}

impl OutboxState {
    /// Returns whether a restart or later synchronization may safely resubmit the operation.
    #[must_use]
    pub const fn is_replayable(self) -> bool {
        matches!(self, Self::Pending | Self::Sending | Self::Retry)
    }

    /// Returns whether the server supplied a durable terminal result.
    #[must_use]
    pub const fn is_terminal(self) -> bool {
        matches!(self, Self::Acknowledged | Self::Rejected | Self::Conflict)
    }
}

/// Application decision recorded for a manual conflict.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ConflictResolution {
    /// Keep the current authoritative state and abandon the optimistic operation.
    AcceptServer,
    /// A new domain operation supersedes the conflicted operation.
    SupersededBy(OperationId),
}

/// Conflict inbox entry with optional application resolution.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ConflictRecord {
    /// Server-provided conflict details.
    pub conflict: aequora_protocol::Conflict,
    /// Durable application decision, or `None` while unresolved.
    pub resolution: Option<ConflictResolution>,
}

/// Durable client queue statistics for UI status, metrics, and backpressure.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct OutboxStats {
    /// Operations not yet selected for delivery.
    pub pending: usize,
    /// Operations selected before a possible process restart.
    pub sending: usize,
    /// Operations awaiting replay after a failed attempt.
    pub retry: usize,
    /// Oldest replayable operation timestamp.
    pub oldest_pending_at: Option<HybridTimestamp>,
}

impl OutboxStats {
    /// Total replayable queue depth.
    #[must_use]
    pub const fn replayable(self) -> usize {
        self.pending
            .saturating_add(self.sending)
            .saturating_add(self.retry)
    }
}

/// Read capability for authoritative entities.
#[async_trait]
pub trait EntityReader: Send + Sync {
    /// Reads one entity inside its tenant boundary.
    async fn read_entity(
        &self,
        tenant: TenantId,
        entity: EntityRef,
    ) -> Result<Option<EntitySnapshot>, StoreError>;
}

/// Authoritative journal pull capability.
#[async_trait]
pub trait ChangeJournal: Send + Sync {
    /// Oldest cursor from which retained incremental history remains complete.
    async fn minimum_retained_cursor(
        &self,
        tenant: TenantId,
        scope: SyncScopeId,
    ) -> Result<Sequence, StoreError>;

    /// Reads events after `sequence`, ordered ascending and restricted to tenant/scope.
    async fn read_changes_after(
        &self,
        tenant: TenantId,
        scope: SyncScopeId,
        sequence: Sequence,
        limit: usize,
        max_payload_bytes: usize,
    ) -> Result<ChangePage, StoreError>;
}

/// Explicit synchronization-journal compaction capability.
#[async_trait]
pub trait JournalCompactor: Send + Sync {
    /// Removes sync events at or below a separately planned safe boundary.
    /// Operation-ledger and audit records must remain unaffected.
    async fn compact_journal(
        &self,
        tenant: TenantId,
        scope: SyncScopeId,
        through: Sequence,
    ) -> Result<u64, StoreError>;
}

/// Atomic authoritative mutation, journal, and idempotency-ledger capability.
#[async_trait]
pub trait OperationLedger: Send + Sync {
    /// Returns a prior logical result, if any.
    async fn operation_result(
        &self,
        tenant: TenantId,
        operation_id: OperationId,
    ) -> Result<Option<OperationAck>, StoreError>;

    /// Atomically compares the expected version, mutates entity state, appends exactly one
    /// journal entry, and records the operation result. A repeated ID returns `Duplicate`.
    async fn commit_operation(&self, commit: CommitOperation) -> Result<CommitOutcome, StoreError>;
}

/// Immutable accountability capability, deliberately separate from the compactable sync journal.
#[async_trait]
pub trait AuditLog: Send + Sync {
    /// Reads tenant-bounded records strictly after `offset` in commit order.
    async fn read_audit_after(
        &self,
        tenant: TenantId,
        offset: AuditOffset,
        limit: usize,
    ) -> Result<AuditPage, StoreError>;
}

/// Consistent, resumable authoritative snapshot capability.
#[async_trait]
pub trait SnapshotStore: Send + Sync {
    /// Atomically captures a snapshot boundary and immutable entity view for a partial scope.
    async fn create_snapshot(
        &self,
        tenant: TenantId,
        scope: SyncScopeId,
        partitions: &[Partition],
    ) -> Result<SnapshotDescriptor, StoreError>;

    /// Reads a bounded page from a previously captured snapshot.
    async fn read_snapshot(
        &self,
        tenant: TenantId,
        snapshot_id: SnapshotId,
        offset: u64,
        max_entities: usize,
        max_payload_bytes: usize,
    ) -> Result<SnapshotPage, StoreError>;
}

/// Storage needed by an authoritative server.
pub trait AuthoritativeStore:
    EntityReader + ChangeJournal + OperationLedger + SnapshotStore + AuditLog
{
}
impl<T> AuthoritativeStore for T where
    T: EntityReader + ChangeJournal + OperationLedger + SnapshotStore + AuditLog
{
}

/// Durable client outbox capability.
#[async_trait]
pub trait OutboxStore: Send + Sync {
    /// Returns pending operations in stable enqueue order, bounded by `limit`.
    async fn pending_operations(&self, limit: usize) -> Result<Vec<OperationEnvelope>, StoreError>;

    /// Appends an operation. Real local adapters should call this within the same database
    /// transaction as the optimistic domain mutation.
    async fn append_operation(&self, operation: OperationEnvelope) -> Result<(), StoreError>;
}

/// Durable outbox state-machine capability.
#[async_trait]
pub trait OutboxStateStore: Send + Sync {
    /// Marks selected operations as in flight before network I/O begins.
    async fn mark_sending(&self, operations: &[OperationId]) -> Result<(), StoreError>;

    /// Marks operations replayable after delivery or local reconciliation fails.
    async fn mark_retry(&self, operations: &[OperationId]) -> Result<(), StoreError>;

    /// Loads the last durable state for application status and crash recovery.
    async fn operation_state(
        &self,
        operation: OperationId,
    ) -> Result<Option<OutboxState>, StoreError>;

    /// Returns durable queue depth and oldest replayable timestamp.
    async fn outbox_stats(&self) -> Result<OutboxStats, StoreError>;
}

/// Durable manual-conflict inbox independent of any UI framework.
#[async_trait]
pub trait ConflictInbox: Send + Sync {
    /// Returns unresolved conflicts in stable persistence order.
    async fn unresolved_conflicts(&self, limit: usize) -> Result<Vec<ConflictRecord>, StoreError>;

    /// Returns the durable unresolved-conflict count without loading details.
    async fn unresolved_conflict_count(&self) -> Result<usize, StoreError>;

    /// Records an application decision without inventing an authoritative mutation.
    async fn resolve_conflict(
        &self,
        operation: OperationId,
        resolution: ConflictResolution,
    ) -> Result<(), StoreError>;
}

/// Durable client cursor capability.
#[async_trait]
pub trait CursorStore: Send + Sync {
    /// Loads the last durably reconciled cursor for a scope.
    async fn load_cursor(&self, scope: SyncScopeId) -> Result<Option<Cursor>, StoreError>;
}

/// Atomic client reconciliation capability.
#[async_trait]
pub trait ReconciliationStore: Send + Sync {
    /// Atomically applies authoritative changes/tombstones, persists conflicts/rejections,
    /// acknowledges outbox operations, and advances the cursor last.
    async fn reconcile(&self, response: &SyncResponse) -> Result<(), StoreError>;

    /// Durably stages a snapshot page. When `has_more` is false, the implementation must
    /// atomically replace the requested scope, set the snapshot cursor, and clear staging.
    async fn stage_snapshot(&self, response: &BootstrapResponse) -> Result<(), StoreError>;

    /// Returns a staged snapshot so bootstrap can resume after a process crash.
    async fn snapshot_progress(
        &self,
        scope: SyncScopeId,
    ) -> Result<Option<SnapshotProgress>, StoreError>;
}

/// Durable client progress for an incomplete bootstrap snapshot.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SnapshotProgress {
    /// Snapshot being staged.
    pub snapshot_id: SnapshotId,
    /// Snapshot boundary that will become the scope cursor at final commit.
    pub cursor: Cursor,
    /// Offset to request next.
    pub next_offset: u64,
}

/// Storage needed by a local client engine.
pub trait LocalStore:
    OutboxStore + OutboxStateStore + ConflictInbox + CursorStore + ReconciliationStore
{
}
impl<T> LocalStore for T where
    T: OutboxStore + OutboxStateStore + ConflictInbox + CursorStore + ReconciliationStore
{
}
