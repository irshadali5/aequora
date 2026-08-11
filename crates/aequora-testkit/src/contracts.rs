//! Reusable behavioral contracts for third-party database adapters.
//!
//! These checks mutate their input stores. Callers must supply isolated stores and fixture IDs
//! that have never previously been used in those stores.

use aequora_protocol::{
    ChangeKind, OperationAck, OperationEnvelope, RemoteChange, SyncDirective, SyncResponse,
};
use aequora_store::{
    AuditLog, AuditOffset, AuthoritativeStore, ChangeJournal, CommitOperation, CommitOutcome,
    EntityReader, LocalStore, OutboxState, OutboxStateStore, SnapshotStore, StoreError,
    TransactionCapabilities, TransactionCapabilityProvider, TransactionGuarantees,
};
use aequora_types::{
    Cursor, EntityId, EntityRef, EntityVersion, HybridTimestamp, OperationId, Sequence, SnapshotId,
    SyncScopeId,
};
use futures_util::future::join;
use std::time::{SystemTime, UNIX_EPOCH};
use thiserror::Error;

/// A database operation failed or violated a required Aequora storage invariant.
#[derive(Clone, Debug, Error, Eq, PartialEq)]
pub enum AdapterContractError {
    /// The adapter returned its normal storage failure.
    #[error(transparent)]
    Store(#[from] StoreError),
    /// The adapter returned success but did not preserve a required semantic guarantee.
    #[error("adapter contract violation: {0}")]
    Violation(&'static str),
}

/// Evidence returned after a local adapter passes its core behavioral contract.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LocalAdapterContractReport {
    /// Operation driven through pending, sending, retry, and acknowledged states.
    pub operation_id: aequora_types::OperationId,
    /// Cursor durably installed by reconciliation.
    pub cursor: Cursor,
}

/// Evidence returned after an authority adapter passes its core behavioral contract.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AuthoritativeAdapterContractReport {
    /// Stable result returned for both the initial and duplicate commit.
    pub acknowledgement: OperationAck,
    /// Consistent snapshot captured after the commit.
    pub snapshot_id: SnapshotId,
    /// Immutable audit offset containing the committed operation.
    pub audit_offset: AuditOffset,
}

/// Exercises the durable outbox state machine and idempotent reconciliation contract.
///
/// `operation`, its ID, and `scope` must be unique within an isolated adapter instance. The helper
/// appends the operation, drives `Pending -> Sending -> Retry`, applies one acknowledgement and
/// authoritative change twice, then verifies terminal cleanup and cursor durability.
///
/// Application-specific optimistic entity mutation atomicity cannot be expressed through
/// [`LocalStore`]; adapter authors must separately test that mutation and outbox insertion use one
/// native database transaction.
///
/// # Errors
///
/// Returns [`AdapterContractError::Store`] for adapter failures and
/// [`AdapterContractError::Violation`] when a required invariant is not preserved.
pub async fn verify_local_store<S>(
    store: &S,
    operation: OperationEnvelope,
    scope: SyncScopeId,
    server_time: HybridTimestamp,
) -> Result<LocalAdapterContractReport, AdapterContractError>
where
    S: LocalStore + TransactionCapabilityProvider,
{
    verify_local_capabilities(store.transaction_capabilities())?;
    let operation_id = operation.operation_id;
    store.append_operation(operation.clone()).await?;
    verify_state(store, operation_id, OutboxState::Pending).await?;
    let pending = store.pending_operations(1_024).await?;
    if pending
        .iter()
        .filter(|candidate| candidate.operation_id == operation_id)
        .count()
        != 1
    {
        return Err(AdapterContractError::Violation(
            "a newly appended operation must appear exactly once in the replayable outbox",
        ));
    }

    store.mark_sending(&[operation_id]).await?;
    verify_state(store, operation_id, OutboxState::Sending).await?;
    verify_retry_schedule(store, operation_id).await?;

    let cursor = Cursor {
        scope,
        sequence: Sequence(1),
    };
    let acknowledgement = OperationAck {
        operation_id,
        entity_version: aequora_types::EntityVersion::INITIAL,
        sequence: cursor.sequence,
        duplicate: false,
    };
    let response = SyncResponse {
        protocol: operation.protocol_version,
        directive: SyncDirective::Continue,
        acknowledged: vec![acknowledgement],
        rejected: Vec::new(),
        conflicts: Vec::new(),
        changes: vec![RemoteChange {
            tenant_id: operation.tenant_id,
            scope_id: scope,
            sequence: cursor.sequence,
            operation_id,
            entity: operation.entity,
            version: aequora_types::EntityVersion::INITIAL,
            change_kind: ChangeKind::Upsert,
            payload: operation.payload,
            timestamp: server_time,
        }],
        next_cursor: cursor,
        has_more: false,
        server_time,
    };
    store.reconcile(&response).await?;
    store.reconcile(&response).await?;

    verify_state(store, operation_id, OutboxState::Acknowledged).await?;
    if store
        .pending_operations(1_024)
        .await?
        .iter()
        .any(|candidate| candidate.operation_id == operation_id)
    {
        return Err(AdapterContractError::Violation(
            "an acknowledged operation must leave the replayable outbox",
        ));
    }
    if store.load_cursor(scope).await? != Some(cursor) {
        return Err(AdapterContractError::Violation(
            "reconciliation must durably advance the scope cursor",
        ));
    }

    Ok(LocalAdapterContractReport {
        operation_id,
        cursor,
    })
}

