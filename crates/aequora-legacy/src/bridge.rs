use crate::{
    CanonicalEntity, LegacyProvenance, LegacyRecordKey, LegacySourcePosition, LegacySystemId,
    MappingError,
};
use aequora_types::{EventId, TenantId};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, sync::Mutex};

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum LegacyChangeKind {
    Insert,
    Update,
    Delete,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct LegacyChange {
    pub provenance: LegacyProvenance,
    pub transaction_marker: Option<Vec<u8>>,
    pub kind: LegacyChangeKind,
    pub payload: Vec<u8>,
    pub source_version: Option<u64>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct LegacyChangeBatch {
    pub changes: Vec<LegacyChange>,
    pub previous_position: Option<LegacySourcePosition>,
    pub final_position: LegacySourcePosition,
}

#[derive(Clone, Debug, Eq, PartialEq, thiserror::Error)]
pub enum CdcError {
    #[error("CDC source failed: {0}")]
    Source(String),
    #[error("source stream has a gap or was reordered")]
    Gap,
    #[error("target is overloaded; source cursor remains unchanged")]
    Backpressure,
    #[error("CDC storage failed")]
    Storage,
    #[error(transparent)]
    Mapping(#[from] MappingError),
}

#[async_trait]
pub trait CdcBridge: Send {
    async fn next_batch(
        &mut self,
        max_items: usize,
        max_bytes: usize,
    ) -> Result<LegacyChangeBatch, CdcError>;
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum BridgeLedgerStatus {
    Applied,
    Quarantined,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct BridgeLedgerEntry {
    pub provenance: LegacyProvenance,
    pub dedup_key: [u8; 32],
    pub canonical_digest: [u8; 32],
    pub status: BridgeLedgerStatus,
    pub applied_event_id: Option<EventId>,
    pub applied_at_unix_ms: u64,
}

impl BridgeLedgerEntry {
    #[must_use]
    pub fn dedup_key(
        system: LegacySystemId,
        position: &LegacySourcePosition,
        key: &LegacyRecordKey,
    ) -> [u8; 32] {
        let mut hasher = blake3::Hasher::new();
        hasher.update(system.as_uuid().as_bytes());
        hasher.update(position.as_bytes());
        hasher.update(key.as_bytes());
        *hasher.finalize().as_bytes()
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct BridgeCursor {
    pub legacy_system_id: LegacySystemId,
    pub stream_id: String,
    pub source_position: LegacySourcePosition,
    pub updated_at_unix_ms: u64,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum LegacyBridgeState {
    Disabled,
    Shadow,
    Following,
    CaughtUp,
    CutoverPending,
    Fenced,
    Retired,
    Failed,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum CatchUpState {
    Historical,
    NearRealtime,
    CaughtUp,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct PollingCursor {
    pub updated_at_unix_ms: u64,
    pub stable_primary_key: LegacyRecordKey,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum DeleteDetection {
    HardDeleteJournal,
    SoftDelete,
    StatusMapping,
    ReconciliationScan,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ReconciliationDigest {
    pub source_count: u64,
    pub target_count: u64,
    pub source_digest: [u8; 32],
    pub target_digest: [u8; 32],
}

impl ReconciliationDigest {
    #[must_use]
    pub fn matches(&self) -> bool {
        self.source_count == self.target_count && self.source_digest == self.target_digest
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct BridgeHealth {
    pub state: LegacyBridgeState,
    pub source_position: Option<LegacySourcePosition>,
    pub applied_position: Option<LegacySourcePosition>,
    pub lag: u64,
    pub last_error_code: Option<String>,
    pub quarantine_count: u64,
    pub catch_up: CatchUpState,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum QuarantineSeverity {
    NonCritical,
    Critical,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct QuarantineRecord {
    pub tenant_id: Option<TenantId>,
    pub provenance: LegacyProvenance,
    pub severity: QuarantineSeverity,
    pub mapping_error: String,
}

/// Transaction boundary required of a physical bridge store.
///
/// Implementations atomically record the canonical result and advance the cursor. A duplicate
/// dedup key returns the original entry without applying another authoritative effect.
#[async_trait]
pub trait BridgeStore: Send + Sync {
    async fn lookup(&self, dedup_key: [u8; 32]) -> Result<Option<BridgeLedgerEntry>, CdcError>;
    async fn record_result_and_advance(
        &self,
        expected_cursor: Option<&LegacySourcePosition>,
        entry: BridgeLedgerEntry,
        next_cursor: BridgeCursor,
    ) -> Result<BridgeLedgerEntry, CdcError>;
    async fn quarantine_and_advance(
        &self,
        expected_cursor: Option<&LegacySourcePosition>,
        record: QuarantineRecord,
        entry: BridgeLedgerEntry,
        next_cursor: BridgeCursor,
    ) -> Result<BridgeLedgerEntry, CdcError>;
}

#[async_trait]
pub trait CanonicalChangeSink: Send + Sync {
    /// Applies one imported effect idempotently under `dedup_key`.
    async fn apply_import(
        &self,
        dedup_key: [u8; 32],
        entity: CanonicalEntity,
        provenance: &LegacyProvenance,
    ) -> Result<EventId, CdcError>;
}

#[derive(Default)]
struct InMemoryBridgeState {
    entries: BTreeMap<[u8; 32], BridgeLedgerEntry>,
    cursors: BTreeMap<(LegacySystemId, String), BridgeCursor>,
    quarantine: Vec<QuarantineRecord>,
}

/// Deterministic reference store for contract tests and embedded migration tooling.
#[derive(Default)]
pub struct InMemoryBridgeStore {
    state: Mutex<InMemoryBridgeState>,
}

impl InMemoryBridgeStore {
    pub fn cursor(
        &self,
        system: LegacySystemId,
        stream: &str,
    ) -> Result<Option<BridgeCursor>, CdcError> {
        let state = self.state.lock().map_err(|_| CdcError::Storage)?;
        Ok(state.cursors.get(&(system, stream.to_owned())).cloned())
    }
}

#[async_trait]
impl BridgeStore for InMemoryBridgeStore {
    async fn lookup(&self, dedup_key: [u8; 32]) -> Result<Option<BridgeLedgerEntry>, CdcError> {
        let state = self.state.lock().map_err(|_| CdcError::Storage)?;
        Ok(state.entries.get(&dedup_key).cloned())
    }

    async fn record_result_and_advance(
        &self,
        expected_cursor: Option<&LegacySourcePosition>,
        entry: BridgeLedgerEntry,
        next_cursor: BridgeCursor,
    ) -> Result<BridgeLedgerEntry, CdcError> {
        let mut state = self.state.lock().map_err(|_| CdcError::Storage)?;
        if let Some(existing) = state.entries.get(&entry.dedup_key) {
            return Ok(existing.clone());
        }
        let cursor_key = (next_cursor.legacy_system_id, next_cursor.stream_id.clone());
        let actual = state
            .cursors
            .get(&cursor_key)
            .map(|cursor| &cursor.source_position);
        if actual != expected_cursor {
            return Err(CdcError::Gap);
        }
        state.entries.insert(entry.dedup_key, entry.clone());
        state.cursors.insert(cursor_key, next_cursor);
        Ok(entry)
    }

    async fn quarantine_and_advance(
        &self,
        expected_cursor: Option<&LegacySourcePosition>,
        record: QuarantineRecord,
        entry: BridgeLedgerEntry,
        next_cursor: BridgeCursor,
    ) -> Result<BridgeLedgerEntry, CdcError> {
        let mut state = self.state.lock().map_err(|_| CdcError::Storage)?;
        if let Some(existing) = state.entries.get(&entry.dedup_key) {
            return Ok(existing.clone());
        }
        let cursor_key = (next_cursor.legacy_system_id, next_cursor.stream_id.clone());
        let actual = state
            .cursors
            .get(&cursor_key)
            .map(|cursor| &cursor.source_position);
        if actual != expected_cursor {
            return Err(CdcError::Gap);
        }
        state.quarantine.push(record);
        state.entries.insert(entry.dedup_key, entry.clone());
        state.cursors.insert(cursor_key, next_cursor);
        Ok(entry)
    }
}
