//! Deterministic, database-free stores and transport for synchronization tests.

pub mod contracts;

use aequora_client::{ClientError, ClientSyncEngine, SyncOutcome};
use aequora_executor::{
    AuthContext, AuthenticatedOperation, AuthoritativeMutation, AuthorizedOperation, CurrentEntity,
    ExecutableOperation, ExecutionError, OperationExecutor,
};
use aequora_protocol::{
    BootstrapRequest, BootstrapResponse, Conflict, OperationAck, OperationEnvelope,
    OperationRejection, Partition, RemoteChange, SnapshotEntity, SyncRequest, SyncResponse,
};
use aequora_server::{ExchangeService, ServerError};
use aequora_store::{
    AuditLog, AuditOffset, AuditPage, AuditRecord, ChangeJournal, ChangePage, CommitOperation,
    CommitOutcome, ConflictInbox, ConflictRecord, ConflictResolution, CursorStore, EntityReader,
    EntitySnapshot, JournalCompactor, OperationLedger, OutboxState, OutboxStateStore, OutboxStats,
    OutboxStore, ReconciliationStore, SnapshotDescriptor, SnapshotPage, SnapshotProgress,
    SnapshotStore, StoreError, StoreErrorKind,
};
use aequora_transport::{
    SnapshotPageStream, StreamingSyncTransport, SyncTransport, TransportError,
};
use aequora_types::{Cursor, EntityRef, OperationId, Sequence, SnapshotId, SyncScopeId, TenantId};
use async_trait::async_trait;
use std::{
    collections::{HashMap, HashSet, VecDeque},
    sync::{
        Arc, Mutex, MutexGuard,
        atomic::{AtomicUsize, Ordering},
    },
};

type EntityKey = (TenantId, EntityRef);
type OperationKey = (TenantId, OperationId);
type ScopeKey = (TenantId, SyncScopeId);

/// Deterministic step-by-step client simulator. Compose its transport with the fault wrappers
/// in this crate to model offline periods, lost responses, and retries without real networking.
pub struct SyncSimulator<L, T> {
    engine: ClientSyncEngine<L, T>,
    history: Vec<SyncOutcome>,
}

impl<L, T> SyncSimulator<L, T> {
    /// Creates a simulator around a configured client engine.
    #[must_use]
    pub const fn new(engine: ClientSyncEngine<L, T>) -> Self {
        Self {
            engine,
            history: Vec::new(),
        }
    }

    /// Borrows the client engine and its local store.
    #[must_use]
    pub const fn engine(&self) -> &ClientSyncEngine<L, T> {
        &self.engine
    }

    /// Successful exchange outcomes in simulation order.
    #[must_use]
    pub fn history(&self) -> &[SyncOutcome] {
        &self.history
    }
}

impl<L, T> SyncSimulator<L, T>
where
    L: aequora_store::LocalStore,
    T: SyncTransport,
{
    /// Runs one retry-aware deterministic simulation step.
    ///
    /// # Errors
    ///
    /// Returns [`ClientError`] when the configured fault remains after retry exhaustion or
    /// when reconciliation/protocol validation fails.
    pub async fn step(&mut self) -> Result<SyncOutcome, ClientError> {
        let outcome = self.engine.run_with_retry().await?;
        self.history.push(outcome);
        Ok(outcome)
    }
}

/// Test executor that authorizes every operation and copies its payload into an upsert event.
/// It is suitable only for protocol/infrastructure tests, never production authorization.
#[derive(Clone, Copy, Debug, Default)]
pub struct AllowAllExecutor;

#[async_trait]
impl OperationExecutor for AllowAllExecutor {
    async fn authorize_scope(
        &self,
        _auth: &AuthContext,
        _session: &aequora_protocol::SessionMetadata,
    ) -> Result<(), ExecutionError> {
        Ok(())
    }