async fn verify_retry_schedule<S>(
    store: &S,
    operation_id: OperationId,
) -> Result<(), AdapterContractError>
where
    S: LocalStore,
{
    let retry_deadline = unix_time_millis().saturating_add(60_000);
    store.mark_retry(&[operation_id], retry_deadline).await?;
    verify_state(store, operation_id, OutboxState::Retry).await?;
    let retry =
        store
            .retry_metadata(operation_id)
            .await?
            .ok_or(AdapterContractError::Violation(
                "a retry transition must persist scheduling metadata",
            ))?;
    if retry.attempt_count != 1 || retry.next_attempt_unix_ms != retry_deadline {
        return Err(AdapterContractError::Violation(
            "the first retry transition must persist its attempt count and deadline",
        ));
    }
    if store
        .pending_operations(1_024)
        .await?
        .iter()
        .any(|operation| operation.operation_id == operation_id)
    {
        return Err(AdapterContractError::Violation(
            "an operation must not be selected before its durable retry deadline",
        ));
    }
    store.mark_retry(&[operation_id], 0).await?;
    let retry =
        store
            .retry_metadata(operation_id)
            .await?
            .ok_or(AdapterContractError::Violation(
                "a repeated retry must retain scheduling metadata",
            ))?;
    if retry.attempt_count != 2 || retry.next_attempt_unix_ms != 0 {
        return Err(AdapterContractError::Violation(
            "a repeated retry must increment its durable attempt count and replace its deadline",
        ));
    }
    Ok(())
}

fn unix_time_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| {
            u64::try_from(duration.as_millis()).unwrap_or(u64::MAX)
        })
}

/// Exercises authoritative atomic commit, idempotency, journal, snapshot, and audit semantics.
///
/// The commit must use a fresh tenant, scope, operation ID, and entity with `expected_version` set
/// to `None`. The helper commits it twice and verifies that exactly one logical state transition is
/// visible through every authoritative capability.
///
/// # Errors
///
/// Returns [`AdapterContractError::Store`] for adapter failures and
/// [`AdapterContractError::Violation`] when a required invariant is not preserved.
pub async fn verify_authoritative_store<S>(
    store: &S,
    commit: CommitOperation,
) -> Result<AuthoritativeAdapterContractReport, AdapterContractError>
where
    S: AuthoritativeStore + TransactionCapabilityProvider,
{
    verify_authoritative_capabilities(store.transaction_capabilities())?;
    if store
        .operation_result(commit.tenant_id, commit.operation_id)
        .await?
        .is_some()
    {
        return Err(AdapterContractError::Violation(
            "the conformance commit operation ID must be unused",
        ));
    }

    let CommitOutcome::Applied(acknowledgement) = store.commit_operation(commit.clone()).await?
    else {
        return Err(AdapterContractError::Violation(
            "a fresh authoritative commit must be applied",
        ));
    };
    let CommitOutcome::Duplicate(duplicate) = store.commit_operation(commit.clone()).await? else {
        return Err(AdapterContractError::Violation(
            "repeating an operation ID must return its duplicate result",
        ));
    };
    if !same_acknowledgement(&acknowledgement, &duplicate) {
        return Err(AdapterContractError::Violation(
            "a duplicate commit must return the original logical result",
        ));
    }
    let stored_acknowledgement = store
        .operation_result(commit.tenant_id, commit.operation_id)
        .await?
        .ok_or(AdapterContractError::Violation(
            "the idempotency ledger must retain the commit result",
        ))?;
    if !same_acknowledgement(&acknowledgement, &stored_acknowledgement) {
        return Err(AdapterContractError::Violation(
            "the durable idempotency result differs from the commit result",
        ));
    }

    verify_authoritative_entity(store, &commit).await?;
    verify_authoritative_journal(store, &commit, &acknowledgement).await?;
    let audit_offset = verify_authoritative_audit(store, &commit).await?;
    let snapshot_id = verify_authoritative_snapshot(store, &commit).await?;
    verify_invalid_version_transition(store, &commit).await?;
    verify_concurrent_duplicate(store, &commit).await?;
    verify_concurrent_version_race(store, &commit).await?;

    Ok(AuthoritativeAdapterContractReport {
        acknowledgement,
        snapshot_id,
        audit_offset,
    })
}

