//! Logical adapter records. These are semantic models, not database rows.

use aequora_types::{
    Cursor, EntityRef, EntityVersion, EventId, OperationId, Sequence, SnapshotId, SyncScopeId,
    TenantId,
};
use serde::{Deserialize, Serialize};

/// Canonical 256-bit payload or artifact digest.
pub type Digest = [u8; 32];

/// Monotonic order assigned when local intent becomes durable.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
pub struct LocalOperationSequence(pub u64);

/// Durable outbox lifecycle.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[non_exhaustive]
pub enum OutboxState {
    /// Eligible for a bounded claim.
    Pending,
    /// Owned by one claim until its deadline.
    InFlight,
    /// Accepted by the authority.
    Accepted,
    /// Permanently business-rejected.
    Rejected,
    /// Requires semantic conflict resolution.
    Conflict,
}

/// Canonical local operation record.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct OutboxRecord {
    /// Permanent idempotency key.
    pub operation_id: OperationId,
    /// Local durable enqueue order.
    pub local_sequence: LocalOperationSequence,
    /// Current durable lifecycle state.
    pub state: OutboxState,
    /// Canonical operation bytes, normally Postcard encoded.
    pub payload: Vec<u8>,
    /// Digest of the canonical operation bytes.
    pub payload_digest: Digest,
}

/// Opaque application mutation that shares a physical transaction with adapter metadata.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct DomainMutation {
    /// Application-controlled stable mutation kind.
    pub kind: u32,
    /// Application-owned canonical mutation bytes.
    pub payload: Vec<u8>,
}

/// Opaque authoritative business mutation with a version precondition.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct AuthorityMutation {
    /// Tenant owning the mutated entity.
    pub tenant_id: TenantId,
    /// Entity selected by the deterministic execution plan.
    pub entity: EntityRef,
    /// Expected version; `None` means creation.
    pub expected_version: Option<EntityVersion>,
    /// Application-owned canonical mutation bytes.
    pub payload: Vec<u8>,
}

/// Logical journal event independent from physical sequence allocation.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct JournalRecord {
    /// Stable event identity.
    pub event_id: EventId,
    /// Tenant owning the event.
    pub tenant_id: TenantId,
    /// Scope whose cursor consumes the event.
    pub scope_id: SyncScopeId,
    /// Committed logical order.
    pub sequence: Sequence,
    /// Canonical event bytes.
    pub payload: Vec<u8>,
    /// Digest of `payload`.
    pub payload_digest: Digest,
}

/// Canonical operation ledger outcome.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct LedgerRecord {
    /// Permanent idempotency key.
    pub operation_id: OperationId,
    /// Digest that detects illegal identifier reuse.
    pub payload_digest: Digest,
    /// Canonical terminal outcome bytes.
    pub outcome: Vec<u8>,
}

/// Payload-free audit metadata required by an authoritative transaction.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct AuditRecord {
    /// Tenant owning the audit evidence.
    pub tenant_id: TenantId,
    /// Operation whose effect is being recorded.
    pub operation_id: OperationId,
    /// Stable application-controlled event kind.
    pub kind: u32,
    /// Digest of any separately governed detail.
    pub detail_digest: Digest,
}

/// Immutable snapshot generation identifier.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
pub struct SnapshotGeneration(pub u64);

/// Logical snapshot publication manifest.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct SnapshotManifest {
    /// Stable snapshot identity.
    pub snapshot_id: SnapshotId,
    /// Immutable generation.
    pub generation: SnapshotGeneration,
    /// Cursor captured at the same consistent boundary.
    pub cursor: Cursor,
    /// Number of immutable chunks.
    pub chunk_count: u32,
    /// Digest over the ordered chunk digest list.
    pub manifest_digest: Digest,
}

/// One bounded immutable snapshot chunk.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct SnapshotChunk {
    /// Snapshot receiving this chunk.
    pub snapshot_id: SnapshotId,
    /// Zero-based chunk offset.
    pub index: u32,
    /// Canonical bounded bytes.
    pub bytes: Vec<u8>,
    /// Digest of `bytes`.
    pub digest: Digest,
}
