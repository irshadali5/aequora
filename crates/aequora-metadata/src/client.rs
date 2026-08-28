//! Capability-specific local metadata repositories and transaction composition.

use crate::{
    BootstrapChunkRecord, BootstrapJobId, ConflictRecord, CoordinatorLeaseRecord,
    LocalOperationSeq, MetadataTransactionGroup, OutboxRecord, OutboxState, PersistenceError,
    ScopeCursorRecord, SubscriptionRecord, Timestamp,
};
use aequora_types::{OperationId, Sequence, SyncScopeId};
use async_trait::async_trait;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ClaimBatch {
    pub now: Timestamp,
    pub limit: usize,
    pub maximum_payload_bytes: usize,
    pub fencing_token: Option<u64>,
}

impl ClaimBatch {
    pub fn validate(self) -> Result<(), PersistenceError> {
        if self.limit == 0 || self.maximum_payload_bytes == 0 {
            Err(PersistenceError::LimitExceeded)
        } else {
            Ok(())
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct StateTransition {
    pub operation_id: OperationId,
    pub expected: OutboxState,
    pub next: OutboxState,
    pub fencing_token: Option<u64>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct OutboxReconciliation {
    pub operation_id: OperationId,
    pub expected: OutboxState,
    pub final_state: OutboxState,
    pub committed_sequence: Option<Sequence>,
}

/// Durable ordered local operation queue.
#[async_trait]
pub trait OutboxStore: Send + Sync {
    async fn enqueue(&self, operation: OutboxRecord) -> Result<(), PersistenceError>;
    async fn lookup(
        &self,
        operation: OperationId,
    ) -> Result<Option<OutboxRecord>, PersistenceError>;
    async fn claim_batch(&self, request: ClaimBatch)
    -> Result<Vec<OutboxRecord>, PersistenceError>;
    async fn compare_and_set(&self, transition: StateTransition) -> Result<bool, PersistenceError>;
    async fn mark_retry(
        &self,
        operation: OperationId,
        expected: OutboxState,
        next_retry_at: Timestamp,
        fencing_token: Option<u64>,
    ) -> Result<bool, PersistenceError>;
    async fn reconcile(&self, result: OutboxReconciliation) -> Result<bool, PersistenceError>;
}

#[async_trait]
pub trait ClientScopeStore: Send + Sync {
    async fn cursor(
        &self,
        scope: SyncScopeId,
    ) -> Result<Option<ScopeCursorRecord>, PersistenceError>;
    async fn subscription(
        &self,
        scope: SyncScopeId,
    ) -> Result<Option<SubscriptionRecord>, PersistenceError>;
}

#[async_trait]
pub trait ConflictStore: Send + Sync {
    async fn unresolved(&self, limit: usize) -> Result<Vec<ConflictRecord>, PersistenceError>;
}

#[async_trait]
pub trait BootstrapMetadataStore: Send + Sync {
    async fn job(
        &self,
        id: BootstrapJobId,
    ) -> Result<Option<crate::BootstrapJobRecord>, PersistenceError>;
    async fn chunks(
        &self,
        id: BootstrapJobId,
    ) -> Result<Vec<BootstrapChunkRecord>, PersistenceError>;
}

#[async_trait]
pub trait CoordinatorLeaseStore: Send + Sync {
    async fn current_lease(&self) -> Result<Option<CoordinatorLeaseRecord>, PersistenceError>;
    async fn compare_and_set_lease(
        &self,
        expected_fence: u64,
        next: CoordinatorLeaseRecord,
    ) -> Result<bool, PersistenceError>;
}

/// Common native transaction handle. Adapters may wrap a SQL transaction, KV batch, or embedded
/// transaction without exposing its physical representation.
pub trait TransactionHandle: Send {
    fn group(&self) -> MetadataTransactionGroup;
    fn is_active(&self) -> bool;
}

#[async_trait]
pub trait LocalBusinessWrite: TransactionHandle {
    async fn write_local_business_state(
        &mut self,
        opaque_change: &[u8],
    ) -> Result<(), PersistenceError>;
}

#[async_trait]
pub trait OutboxWrite: TransactionHandle {
    async fn write_outbox(&mut self, record: &OutboxRecord) -> Result<(), PersistenceError>;
}

#[async_trait]
pub trait CursorWrite: TransactionHandle {
    async fn write_cursor(&mut self, cursor: ScopeCursorRecord) -> Result<(), PersistenceError>;
}

#[async_trait]
pub trait ConflictWrite: TransactionHandle {
    async fn write_conflict(&mut self, conflict: &ConflictRecord) -> Result<(), PersistenceError>;
}

/// One native local transaction capable of both required client transaction groups.
pub trait LocalTransaction: LocalBusinessWrite + OutboxWrite + CursorWrite + ConflictWrite {}

impl<T> LocalTransaction for T where
    T: LocalBusinessWrite + OutboxWrite + CursorWrite + ConflictWrite
{
}

#[async_trait]
pub trait LocalUnitOfWork: Send + Sync {
    type Transaction: LocalTransaction;
    async fn begin(
        &self,
        group: MetadataTransactionGroup,
    ) -> Result<Self::Transaction, PersistenceError>;
    async fn commit(&self, transaction: Self::Transaction) -> Result<(), PersistenceError>;
    async fn rollback(&self, transaction: Self::Transaction) -> Result<(), PersistenceError>;
}

/// A bounded startup scan result; normal startup must not full-scan large metadata collections.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ClientStartupStatus {
    pub stale_in_flight: u64,
    pub highest_local_sequence: Option<LocalOperationSeq>,
    pub pending_operations: u64,
    pub cursor_count: u64,
}
