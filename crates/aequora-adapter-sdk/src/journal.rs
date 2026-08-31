//! Ordered authoritative journal capability.

use crate::{AdapterError, JournalRecord};
use aequora_types::{Sequence, SyncScopeId, TenantId};
use async_trait::async_trait;

/// Bounded page of journal records in committed logical order.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct JournalPage {
    /// Records strictly after the requested cursor.
    pub records: Vec<JournalRecord>,
    /// Greatest committed sequence represented by this page.
    pub high_watermark: Sequence,
    /// Earliest cursor still available for incremental reads.
    pub retention_floor: Sequence,
}

/// Journal append and range-scan semantics.
#[async_trait]
pub trait JournalStore: Send + Sync {
    /// Appends an event as part of its authoritative transaction.
    async fn append(&self, record: JournalRecord) -> Result<(), AdapterError>;

    /// Scans committed records after `sequence`, bounded by `limit`.
    async fn scan_after(
        &self,
        tenant_id: TenantId,
        scope_id: SyncScopeId,
        sequence: Sequence,
        limit: usize,
    ) -> Result<JournalPage, AdapterError>;

    /// Returns the earliest cursor retained for one scope.
    async fn retention_floor(
        &self,
        tenant_id: TenantId,
        scope_id: SyncScopeId,
    ) -> Result<Sequence, AdapterError>;
}