async fn verify_invalid_version_transition<S>(
    store: &S,
    baseline: &CommitOperation,
) -> Result<(), AdapterContractError>
where
    S: AuthoritativeStore,
{
    let invalid_next =
        EntityVersion::INITIAL
            .checked_next()
            .ok_or(AdapterContractError::Violation(
                "the conformance fixture cannot create an invalid version transition",
            ))?;
    let invalid = CommitOperation {
        operation_id: OperationId::new(),
        entity: EntityRef {
            entity_type: baseline.entity.entity_type,
            entity_id: EntityId::new(),
        },
        expected_version: None,
        next_version: invalid_next,
        ..baseline.clone()
    };
    if store.commit_operation(invalid.clone()).await.is_ok() {
        return Err(AdapterContractError::Violation(
            "an authoritative adapter must reject a version transition that skips the initial version",
        ));
    }
    if store
        .read_entity(invalid.tenant_id, invalid.entity)
        .await?
        .is_some()
        || store
            .operation_result(invalid.tenant_id, invalid.operation_id)
            .await?
            .is_some()
    {
        return Err(AdapterContractError::Violation(
            "an invalid version transition must not leave entity or ledger state",
        ));
    }
    verify_no_journal_event(store, &invalid).await?;
    verify_no_audit_record(store, &invalid).await
}

fn verify_local_capabilities(
    capabilities: TransactionCapabilities,
) -> Result<(), AdapterContractError> {
    if !capabilities.is_consistent() {
        return Err(AdapterContractError::Violation(
            "the adapter transaction capability declaration is internally inconsistent",
        ));
    }
    if !capabilities.guarantees.contains(
        TransactionGuarantees::LOCAL_MUTATION_OUTBOX
            .union(TransactionGuarantees::RECONCILIATION_CURSOR),
    ) {
        return Err(AdapterContractError::Violation(
            "a writable local adapter must declare both local/outbox and reconciliation/cursor atomicity",
        ));
    }
    Ok(())
}

fn verify_authoritative_capabilities(
    capabilities: TransactionCapabilities,
) -> Result<(), AdapterContractError> {
    if !capabilities.is_consistent() {
        return Err(AdapterContractError::Violation(
            "the adapter transaction capability declaration is internally inconsistent",
        ));
    }
    if !capabilities.guarantees.contains(
        TransactionGuarantees::AUTHORITATIVE_COMMIT
            .union(TransactionGuarantees::CONCURRENT_IDEMPOTENCY)
            .union(TransactionGuarantees::CONSISTENT_SNAPSHOT),
    ) {
        return Err(AdapterContractError::Violation(
            "an authoritative adapter must declare atomic commit, concurrent idempotency, and consistent snapshots",
        ));
    }
    Ok(())
}

async fn verify_concurrent_duplicate<S>(
    store: &S,
    baseline: &CommitOperation,
) -> Result<(), AdapterContractError>
where
    S: AuthoritativeStore,
{
    let expected_version = baseline.next_version;
    let next_version = expected_version
        .checked_next()
        .ok_or(AdapterContractError::Violation(
            "the conformance fixture cannot advance its entity version",
        ))?;
    let operation_id = OperationId::new();
    let commit = CommitOperation {
        operation_id,
        expected_version: Some(expected_version),
        next_version,
        ..baseline.clone()
    };
    let (left, right) = join(
        store.commit_operation(commit.clone()),
        store.commit_operation(commit.clone()),
    )
    .await;
    let (left, right) = (left?, right?);
    let acknowledgement = match (&left, &right) {
        (CommitOutcome::Applied(applied), CommitOutcome::Duplicate(duplicate))
        | (CommitOutcome::Duplicate(duplicate), CommitOutcome::Applied(applied))
            if same_acknowledgement(applied, duplicate) =>
        {
            applied
        }
        _ => {
            return Err(AdapterContractError::Violation(
                "concurrent duplicate commits must yield one applied and one identical duplicate result",
            ));
        }
    };
    let stored = store
        .operation_result(baseline.tenant_id, operation_id)
        .await?
        .ok_or(AdapterContractError::Violation(
            "a concurrent duplicate race did not retain its operation result",
        ))?;
    if !same_acknowledgement(acknowledgement, &stored) {
        return Err(AdapterContractError::Violation(
            "the concurrent duplicate ledger result differs from the winning commit",
        ));
    }
    verify_exactly_one_journal_event(store, &commit).await?;
    verify_exactly_one_audit_record(store, &commit).await
}