    async fn authorize<'a>(
        &self,
        _auth: &AuthContext,
        operation: AuthenticatedOperation<'a>,
    ) -> Result<AuthorizedOperation<'a>, ExecutionError> {
        Ok(operation.authorize())
    }

    async fn execute(
        &self,
        _auth: &AuthContext,
        operation: ExecutableOperation<'_>,
        _current: Option<&CurrentEntity>,
    ) -> Result<AuthoritativeMutation, ExecutionError> {
        let operation = operation.envelope();
        Ok(AuthoritativeMutation {
            payload: operation.payload.clone(),
            change_kind: if operation.operation_kind == aequora_protocol::OperationKind(2) {
                aequora_protocol::ChangeKind::Tombstone
            } else {
                aequora_protocol::ChangeKind::Upsert
            },
        })
    }
}

#[derive(Clone, Default)]
struct AuthoritativeState {
    entities: HashMap<EntityKey, EntitySnapshot>,
    ledger: HashMap<OperationKey, OperationAck>,
    journal: Vec<RemoteChange>,
    sequences: HashMap<ScopeKey, Sequence>,
    journal_floors: HashMap<ScopeKey, Sequence>,
    entity_scopes: HashMap<EntityKey, HashSet<SyncScopeId>>,
    snapshots: HashMap<(TenantId, SnapshotId), CapturedSnapshot>,
    audit: Vec<AuditRecord>,
}

#[derive(Clone)]
struct CapturedSnapshot {
    descriptor: SnapshotDescriptor,
    entities: Vec<SnapshotEntity>,
}

/// Atomic authoritative reference store for deterministic tests.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CommitFailPoint {
    /// Fail before constructing an entity write.
    BeforeWrite,
    /// Fail after staging the entity write.
    AfterWrite,
    /// Fail before staging the journal event.
    BeforeJournal,
    /// Fail after staging the journal event.
    AfterJournal,
    /// Fail before staging the idempotency-ledger result.
    BeforeLedger,
    /// Fail after staging the idempotency-ledger result.
    AfterLedger,
    /// Fail before staging the immutable audit record.
    BeforeAudit,
    /// Fail after staging the immutable audit record.
    AfterAudit,
    /// Fail immediately before the atomic state swap.
    BeforeCommit,
    /// Commit successfully but report a transient failure to simulate a lost result.
    AfterCommit,
}

/// Atomic authoritative reference store for deterministic tests.
#[derive(Clone, Default)]
pub struct InMemoryAuthoritativeStore {
    state: Arc<Mutex<AuthoritativeState>>,
    commit_failures: Arc<Mutex<VecDeque<CommitFailPoint>>>,
}

impl InMemoryAuthoritativeStore {
    fn state(&self) -> MutexGuard<'_, AuthoritativeState> {
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// Returns the number of logical operations committed by the server.
    #[must_use]
    pub fn applied_operation_count(&self) -> usize {
        self.state().ledger.len()
    }

    /// Reads an entity synchronously for assertions.
    #[must_use]
    pub fn entity(&self, tenant: TenantId, entity: EntityRef) -> Option<EntitySnapshot> {
        self.state().entities.get(&(tenant, entity)).cloned()
    }

    /// Returns authoritative synchronization-journal length.
    #[must_use]
    pub fn journal_len(&self) -> usize {
        self.state().journal.len()
    }

    /// Returns immutable audit-log length.
    #[must_use]
    pub fn audit_len(&self) -> usize {
        self.state().audit.len()
    }

    /// Injects one transient failure at the selected phase of the next new commit.
    pub fn inject_commit_failure(&self, failpoint: CommitFailPoint) {
        self.commit_failures
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .push_back(failpoint);
    }
}

#[async_trait]
impl EntityReader for InMemoryAuthoritativeStore {
    async fn read_entity(
        &self,
        tenant: TenantId,
        entity: EntityRef,
    ) -> Result<Option<EntitySnapshot>, StoreError> {
        Ok(self.state().entities.get(&(tenant, entity)).cloned())
    }
}

#[async_trait]
impl OperationLedger for InMemoryAuthoritativeStore {
    async fn operation_result(
        &self,
        tenant: TenantId,
        operation_id: OperationId,
    ) -> Result<Option<OperationAck>, StoreError> {
        Ok(self.state().ledger.get(&(tenant, operation_id)).cloned())
    }

