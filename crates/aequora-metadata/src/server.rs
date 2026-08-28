//! Capability-specific authority metadata repositories and transaction composition.

use crate::{
    AuditEventRecord, FieldProvenanceRecord, JobLeaseRecord, JournalFloorRecord, JournalRecord,
    LegalHoldRecord, MetadataTransactionGroup, OperationLedgerRecord, PersistenceError,
    ScopeMembershipRecord, ScopeRegistryRecord, SnapshotChunkRecord, SnapshotLeaseRecord,
    SnapshotRecord, SnapshotState, StorageSurfaceRecord, Timestamp, TransactionHandle,
};
use aequora_types::{
    AuthorityEpoch, EventId, JobId, OperationId, Sequence, SnapshotId, SyncScopeId, TenantId,
};
use async_trait::async_trait;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TenantContext {
    pub tenant_id: TenantId,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct JournalScan {
    pub tenant: TenantContext,
    pub authority_epoch: AuthorityEpoch,
    pub after: Sequence,
    pub limit: usize,
    pub maximum_payload_bytes: usize,
}

#[async_trait]
pub trait JournalStore: Send + Sync {
    async fn append(
        &self,
        transaction: &mut dyn JournalWrite,
        record: &JournalRecord,
    ) -> Result<(), PersistenceError>;
    async fn scan_after(
        &self,
        request: JournalScan,
    ) -> Result<Vec<JournalRecord>, PersistenceError>;
    async fn floor(
        &self,
        tenant: TenantContext,
        scope: SyncScopeId,
    ) -> Result<JournalFloorRecord, PersistenceError>;
    async fn lookup_event(
        &self,
        tenant: TenantContext,
        event: EventId,
    ) -> Result<Option<JournalRecord>, PersistenceError>;
}

#[async_trait]
pub trait OperationLedgerStore: Send + Sync {
    async fn lookup(
        &self,
        tenant: TenantContext,
        operation: OperationId,
    ) -> Result<Option<OperationLedgerRecord>, PersistenceError>;
    async fn insert_result(
        &self,
        transaction: &mut dyn LedgerWrite,
        record: &OperationLedgerRecord,
    ) -> Result<(), PersistenceError>;
}

pub use OperationLedgerStore as LedgerStore;

#[async_trait]
pub trait ScopeStore: Send + Sync {
    async fn descriptor(
        &self,
        tenant: TenantContext,
        scope: SyncScopeId,
    ) -> Result<Option<ScopeRegistryRecord>, PersistenceError>;
    async fn membership_page(
        &self,
        tenant: TenantContext,
        scope: SyncScopeId,
        after_entity: Option<aequora_types::EntityRef>,
        limit: usize,
    ) -> Result<Vec<ScopeMembershipRecord>, PersistenceError>;
}

#[async_trait]
pub trait SnapshotStore: Send + Sync {
    async fn catalog(
        &self,
        tenant: TenantContext,
        snapshot: SnapshotId,
    ) -> Result<Option<SnapshotRecord>, PersistenceError>;
    async fn chunks(
        &self,
        tenant: TenantContext,
        snapshot: SnapshotId,
    ) -> Result<Vec<SnapshotChunkRecord>, PersistenceError>;
    async fn lease(
        &self,
        tenant: TenantContext,
        lease: SnapshotLeaseRecord,
    ) -> Result<(), PersistenceError>;
    async fn publish(
        &self,
        transaction: &mut dyn SnapshotWrite,
        snapshot: &SnapshotRecord,
        chunks: &[SnapshotChunkRecord],
    ) -> Result<(), PersistenceError>;
}

#[async_trait]
pub trait AuditStore: Send + Sync {
    async fn append_audit(
        &self,
        transaction: &mut dyn AuditWrite,
        event: &AuditEventRecord,
    ) -> Result<(), PersistenceError>;
    async fn update_provenance(
        &self,
        transaction: &mut dyn AuditWrite,
        provenance: &[FieldProvenanceRecord],
    ) -> Result<(), PersistenceError>;
}

#[async_trait]
pub trait GovernanceStore: Send + Sync {
    async fn active_holds(
        &self,
        tenant: TenantContext,
    ) -> Result<Vec<LegalHoldRecord>, PersistenceError>;
    async fn storage_surfaces(&self) -> Result<Vec<StorageSurfaceRecord>, PersistenceError>;
}

#[async_trait]
pub trait JobStore: Send + Sync {
    async fn claim(
        &self,
        worker: crate::WorkerId,
        now: Timestamp,
        limit: usize,
    ) -> Result<Vec<crate::JobRecord>, PersistenceError>;
    async fn checkpoint(
        &self,
        job: JobId,
        expected_row_version: u64,
        lease: JobLeaseRecord,
        checkpoint: &[u8],
    ) -> Result<bool, PersistenceError>;
}

#[async_trait]
pub trait BusinessWrite: TransactionHandle {
    async fn write_business_state(&mut self, opaque_change: &[u8]) -> Result<(), PersistenceError>;
}

#[async_trait]
pub trait JournalWrite: TransactionHandle {
    async fn write_journal(&mut self, record: &JournalRecord) -> Result<(), PersistenceError>;
}

#[async_trait]
pub trait LedgerWrite: TransactionHandle {
    async fn write_ledger(
        &mut self,
        record: &OperationLedgerRecord,
    ) -> Result<(), PersistenceError>;
}

#[async_trait]
pub trait AuditWrite: TransactionHandle {
    async fn write_audit(&mut self, record: &AuditEventRecord) -> Result<(), PersistenceError>;
}

#[async_trait]
pub trait SideEffectWrite: TransactionHandle {
    async fn write_side_effect(
        &mut self,
        record: &crate::SideEffectIntentRecord,
    ) -> Result<(), PersistenceError>;
}

#[async_trait]
pub trait SnapshotWrite: TransactionHandle {
    async fn write_snapshot(
        &mut self,
        snapshot: &SnapshotRecord,
        chunks: &[SnapshotChunkRecord],
    ) -> Result<(), PersistenceError>;
}

pub trait AuthoritativeTransaction:
    BusinessWrite + JournalWrite + LedgerWrite + AuditWrite + SideEffectWrite
{
}

impl<T> AuthoritativeTransaction for T where
    T: BusinessWrite + JournalWrite + LedgerWrite + AuditWrite + SideEffectWrite
{
}

#[async_trait]
pub trait AuthoritativeUnitOfWork: Send + Sync {
    type Transaction: AuthoritativeTransaction;
    async fn begin(
        &self,
        group: MetadataTransactionGroup,
    ) -> Result<Self::Transaction, PersistenceError>;
    async fn commit(&self, transaction: Self::Transaction) -> Result<(), PersistenceError>;
    async fn rollback(&self, transaction: Self::Transaction) -> Result<(), PersistenceError>;
}

/// Checks the snapshot publication transaction before an adapter writes it.
pub fn validate_snapshot_publication(
    snapshot: &SnapshotRecord,
    chunks: &[SnapshotChunkRecord],
) -> Result<(), PersistenceError> {
    if snapshot.state != SnapshotState::Published || chunks.is_empty() {
        return Err(PersistenceError::InvalidTransition);
    }
    if chunks.iter().any(|chunk| {
        chunk.snapshot_id != snapshot.snapshot_id
            || chunk.authority_epoch != snapshot.authority_epoch
            || chunk.boundary_sequence != snapshot.boundary_sequence
            || !chunk.durable
            || !chunk.verified
    }) {
        return Err(PersistenceError::ConstraintViolation(
            "published snapshot contains missing, unverified, or cross-boundary chunks".into(),
        ));
    }
    if snapshot.published_at.is_none()
        || snapshot.manifest_digest == [0; 32]
        || snapshot.root_digest == [0; 32]
    {
        return Err(PersistenceError::InvalidRecord(
            "published snapshot lacks publication evidence".into(),
        ));
    }
    Ok(())
}
