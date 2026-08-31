//! Local transaction, outbox, cursor, and backup capabilities.

use crate::{AdapterError, DomainMutation, OutboxRecord, SnapshotManifest};
use aequora_types::{Cursor, OperationId, SyncScopeId};
use async_trait::async_trait;
use std::time::Duration;

/// Transaction that couples application mutation and Aequora outbox intent.
#[async_trait]
pub trait LocalTransaction: Send {
    /// Applies one application-owned mutation inside this physical transaction.
    async fn apply_domain_mutation(&mut self, mutation: DomainMutation)
    -> Result<(), AdapterError>;

    /// Inserts the matching canonical outbox record in the same transaction.
    async fn insert_outbox(&mut self, record: OutboxRecord) -> Result<(), AdapterError>;

    /// Atomically publishes all staged writes.
    async fn commit(self) -> Result<(), AdapterError>;

    /// Discards all staged writes.
    async fn rollback(self) -> Result<(), AdapterError>;
}

/// Factory for local transactions without exposing a database transaction type to core.
#[async_trait]
pub trait LocalTransactionStore: Send + Sync {
    /// Concrete transaction retained inside the application adapter layer.
    type Tx<'a>: LocalTransaction + Send
    where
        Self: 'a;

    /// Begins one atomic local domain/outbox transaction.
    async fn begin_local_tx(&self) -> Result<Self::Tx<'_>, AdapterError>;
}

/// Compile-time composition marker for atomic local intent.
///
/// Runtime conformance is still mandatory because a marker cannot prove physical durability.
pub trait SupportsAtomicLocalOutbox: LocalTransactionStore {}

/// Durable outbox claiming and disposition.
#[async_trait]
pub trait OutboxStore: Send + Sync {
    /// Inserts a canonical operation record.
    async fn enqueue(&self, record: OutboxRecord) -> Result<(), AdapterError>;

    /// Claims at most `limit` eligible records for a bounded duration.
    async fn claim_batch(
        &self,
        limit: usize,
        claim_for: Duration,
    ) -> Result<Vec<OutboxRecord>, AdapterError>;

    /// Marks an operation accepted by the authority.
    async fn mark_accepted(&self, operation_id: OperationId) -> Result<(), AdapterError>;

    /// Marks an operation permanently rejected without deleting its evidence.
    async fn mark_rejected(
        &self,
        operation_id: OperationId,
        reason_code: &str,
    ) -> Result<(), AdapterError>;

    /// Releases expired claims back to eligible state.
    async fn release_stale_claims(&self) -> Result<u64, AdapterError>;

    /// Looks up one operation by permanent identity.
    async fn operation(
        &self,
        operation_id: OperationId,
    ) -> Result<Option<OutboxRecord>, AdapterError>;
}

/// Cursor reads and compare/update semantics.
#[async_trait]
pub trait CursorStore: Send + Sync {
    /// Reads the currently committed cursor for one scope.
    async fn cursor(&self, scope: SyncScopeId) -> Result<Option<Cursor>, AdapterError>;

    /// Advances from `expected` to `next` only as part of durable reconcile.
    async fn compare_and_reconcile(
        &self,
        expected: Option<Cursor>,
        next: Cursor,
        canonical_changes: &[u8],
    ) -> Result<(), AdapterError>;
}

/// Optional consistent local backup integration.
#[async_trait]
pub trait LocalBackupProvider: Send + Sync {
    /// Produces a consistent immutable backup manifest.
    async fn create_backup(&self) -> Result<SnapshotManifest, AdapterError>;

    /// Restores a verified backup into staging; identity/schema checks run before activation.
    async fn restore_backup(&self, manifest: &SnapshotManifest) -> Result<(), AdapterError>;
}