    async fn commit_operation(&self, commit: CommitOperation) -> Result<CommitOutcome, StoreError> {
        let mut state = self.state();
        let operation_key = (commit.tenant_id, commit.operation_id);
        if let Some(previous) = state.ledger.get(&operation_key) {
            return Ok(CommitOutcome::Duplicate(previous.clone()));
        }
        let entity_key = (commit.tenant_id, commit.entity);
        let current = state
            .entities
            .get(&entity_key)
            .map(|snapshot| snapshot.current.version);
        if current != commit.expected_version {
            return Ok(CommitOutcome::VersionChanged { current });
        }
        let failpoint = self
            .commit_failures
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .pop_front();
        fail_at(failpoint, CommitFailPoint::BeforeWrite)?;
        let scope_key = (commit.tenant_id, commit.scope_id);
        let next_sequence = state
            .sequences
            .get(&scope_key)
            .map_or(Sequence(1), |sequence| {
                Sequence(sequence.0.saturating_add(1))
            });
        let tombstone = matches!(commit.change_kind, aequora_protocol::ChangeKind::Tombstone);
        let entity_snapshot = EntitySnapshot {
            entity: commit.entity,
            tenant_id: commit.tenant_id,
            current: CurrentEntity {
                version: commit.next_version,
                payload: commit.payload.clone(),
                tombstone,
            },
        };
        fail_at(failpoint, CommitFailPoint::AfterWrite)?;
        fail_at(failpoint, CommitFailPoint::BeforeJournal)?;
        let change = RemoteChange {
            tenant_id: commit.tenant_id,
            scope_id: commit.scope_id,
            sequence: next_sequence,
            operation_id: commit.operation_id,
            entity: commit.entity,
            version: commit.next_version,
            change_kind: commit.change_kind,
            payload: commit.payload,
            timestamp: commit.timestamp,
        };
        fail_at(failpoint, CommitFailPoint::AfterJournal)?;
        fail_at(failpoint, CommitFailPoint::BeforeLedger)?;
        let ack = OperationAck {
            operation_id: commit.operation_id,
            entity_version: commit.next_version,
            sequence: next_sequence,
            duplicate: false,
        };
        fail_at(failpoint, CommitFailPoint::AfterLedger)?;
        fail_at(failpoint, CommitFailPoint::BeforeAudit)?;
        let audit = AuditRecord {
            offset: AuditOffset(
                u64::try_from(state.audit.len())
                    .unwrap_or(u64::MAX)
                    .saturating_add(1),
            ),
            tenant_id: commit.tenant_id,
            operation_id: commit.operation_id,
            actor_id: commit.actor_id,
            device_id: commit.device_id,
            operation_kind: commit.operation_kind,
            entity: commit.entity,
            entity_version: commit.next_version,
            command_digest: commit.command_digest,
            timestamp: commit.timestamp,
        };
        fail_at(failpoint, CommitFailPoint::AfterAudit)?;
        fail_at(failpoint, CommitFailPoint::BeforeCommit)?;

        state.entities.insert(entity_key, entity_snapshot);
        state
            .entity_scopes
            .entry(entity_key)
            .or_default()
            .insert(commit.scope_id);
        state.sequences.insert(scope_key, next_sequence);
        state.journal.push(change);
        state.ledger.insert(operation_key, ack.clone());
        state.audit.push(audit);
        fail_at(failpoint, CommitFailPoint::AfterCommit)?;
        Ok(CommitOutcome::Applied(ack))
    }
}

#[async_trait]
impl AuditLog for InMemoryAuthoritativeStore {
    async fn read_audit_after(
        &self,
        tenant: TenantId,
        offset: AuditOffset,
        limit: usize,
    ) -> Result<AuditPage, StoreError> {
        let state = self.state();
        let mut matching = state
            .audit
            .iter()
            .filter(|record| record.tenant_id == tenant && record.offset > offset);
        let records: Vec<_> = matching.by_ref().take(limit).cloned().collect();
        let has_more = matching.next().is_some();
        let next_offset = records.last().map_or(offset, |record| record.offset);
        Ok(AuditPage {
            records,
            next_offset,
            has_more,
        })
    }
}

fn fail_at(selected: Option<CommitFailPoint>, current: CommitFailPoint) -> Result<(), StoreError> {
    if selected == Some(current) {
        Err(StoreError::transient(format!(
            "simulated authoritative failure at {current:?}"
        )))
    } else {
        Ok(())
    }
}