async fn verify_concurrent_version_race<S>(
    store: &S,
    baseline: &CommitOperation,
) -> Result<(), AdapterContractError>
where
    S: AuthoritativeStore,
{
    let entity = EntityRef {
        entity_type: baseline.entity.entity_type,
        entity_id: EntityId::new(),
    };
    let mut left_commit = CommitOperation {
        operation_id: OperationId::new(),
        entity,
        expected_version: None,
        next_version: EntityVersion::INITIAL,
        payload: [baseline.payload.as_slice(), b"-race-left"].concat(),
        ..baseline.clone()
    };
    left_commit.command_digest[0] ^= 1;
    let mut right_commit = CommitOperation {
        operation_id: OperationId::new(),
        payload: [baseline.payload.as_slice(), b"-race-right"].concat(),
        ..left_commit.clone()
    };
    right_commit.command_digest[0] ^= 2;

    let (left, right) = join(
        store.commit_operation(left_commit.clone()),
        store.commit_operation(right_commit.clone()),
    )
    .await;
    let (left, right) = (left?, right?);
    let (winner, loser) = match (&left, &right) {
        (
            CommitOutcome::Applied(_),
            CommitOutcome::VersionChanged {
                current: Some(EntityVersion::INITIAL),
            },
        ) => (&left_commit, &right_commit),
        (
            CommitOutcome::VersionChanged {
                current: Some(EntityVersion::INITIAL),
            },
            CommitOutcome::Applied(_),
        ) => (&right_commit, &left_commit),
        _ => {
            return Err(AdapterContractError::Violation(
                "concurrent creation of one entity must apply once and reject the losing version race",
            ));
        }
    };
    if store
        .operation_result(loser.tenant_id, loser.operation_id)
        .await?
        .is_some()
    {
        return Err(AdapterContractError::Violation(
            "the losing version race must not record an operation-ledger result",
        ));
    }
    verify_authoritative_entity(store, winner).await?;
    verify_exactly_one_journal_event(store, winner).await?;
    verify_no_journal_event(store, loser).await?;
    verify_exactly_one_audit_record(store, winner).await?;
    verify_no_audit_record(store, loser).await
}

async fn verify_exactly_one_journal_event<S>(
    store: &S,
    commit: &CommitOperation,
) -> Result<(), AdapterContractError>
where
    S: ChangeJournal,
{
    let page = store
        .read_changes_after(
            commit.tenant_id,
            commit.scope_id,
            Sequence(0),
            1_024,
            64 * 1_024 * 1_024,
        )
        .await?;
    if page
        .changes
        .iter()
        .filter(|change| change.operation_id == commit.operation_id)
        .count()
        != 1
    {
        return Err(AdapterContractError::Violation(
            "a successful raced commit must append exactly one journal event",
        ));
    }
    Ok(())
}

async fn verify_no_journal_event<S>(
    store: &S,
    commit: &CommitOperation,
) -> Result<(), AdapterContractError>
where
    S: ChangeJournal,
{
    let page = store
        .read_changes_after(
            commit.tenant_id,
            commit.scope_id,
            Sequence(0),
            1_024,
            64 * 1_024 * 1_024,
        )
        .await?;
    if page
        .changes
        .iter()
        .any(|change| change.operation_id == commit.operation_id)
    {
        return Err(AdapterContractError::Violation(
            "the losing version race must not append a journal event",
        ));
    }
    Ok(())
}

async fn verify_exactly_one_audit_record<S>(
    store: &S,
    commit: &CommitOperation,
) -> Result<(), AdapterContractError>
where
    S: AuditLog,
{
    let page = store
        .read_audit_after(commit.tenant_id, AuditOffset(0), 1_024)
        .await?;
    if page
        .records
        .iter()
        .filter(|record| record.operation_id == commit.operation_id)
        .count()
        != 1
    {
        return Err(AdapterContractError::Violation(
            "a successful raced commit must append exactly one audit record",
        ));
    }
    Ok(())
}