#[async_trait]
impl SnapshotStore for InMemoryAuthoritativeStore {
    async fn create_snapshot(
        &self,
        tenant: TenantId,
        scope: SyncScopeId,
        _partitions: &[Partition],
    ) -> Result<SnapshotDescriptor, StoreError> {
        let mut state = self.state();
        let descriptor = SnapshotDescriptor {
            snapshot_id: SnapshotId::new(),
            cursor: Cursor {
                scope,
                sequence: state
                    .sequences
                    .get(&(tenant, scope))
                    .copied()
                    .unwrap_or(Sequence(0)),
            },
        };
        let mut entities: Vec<_> = state
            .entities
            .iter()
            .filter(|((entity_tenant, entity), _)| {
                *entity_tenant == tenant
                    && state
                        .entity_scopes
                        .get(&(tenant, *entity))
                        .is_some_and(|scopes| scopes.contains(&scope))
            })
            .map(|(_, snapshot)| SnapshotEntity {
                entity: snapshot.entity,
                version: snapshot.current.version,
                payload: snapshot.current.payload.clone(),
                tombstone: snapshot.current.tombstone,
            })
            .collect();
        entities.sort_by_key(|entity| entity.entity);
        state.snapshots.insert(
            (tenant, descriptor.snapshot_id),
            CapturedSnapshot {
                descriptor,
                entities,
            },
        );
        Ok(descriptor)
    }

    async fn read_snapshot(
        &self,
        tenant: TenantId,
        snapshot_id: SnapshotId,
        offset: u64,
        max_entities: usize,
        max_payload_bytes: usize,
    ) -> Result<SnapshotPage, StoreError> {
        let state = self.state();
        let snapshot = state
            .snapshots
            .get(&(tenant, snapshot_id))
            .ok_or_else(|| StoreError::permanent("snapshot is missing or expired"))?;
        let start = usize::try_from(offset)
            .map_err(|_| StoreError::permanent("snapshot offset is too large"))?;
        if start > snapshot.entities.len() {
            return Err(StoreError::permanent(
                "snapshot offset is beyond the entity count",
            ));
        }
        let mut entities = Vec::new();
        let mut payload_bytes = 0_usize;
        for entity in snapshot.entities.iter().skip(start) {
            if entities.len() == max_entities
                || payload_bytes.saturating_add(entity.payload.len()) > max_payload_bytes
            {
                break;
            }
            payload_bytes = payload_bytes.saturating_add(entity.payload.len());
            entities.push(entity.clone());
        }
        let next_offset = offset.saturating_add(u64::try_from(entities.len()).unwrap_or(u64::MAX));
        let has_more =
            usize::try_from(next_offset).map_or(true, |next| next < snapshot.entities.len());
        Ok(SnapshotPage {
            descriptor: snapshot.descriptor,
            entities,
            next_offset,
            has_more,
        })
    }
}

#[async_trait]
impl ChangeJournal for InMemoryAuthoritativeStore {
    async fn minimum_retained_cursor(
        &self,
        tenant: TenantId,
        scope: SyncScopeId,
    ) -> Result<Sequence, StoreError> {
        Ok(self
            .state()
            .journal_floors
            .get(&(tenant, scope))
            .copied()
            .unwrap_or(Sequence(0)))
    }

    async fn read_changes_after(
        &self,
        tenant: TenantId,
        scope: SyncScopeId,
        sequence: Sequence,
        limit: usize,
        max_payload_bytes: usize,
    ) -> Result<ChangePage, StoreError> {
        let state = self.state();
        let mut matching = state
            .journal
            .iter()
            .filter(|change| {
                change.tenant_id == tenant && change.scope_id == scope && change.sequence > sequence
            })
            .cloned();
        let mut changes = Vec::new();
        let mut payload_bytes = 0_usize;
        let mut has_more = false;
        for change in matching.by_ref() {
            if changes.len() == limit
                || payload_bytes.saturating_add(change.payload.len()) > max_payload_bytes
            {
                has_more = true;
                break;
            }
            payload_bytes = payload_bytes.saturating_add(change.payload.len());
            changes.push(change);
        }
        let next_sequence = changes.last().map_or(sequence, |change| change.sequence);
        Ok(ChangePage {
            changes,
            next_sequence,
            has_more,
        })
    }
}

#[async_trait]
impl JournalCompactor for InMemoryAuthoritativeStore {
    async fn compact_journal(
        &self,
        tenant: TenantId,
        scope: SyncScopeId,
        through: Sequence,
    ) -> Result<u64, StoreError> {
        let mut state = self.state();
        let floor = state
            .journal_floors
            .entry((tenant, scope))
            .or_insert(Sequence(0));
        *floor = Sequence(floor.0.max(through.0));
        let before = state.journal.len();
        state.journal.retain(|change| {
            change.tenant_id != tenant || change.scope_id != scope || change.sequence > through
        });
        Ok(u64::try_from(before.saturating_sub(state.journal.len())).unwrap_or(u64::MAX))
    }
}

#[derive(Default)]
struct LocalState {
    pending: Vec<OperationEnvelope>,
    original: HashMap<OperationId, OperationEnvelope>,
    outbox_states: HashMap<OperationId, OutboxState>,
    cursors: HashMap<SyncScopeId, Cursor>,
    entities: HashMap<(SyncScopeId, EntityRef), SnapshotEntity>,
    processed: HashSet<(SyncScopeId, Sequence)>,
    rejections: Vec<OperationRejection>,
    conflicts: Vec<Conflict>,
    conflict_resolutions: HashMap<OperationId, ConflictResolution>,
    staged_snapshots: HashMap<SyncScopeId, StagedSnapshot>,
}

struct StagedSnapshot {
    progress: SnapshotProgress,
    entities: Vec<SnapshotEntity>,
}

/// Atomic local reference store with durable-state semantics within one process.
#[derive(Clone, Default)]
pub struct InMemoryLocalStore {
    state: Arc<Mutex<LocalState>>,
}

impl InMemoryLocalStore {
    fn state(&self) -> MutexGuard<'_, LocalState> {
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// Returns pending outbox length.
    #[must_use]
    pub fn pending_count(&self) -> usize {
        self.state().pending.len()
    }

    /// Returns a reconciled authoritative entity for assertions.
    #[must_use]
    pub fn entity(&self, entity: EntityRef) -> Option<SnapshotEntity> {
        self.state()
            .entities
            .iter()
            .find_map(|((_, candidate), value)| (*candidate == entity).then(|| value.clone()))
    }

    /// Returns the number of installed entities in one partial synchronization scope.
    #[must_use]
    pub fn entity_count(&self, scope: SyncScopeId) -> usize {
        self.state()
            .entities
            .keys()
            .filter(|(entity_scope, _)| *entity_scope == scope)
            .count()
    }

    /// Returns persisted rejections.
    #[must_use]
    pub fn rejections(&self) -> Vec<OperationRejection> {
        self.state().rejections.clone()
    }

    /// Returns persisted conflicts.
    #[must_use]
    pub fn conflicts(&self) -> Vec<Conflict> {
        self.state().conflicts.clone()
    }
}

#[async_trait]
impl OutboxStore for InMemoryLocalStore {
    async fn pending_operations(&self, limit: usize) -> Result<Vec<OperationEnvelope>, StoreError> {
        let state = self.state();
        Ok(state
            .pending
            .iter()
            .filter(|operation| {
                state
                    .outbox_states
                    .get(&operation.operation_id)
                    .is_some_and(|status| status.is_replayable())
            })
            .take(limit)
            .cloned()
            .collect())
    }

    async fn append_operation(&self, operation: OperationEnvelope) -> Result<(), StoreError> {
        let mut state = self.state();
        if state.original.contains_key(&operation.operation_id) {
            return Err(StoreError::permanent(
                "operation ID already exists in local outbox",
            ));
        }
        state
            .original
            .insert(operation.operation_id, operation.clone());
        state
            .outbox_states
            .insert(operation.operation_id, OutboxState::Pending);
        state.pending.push(operation);
        Ok(())
    }
}