async fn verify_no_audit_record<S>(
    store: &S,
    commit: &CommitOperation,
) -> Result<(), AdapterContractError>
where
    S: AuditLog,
{
    let page = store
        .read_audit_after(commit.tenant_id, AuditOffset(0), 1_024)
        .await?;
    if page
        .records
        .iter()
        .any(|record| record.operation_id == commit.operation_id)
    {
        return Err(AdapterContractError::Violation(
            "the losing version race must not append an audit record",
        ));
    }
    Ok(())
}

async fn verify_state<S>(
    store: &S,
    operation: aequora_types::OperationId,
    expected: OutboxState,
) -> Result<(), AdapterContractError>
where
    S: OutboxStateStore,
{
    if store.operation_state(operation).await? != Some(expected) {
        return Err(AdapterContractError::Violation(
            "the outbox did not persist its required state transition",
        ));
    }
    Ok(())
}

fn same_acknowledgement(left: &OperationAck, right: &OperationAck) -> bool {
    left.operation_id == right.operation_id
        && left.entity_version == right.entity_version
        && left.sequence == right.sequence
}

async fn verify_authoritative_entity<S>(
    store: &S,
    commit: &CommitOperation,
) -> Result<(), AdapterContractError>
where
    S: EntityReader,
{
    let entity = store
        .read_entity(commit.tenant_id, commit.entity)
        .await?
        .ok_or(AdapterContractError::Violation(
            "the committed authoritative entity is missing",
        ))?;
    let expected_tombstone = matches!(commit.change_kind, ChangeKind::Tombstone);
    if entity.current.version != commit.next_version
        || entity.current.payload != commit.payload
        || entity.current.tombstone != expected_tombstone
    {
        return Err(AdapterContractError::Violation(
            "authoritative entity state differs from the committed mutation",
        ));
    }
    Ok(())
}

async fn verify_authoritative_journal<S>(
    store: &S,
    commit: &CommitOperation,
    acknowledgement: &OperationAck,
) -> Result<(), AdapterContractError>
where
    S: ChangeJournal,
{
    let page = store
        .read_changes_after(
            commit.tenant_id,
            commit.scope_id,
            Sequence(0),
            1_024,
            commit.payload.len().saturating_add(1_024),
        )
        .await?;
    let matching: Vec<_> = page
        .changes
        .iter()
        .filter(|change| change.operation_id == commit.operation_id)
        .collect();
    if matching.len() != 1
        || matching[0].sequence != acknowledgement.sequence
        || matching[0].entity != commit.entity
        || matching[0].payload != commit.payload
    {
        return Err(AdapterContractError::Violation(
            "one authoritative commit must produce exactly one matching journal event",
        ));
    }
    Ok(())
}

async fn verify_authoritative_audit<S>(
    store: &S,
    commit: &CommitOperation,
) -> Result<AuditOffset, AdapterContractError>
where
    S: AuditLog,
{
    let page = store
        .read_audit_after(commit.tenant_id, AuditOffset(0), 1_024)
        .await?;
    let matching: Vec<_> = page
        .records
        .iter()
        .filter(|record| record.operation_id == commit.operation_id)
        .collect();
    if matching.len() != 1 || matching[0].command_digest != commit.command_digest {
        return Err(AdapterContractError::Violation(
            "one authoritative commit must produce exactly one matching audit record",
        ));
    }
    Ok(matching[0].offset)
}

async fn verify_authoritative_snapshot<S>(
    store: &S,
    commit: &CommitOperation,
) -> Result<SnapshotId, AdapterContractError>
where
    S: SnapshotStore,
{
    let descriptor = store
        .create_snapshot(commit.tenant_id, commit.scope_id, &[])
        .await?;
    let page = store
        .read_snapshot(
            commit.tenant_id,
            descriptor.snapshot_id,
            0,
            1_024,
            commit.payload.len().saturating_add(1_024),
        )
        .await?;
    let matching: Vec<_> = page
        .entities
        .iter()
        .filter(|entity| entity.entity == commit.entity)
        .collect();
    if matching.len() != 1
        || matching[0].version != commit.next_version
        || matching[0].payload != commit.payload
    {
        return Err(AdapterContractError::Violation(
            "a consistent snapshot must contain the committed authoritative entity exactly once",
        ));
    }
    Ok(descriptor.snapshot_id)
}