#[async_trait]
impl OutboxStateStore for InMemoryLocalStore {
    async fn mark_sending(&self, operations: &[OperationId]) -> Result<(), StoreError> {
        let mut state = self.state();
        transition_replayable(&mut state, operations, OutboxState::Sending)
    }

    async fn mark_retry(&self, operations: &[OperationId]) -> Result<(), StoreError> {
        let mut state = self.state();
        transition_replayable(&mut state, operations, OutboxState::Retry)
    }

    async fn operation_state(
        &self,
        operation: OperationId,
    ) -> Result<Option<OutboxState>, StoreError> {
        Ok(self.state().outbox_states.get(&operation).copied())
    }

    async fn outbox_stats(&self) -> Result<OutboxStats, StoreError> {
        let stored_state = self.state();
        let mut queue_stats = OutboxStats::default();
        for operation in &stored_state.pending {
            let status = stored_state
                .outbox_states
                .get(&operation.operation_id)
                .copied();
            match status {
                Some(OutboxState::Pending) => {
                    queue_stats.pending = queue_stats.pending.saturating_add(1);
                }
                Some(OutboxState::Sending) => {
                    queue_stats.sending = queue_stats.sending.saturating_add(1);
                }
                Some(OutboxState::Retry) => {
                    queue_stats.retry = queue_stats.retry.saturating_add(1);
                }
                _ => continue,
            }
            queue_stats.oldest_pending_at = Some(
                queue_stats
                    .oldest_pending_at
                    .map_or(operation.created_at, |current| {
                        current.min(operation.created_at)
                    }),
            );
        }
        Ok(queue_stats)
    }
}

#[async_trait]
impl ConflictInbox for InMemoryLocalStore {
    async fn unresolved_conflicts(&self, limit: usize) -> Result<Vec<ConflictRecord>, StoreError> {
        let state = self.state();
        Ok(state
            .conflicts
            .iter()
            .filter(|conflict| {
                !state
                    .conflict_resolutions
                    .contains_key(&conflict.operation_id)
            })
            .take(limit)
            .cloned()
            .map(|conflict| ConflictRecord {
                conflict,
                resolution: None,
            })
            .collect())
    }

    async fn unresolved_conflict_count(&self) -> Result<usize, StoreError> {
        let state = self.state();
        Ok(state
            .conflicts
            .iter()
            .filter(|conflict| {
                !state
                    .conflict_resolutions
                    .contains_key(&conflict.operation_id)
            })
            .count())
    }

    async fn resolve_conflict(
        &self,
        operation: OperationId,
        resolution: ConflictResolution,
    ) -> Result<(), StoreError> {
        let mut state = self.state();
        if !state
            .conflicts
            .iter()
            .any(|conflict| conflict.operation_id == operation)
        {
            return Err(StoreError::permanent("conflict does not exist"));
        }
        state.conflict_resolutions.insert(operation, resolution);
        Ok(())
    }
}

fn transition_replayable(
    state: &mut LocalState,
    operations: &[OperationId],
    next: OutboxState,
) -> Result<(), StoreError> {
    for operation in operations {
        let current = state
            .outbox_states
            .get(operation)
            .copied()
            .ok_or_else(|| StoreError::permanent("operation is missing from the outbox"))?;
        if !current.is_replayable() {
            return Err(StoreError::permanent(
                "terminal outbox operation cannot transition back to replayable",
            ));
        }
    }
    for operation in operations {
        state.outbox_states.insert(*operation, next);
    }
    Ok(())
}

#[async_trait]
impl CursorStore for InMemoryLocalStore {
    async fn load_cursor(&self, scope: SyncScopeId) -> Result<Option<Cursor>, StoreError> {
        Ok(self.state().cursors.get(&scope).copied())
    }
}

#[async_trait]
impl ReconciliationStore for InMemoryLocalStore {
    async fn reconcile(&self, response: &SyncResponse) -> Result<(), StoreError> {
        let mut state = self.state();
        let current = state.cursors.get(&response.next_cursor.scope).copied();
        if current.is_some_and(|cursor| cursor.sequence > response.next_cursor.sequence) {
            return Err(StoreError::permanent(
                "cursor regression during reconciliation",
            ));
        }
        let terminal: HashSet<_> = response
            .acknowledged
            .iter()
            .map(|ack| ack.operation_id)
            .chain(response.rejected.iter().map(|item| item.operation_id))
            .chain(response.conflicts.iter().map(|item| item.operation_id))
            .collect();
        for acknowledgement in &response.acknowledged {
            state
                .outbox_states
                .insert(acknowledgement.operation_id, OutboxState::Acknowledged);
        }
        for rejection in &response.rejected {
            state
                .outbox_states
                .insert(rejection.operation_id, OutboxState::Rejected);
        }
        for conflict in &response.conflicts {
            state
                .outbox_states
                .insert(conflict.operation_id, OutboxState::Conflict);
        }
        state
            .pending
            .retain(|operation| !terminal.contains(&operation.operation_id));
        for change in &response.changes {
            let marker = (change.scope_id, change.sequence);
            if state.processed.insert(marker) {
                state.entities.insert(
                    (change.scope_id, change.entity),
                    SnapshotEntity {
                        entity: change.entity,
                        version: change.version,
                        payload: change.payload.clone(),
                        tombstone: matches!(
                            change.change_kind,
                            aequora_protocol::ChangeKind::Tombstone
                        ),
                    },
                );
            }
        }
        state.rejections.extend(response.rejected.iter().cloned());
        state.conflicts.extend(response.conflicts.iter().cloned());
        state
            .cursors
            .insert(response.next_cursor.scope, response.next_cursor);
        Ok(())
    }

    async fn stage_snapshot(&self, response: &BootstrapResponse) -> Result<(), StoreError> {
        let mut state = self.state();
        let scope = response.cursor.scope;
        let staged = state
            .staged_snapshots
            .entry(scope)
            .or_insert_with(|| StagedSnapshot {
                progress: SnapshotProgress {
                    snapshot_id: response.snapshot_id,
                    cursor: response.cursor,
                    next_offset: 0,
                },
                entities: Vec::new(),
            });
        if staged.progress.snapshot_id != response.snapshot_id
            || staged.progress.cursor != response.cursor
            || staged.progress.next_offset != response.offset
        {
            return Err(StoreError::permanent(
                "snapshot page does not match staged progress",
            ));
        }
        staged.entities.extend(response.entities.iter().cloned());
        staged.progress.next_offset = response.next_offset;
        if !response.has_more {
            let staged = state
                .staged_snapshots
                .remove(&scope)
                .ok_or_else(|| StoreError::permanent("snapshot staging disappeared"))?;
            state
                .entities
                .retain(|(entity_scope, _), _| *entity_scope != scope);
            for entity in staged.entities {
                state.entities.insert((scope, entity.entity), entity);
            }
            state.cursors.insert(scope, response.cursor);
        }
        Ok(())
    }

    async fn snapshot_progress(
        &self,
        scope: SyncScopeId,
    ) -> Result<Option<SnapshotProgress>, StoreError> {
        Ok(self
            .state()
            .staged_snapshots
            .get(&scope)
            .map(|snapshot| snapshot.progress))
    }
}

/// Direct transport that invokes a server without network nondeterminism.
#[derive(Clone)]
pub struct InProcessTransport {
    service: Arc<dyn ExchangeService>,
    auth: AuthContext,
}

impl InProcessTransport {
    /// Creates an in-process authenticated transport.
    #[must_use]
    pub fn new(service: Arc<dyn ExchangeService>, auth: AuthContext) -> Self {
        Self { service, auth }
    }
}

#[async_trait]
impl SyncTransport for InProcessTransport {
    async fn exchange(&self, request: SyncRequest) -> Result<SyncResponse, TransportError> {
        self.service
            .exchange(self.auth, request)
            .await
            .map_err(map_server_error)
    }

    async fn bootstrap(
        &self,
        request: BootstrapRequest,
    ) -> Result<BootstrapResponse, TransportError> {
        self.service
            .bootstrap(self.auth, request)
            .await
            .map_err(map_server_error)
    }
}

struct InProcessSnapshotStream {
    service: Arc<dyn ExchangeService>,
    auth: AuthContext,
    request: BootstrapRequest,
    finished: bool,
}

#[async_trait]
impl SnapshotPageStream for InProcessSnapshotStream {
    async fn next_page(&mut self) -> Result<Option<BootstrapResponse>, TransportError> {
        if self.finished {
            return Ok(None);
        }
        let response = self
            .service
            .bootstrap(self.auth, self.request.clone())
            .await
            .map_err(map_server_error)?;
        self.finished = !response.has_more;
        self.request.snapshot_id = Some(response.snapshot_id);
        self.request.offset = response.next_offset;
        Ok(Some(response))
    }
}

#[async_trait]
impl StreamingSyncTransport for InProcessTransport {
    async fn bootstrap_stream(
        &self,
        request: BootstrapRequest,
    ) -> Result<Box<dyn SnapshotPageStream>, TransportError> {
        Ok(Box::new(InProcessSnapshotStream {
            service: self.service.clone(),
            auth: self.auth,
            request,
            finished: false,
        }))
    }
}

/// Transport wrapper that emits a deterministic sequence of failures before delegating.
pub struct FaultInjectingTransport<T> {
    inner: T,
    failures: Mutex<VecDeque<TransportError>>,
    attempts: AtomicUsize,
}

/// Transport wrapper that lets the server commit, then drops successful responses.
/// This deterministically exercises exactly-once logical effects over retry delivery.
pub struct ResponseDroppingTransport<T> {
    inner: T,
    drops_remaining: AtomicUsize,
}

impl<T> ResponseDroppingTransport<T> {
    /// Creates a wrapper that drops the next `drops` successful responses.
    #[must_use]
    pub const fn new(inner: T, drops: usize) -> Self {
        Self {
            inner,
            drops_remaining: AtomicUsize::new(drops),
        }
    }

    fn should_drop(&self) -> bool {
        self.drops_remaining
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |remaining| {
                remaining.checked_sub(1)
            })
            .is_ok()
    }
}

#[async_trait]
impl<T: SyncTransport> SyncTransport for ResponseDroppingTransport<T> {
    async fn exchange(&self, request: SyncRequest) -> Result<SyncResponse, TransportError> {
        let response = self.inner.exchange(request).await?;
        if self.should_drop() {
            Err(TransportError::transient(
                "simulated response loss after server commit",
            ))
        } else {
            Ok(response)
        }
    }

    async fn bootstrap(
        &self,
        request: BootstrapRequest,
    ) -> Result<BootstrapResponse, TransportError> {
        let response = self.inner.bootstrap(request).await?;
        if self.should_drop() {
            Err(TransportError::transient(
                "simulated bootstrap response loss",
            ))
        } else {
            Ok(response)
        }
    }
}

impl<T> FaultInjectingTransport<T> {
    /// Creates a wrapper whose failures are returned in insertion order.
    #[must_use]
    pub fn new(inner: T, failures: impl IntoIterator<Item = TransportError>) -> Self {
        Self {
            inner,
            failures: Mutex::new(failures.into_iter().collect()),
            attempts: AtomicUsize::new(0),
        }
    }

    /// Number of attempted exchanges, including injected failures.
    #[must_use]
    pub fn attempts(&self) -> usize {
        self.attempts.load(Ordering::Relaxed)
    }
}

#[async_trait]
impl<T: SyncTransport> SyncTransport for FaultInjectingTransport<T> {
    async fn exchange(&self, request: SyncRequest) -> Result<SyncResponse, TransportError> {
        self.attempts.fetch_add(1, Ordering::Relaxed);
        let failure = self
            .failures
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .pop_front();
        match failure {
            Some(error) => Err(error),
            None => self.inner.exchange(request).await,
        }
    }

    async fn bootstrap(
        &self,
        request: BootstrapRequest,
    ) -> Result<BootstrapResponse, TransportError> {
        self.attempts.fetch_add(1, Ordering::Relaxed);
        let failure = self
            .failures
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .pop_front();
        match failure {
            Some(error) => Err(error),
            None => self.inner.bootstrap(request).await,
        }
    }
}

fn map_server_error(error: ServerError) -> TransportError {
    match error {
        ServerError::Store(store) if store.kind == StoreErrorKind::Transient => {
            TransportError::transient(store.message)
        }
        other => TransportError::permanent(other.to_string()),
    }
}
