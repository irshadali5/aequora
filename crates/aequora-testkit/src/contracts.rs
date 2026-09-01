//! Reusable behavioral contracts for third-party database adapters.
//!
//! These checks mutate their input stores. Callers must supply isolated stores and fixture IDs
//! that have never previously been used in those stores.

use aequora_coordination::{
    FencingToken, LeaseKind, LeaseRequest, LocalCoordinationSupport, LocalStoreGeneration,
    LocalStoreId, ProcessInstanceId,
};
use aequora_integrity::{
    CanonicalEntity, IntegrityComparison, IntegrityGeneration, IntegritySnapshot, IntegritySupport,
    PartitionScheme, RepairPlan,
};
use aequora_protocol::{
    ChangeKind, OperationAck, OperationEnvelope, OperationKind, OperationMetadata, RemoteChange,
    SyncDirective, SyncResponse,
};
use aequora_scope::{
    LocalScopeState, MembershipRecord, ProjectionVersion, ResolvedScope, ScopeCursor,
    ScopeDefinitionId, ScopeDescriptor, ScopeGeneration, ScopeTransition, ScopeTransitionId,
    ScopeTransitionKind, ScopeTransitionOutcome, ScopeVersion, Subscription, SubscriptionId,
    SubscriptionState,
};
use aequora_store::{
    AuditLog, AuditOffset, AuthoritativeIntegritySource, AuthoritativeStore, ChangeJournal,
    CommitOperation, CommitOutcome, CorrelationLog, CursorStore, EntityReader,
    IntegrityCapabilityProvider, LocalCoordinationStore, LocalIntegrityStore, LocalStore,
    OutboxState, OutboxStateStore, OutboxStore, ScopeStateStore, SnapshotStore, StoreError,
    TransactionCapabilities, TransactionCapabilityProvider, TransactionGuarantees,
};
use aequora_types::{
    ActorId, CorrelationId, Cursor, DeviceId, EntityId, EntityRef, EntityType, EntityVersion,
    EventId, HybridTimestamp, LineageContext, LineageRef, NodeId, OperationId, ProtocolVersion,
    SchemaVersion, Sequence, SnapshotId, SyncScopeId, TenantId,
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

/// Evidence returned after a local adapter passes durable coordination compliance.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LocalCoordinationContractReport {
    /// Persistent identity retained across election and generation changes.
    pub store_id: LocalStoreId,
    /// Highest fencing token allocated during the scenario.
    pub fencing_token: FencingToken,
    /// Generation allocated by an exclusive maintenance owner.
    pub store_generation: LocalStoreGeneration,
}

/// Evidence returned after a local adapter passes dynamic-scope compliance.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ScopeStateContractReport {
    pub subscription: aequora_scope::SubscriptionId,
    pub transition: aequora_scope::ScopeTransitionId,
    pub quarantined_operation: OperationId,
}

/// Produces isolated inputs for [`verify_scope_state_store`].
///
/// # Errors
///
/// Returns a typed scope error only if a built-in non-zero fixture identifier is invalid.
pub fn scope_state_contract_fixture()
-> Result<(Subscription, ScopeTransition, OperationEnvelope), aequora_scope::ScopeError> {
    let tenant = TenantId::new();
    let scope_id = SyncScopeId::new();
    let entity = EntityRef {
        entity_type: EntityType::new(81).map_err(|_| aequora_scope::ScopeError::ZeroIdentity)?,
        entity_id: EntityId::new(),
    };
    let definition = ScopeDefinitionId::new(1)?;
    let initial = ResolvedScope {
        scope_id,
        version: ScopeVersion::INITIAL,
        generation: ScopeGeneration::INITIAL,
        descriptor: ScopeDescriptor {
            tenant_id: tenant,
            definition,
            partitions: std::collections::BTreeSet::new(),
            policy_version: 1,
            projection: ProjectionVersion(1),
        },
    };
    let subscription = Subscription {
        subscription_id: SubscriptionId::new(),
        scope: initial.clone(),
        state: SubscriptionState::Resolved,
        cursor: None,
        pending_transition: None,
    };
    let next_version = initial.version.next()?;
    let mut target = initial.clone();
    target.version = next_version;
    let transition = ScopeTransition {
        transition_id: ScopeTransitionId::new(),
        subscription_id: subscription.subscription_id,
        from_version: initial.version,
        from_generation: initial.generation,
        target: Some(target),
        boundary: Some(ScopeCursor {
            scope_id,
            version: next_version,
            generation: initial.generation,
            sequence: Sequence(12),
        }),
        kind: ScopeTransitionKind::FullBootstrap,
        additions: vec![MembershipRecord {
            scope_id,
            projection: ProjectionVersion(1),
            entity,
            membership_version: next_version,
        }],
        removals: Vec::new(),
        affected_pending_operations: Vec::new(),
        staging_complete: true,
    };
    let operation = OperationEnvelope {
        protocol_version: ProtocolVersion::V1,
        operation_id: OperationId::new(),
        tenant_id: tenant,
        actor_id: ActorId::new(),
        device_id: DeviceId::new(),
        entity,
        base_version: None,
        created_at: HybridTimestamp {
            physical_ms: 1,
            logical: 0,
            node: NodeId::new(),
        },
        schema_version: SchemaVersion(1),
        operation_kind: OperationKind(1),
        payload: b"scope contract".to_vec(),
        metadata: OperationMetadata::default(),
    };
    Ok((subscription, transition, operation))
}

/// Proves durable staging, atomic activation, transition idempotency, and outbox quarantine.
///
/// The supplied transition must be a complete bootstrap or expansion from `subscription`, include
/// at least one membership addition, and use `operation.operation_id` when the generated
/// revocation is applied.
///
/// # Errors
///
/// Returns a typed storage failure or semantic contract violation.
pub async fn verify_scope_state_store<S>(
    store: &S,
    subscription: Subscription,
    mut activation: ScopeTransition,
    operation: OperationEnvelope,
) -> Result<ScopeStateContractReport, AdapterContractError>
where
    S: ScopeStateStore + OutboxStore,
{
    store.install_subscription(&subscription).await?;
    store.append_operation(operation.clone()).await?;

    activation.staging_complete = false;
    let staged = store.apply_scope_transition(&activation).await?;
    let state = store.load_scope_state().await?;
    if staged.applied
        || state
            .subscription(subscription.subscription_id)
            .is_none_or(|item| {
                !matches!(
                    item.state,
                    SubscriptionState::Bootstrapping | SubscriptionState::Expanding
                )
            })
    {
        return Err(AdapterContractError::Violation(
            "incomplete scope staging became active",
        ));
    }

    activation.staging_complete = true;
    let applied = store.apply_scope_transition(&activation).await?;
    if !applied.applied {
        return Err(AdapterContractError::Violation(
            "complete scope transition did not activate",
        ));
    }
    let duplicate = store.apply_scope_transition(&activation).await?;
    if duplicate.applied {
        return Err(AdapterContractError::Violation(
            "scope transition retry was not idempotent",
        ));
    }

    let active = store.load_scope_state().await?;
    let current = active
        .subscription(subscription.subscription_id)
        .cloned()
        .ok_or(AdapterContractError::Violation(
            "activated subscription disappeared",
        ))?;
    if current.state != SubscriptionState::Active {
        return Err(AdapterContractError::Violation(
            "complete scope transition did not become active",
        ));
    }
    let revocation = ScopeTransition {
        transition_id: aequora_scope::ScopeTransitionId::new(),
        subscription_id: current.subscription_id,
        from_version: current.scope.version,
        from_generation: current.scope.generation,
        target: None,
        boundary: None,
        kind: aequora_scope::ScopeTransitionKind::Revocation,
        additions: Vec::new(),
        removals: Vec::new(),
        affected_pending_operations: vec![operation.operation_id],
        staging_complete: true,
    };
    let revoked: ScopeTransitionOutcome = store.apply_scope_transition(&revocation).await?;
    if !revoked.applied
        || store
            .pending_operations(1_024)
            .await?
            .iter()
            .any(|candidate| candidate.operation_id == operation.operation_id)
    {
        return Err(AdapterContractError::Violation(
            "revoked scope operation remained transmittable",
        ));
    }
    let final_state: LocalScopeState = store.load_scope_state().await?;
    if final_state
        .pending_disposition(operation.operation_id)
        .is_none()
    {
        return Err(AdapterContractError::Violation(
            "revocation did not retain pending-intent disposition",
        ));
    }
    Ok(ScopeStateContractReport {
        subscription: subscription.subscription_id,
        transition: activation.transition_id,
        quarantined_operation: operation.operation_id,
    })
}

/// Exercises acquisition exclusion, renewal, release, takeover fencing, and generation switching.
///
/// # Errors
///
/// Returns a typed violation when the adapter advertises no durable coordination or accepts a
/// stale leader/generation transition.
pub async fn verify_local_coordination<S>(
    store: &S,
) -> Result<LocalCoordinationContractReport, AdapterContractError>
where
    S: LocalCoordinationStore,
{
    if store.coordination_support() != LocalCoordinationSupport::Full {
        return Err(AdapterContractError::Violation(
            "multi-process compliance requires full local coordination support",
        ));
    }
    let initial = store.coordination_snapshot().await?;
    let first = store
        .acquire_lease(LeaseRequest {
            owner_id: ProcessInstanceId::new(),
            kind: LeaseKind::SyncCoordinator,
            now_unix_ms: 1_000,
            ttl_ms: 1_000,
        })
        .await?;
    if store
        .acquire_lease(LeaseRequest {
            owner_id: ProcessInstanceId::new(),
            kind: LeaseKind::SyncCoordinator,
            now_unix_ms: 1_001,
            ttl_ms: 1_000,
        })
        .await
        .is_ok()
    {
        return Err(AdapterContractError::Violation(
            "two processes acquired the same unexpired lease",
        ));
    }
    let renewed = store.renew_lease(first, 1_100, 1_000).await?;
    store.release_lease(renewed, 1_101).await?;
    let takeover = store
        .acquire_lease(LeaseRequest {
            owner_id: ProcessInstanceId::new(),
            kind: LeaseKind::SyncCoordinator,
            now_unix_ms: 1_102,
            ttl_ms: 1_000,
        })
        .await?;
    if takeover.fencing_token <= first.fencing_token
        || store.validate_fence(first, 1_103).await.is_ok()
    {
        return Err(AdapterContractError::Violation(
            "takeover did not monotonically fence the previous leader",
        ));
    }
    store.release_lease(takeover, 1_104).await?;
    let maintenance = store
        .acquire_lease(LeaseRequest {
            owner_id: ProcessInstanceId::new(),
            kind: LeaseKind::Maintenance,
            now_unix_ms: 1_105,
            ttl_ms: 1_000,
        })
        .await?;
    let generation = store.advance_store_generation(maintenance, 1_106).await?;
    if initial.store_id != maintenance.store_id || generation <= initial.store_generation {
        return Err(AdapterContractError::Violation(
            "maintenance generation switch changed store identity or failed to advance",
        ));
    }
    store
        .release_lease(
            aequora_coordination::LeaseGrant {
                store_generation: generation,
                ..maintenance
            },
            1_107,
        )
        .await?;
    Ok(LocalCoordinationContractReport {
        store_id: initial.store_id,
        fencing_token: maintenance.fencing_token,
        store_generation: generation,
    })
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

/// Evidence that two physically independent adapters produced one canonical integrity root.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct IntegrityAdapterContractReport {
    /// Canonical authority snapshot.
    pub authority: IntegritySnapshot,
    /// Canonical local authoritative-base snapshot.
    pub local: IntegritySnapshot,
}

/// Shared boundary and bounds for one cross-adapter integrity contract run.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct IntegrityContractRequest {
    /// Authority tenant whose canonical state is captured.
    pub tenant: TenantId,
    /// Synchronization scope represented by both adapters.
    pub scope: SyncScopeId,
    /// Exact authority/local cursor boundary.
    pub boundary: Cursor,
    /// Canonical hash and partition generation.
    pub generation: IntegrityGeneration,
    /// Deterministic bounded partition scheme.
    pub scheme: PartitionScheme,
    /// Maximum canonical entities either adapter may scan.
    pub max_entities: usize,
}

/// Verifies cross-adapter canonical root equality at exactly one shared boundary.
///
/// # Errors
///
/// Returns a typed violation when either adapter lacks integrity support or their canonical roots
/// differ despite representing the same synchronized state.
pub async fn verify_integrity_pair<A, L>(
    authority: &A,
    local: &L,
    request: IntegrityContractRequest,
) -> Result<IntegrityAdapterContractReport, AdapterContractError>
where
    A: AuthoritativeIntegritySource + IntegrityCapabilityProvider,
    L: LocalIntegrityStore + IntegrityCapabilityProvider,
{
    if authority.integrity_support() == IntegritySupport::None
        || local.integrity_support() == IntegritySupport::None
    {
        return Err(AdapterContractError::Violation(
            "integrity compliance requires canonical snapshot support",
        ));
    }
    let authority = authority
        .capture_authoritative_integrity(
            request.tenant,
            request.scope,
            request.boundary,
            request.generation,
            request.scheme,
            request.max_entities,
        )
        .await?;
    let local = local
        .capture_local_integrity(
            request.scope,
            request.boundary,
            request.generation,
            request.scheme,
            request.max_entities,
        )
        .await?;
    if authority.compare(&local).map_err(|_| {
        AdapterContractError::Violation("integrity manifests are not comparison-compatible")
    })? != IntegrityComparison::Match
    {
        return Err(AdapterContractError::Violation(
            "canonical roots differ across synchronized adapters",
        ));
    }
    Ok(IntegrityAdapterContractReport { authority, local })
}

/// Verifies repair preserves pending operations exactly and never advances the sync cursor.
///
/// # Errors
///
/// Returns a contract violation when the adapter loses/reorders pending intent or changes the
/// cursor as a side effect of integrity maintenance.
pub async fn verify_replica_repair<L>(
    local: &L,
    plan: &RepairPlan,
    replacements: &[CanonicalEntity],
    removals: &[EntityRef],
) -> Result<(), AdapterContractError>
where
    L: LocalIntegrityStore + OutboxStore + CursorStore,
{
    let pending_before = local.pending_operations(usize::MAX).await?;
    let cursor_before = local.load_cursor(plan.boundary.scope).await?;
    let report = local
        .repair_local_replica(plan, replacements, removals)
        .await?;
    let pending_after = local.pending_operations(usize::MAX).await?;
    let cursor_after = local.load_cursor(plan.boundary.scope).await?;
    if pending_before != pending_after {
        return Err(AdapterContractError::Violation(
            "integrity repair changed pending operation intent",
        ));
    }
    if cursor_before != cursor_after
        || cursor_after != Some(plan.boundary)
        || report.sync_cursor != plan.boundary
    {
        return Err(AdapterContractError::Violation(
            "integrity repair changed the normal synchronization cursor",
        ));
    }
    Ok(())
}

/// Isolated local-adapter fixture returned by a third-party compliance factory.
pub struct LocalAdapterContractFixture<S> {
    /// Fresh adapter instance or isolated namespace.
    pub store: S,
    /// Fresh operation envelope.
    pub operation: OperationEnvelope,
    /// Fresh synchronization scope.
    pub scope: SyncScopeId,
    /// Deterministic server timestamp used by reconciliation.
    pub server_time: HybridTimestamp,
}

/// Factory used by [`crate::aequora_local_store_compliance!`] in third-party adapter crates.
#[async_trait::async_trait]
pub trait LocalAdapterContractFactory {
    /// Concrete local adapter being certified.
    type Store: LocalStore + TransactionCapabilityProvider;

    /// Creates an isolated store and unique fixture identifiers.
    ///
    /// # Errors
    ///
    /// Returns setup/storage failures or a typed fixture invariant failure.
    async fn setup() -> Result<LocalAdapterContractFixture<Self::Store>, AdapterContractError>;
}

/// Isolated authority-adapter fixture returned by a third-party compliance factory.
pub struct AuthoritativeAdapterContractFixture<S> {
    /// Fresh adapter instance or isolated namespace.
    pub store: S,
    /// Fresh first-version authoritative commit.
    pub commit: CommitOperation,
}

/// Factory used by [`crate::aequora_authoritative_store_compliance!`] in adapter crates.
#[async_trait::async_trait]
pub trait AuthoritativeAdapterContractFactory {
    /// Concrete authority adapter being certified.
    type Store: AuthoritativeStore + TransactionCapabilityProvider;

    /// Creates an isolated store and unique first-version commit.
    ///
    /// # Errors
    ///
    /// Returns setup/storage failures or a typed fixture invariant failure.
    async fn setup()
    -> Result<AuthoritativeAdapterContractFixture<Self::Store>, AdapterContractError>;
}

/// Generates one Tokio test that runs the complete public local-store compliance contract.
///
/// The downstream adapter's dev-dependencies must include Tokio with macro/runtime support.
#[macro_export]
macro_rules! aequora_local_store_compliance {
    ($test_name:ident, $factory:ty) => {
        #[tokio::test]
        async fn $test_name() -> Result<(), Box<dyn std::error::Error>> {
            let fixture =
                <$factory as $crate::contracts::LocalAdapterContractFactory>::setup().await?;
            $crate::contracts::verify_local_store(
                &fixture.store,
                fixture.operation,
                fixture.scope,
                fixture.server_time,
            )
            .await?;
            Ok(())
        }
    };
}

/// Generates one Tokio test that runs the complete public authoritative-store contract.
///
/// The downstream adapter's dev-dependencies must include Tokio with macro/runtime support.
#[macro_export]
macro_rules! aequora_authoritative_store_compliance {
    ($test_name:ident, $factory:ty) => {
        #[tokio::test]
        async fn $test_name() -> Result<(), Box<dyn std::error::Error>> {
            let fixture =
                <$factory as $crate::contracts::AuthoritativeAdapterContractFactory>::setup()
                    .await?;
            $crate::contracts::verify_authoritative_store(&fixture.store, fixture.commit).await?;
            Ok(())
        }
    };
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
    if pending
        .iter()
        .find(|candidate| candidate.operation_id == operation_id)
        != Some(&operation)
    {
        return Err(AdapterContractError::Violation(
            "the replayable outbox must preserve operation lineage losslessly",
        ));
    }

    store.mark_sending(&[operation_id]).await?;
    verify_state(store, operation_id, OutboxState::Sending).await?;
    verify_retry_schedule(store, operation_id).await?;

    let cursor = Cursor::legacy(scope, Sequence(1));
    let event_id = EventId::new();
    let event_lineage = operation
        .metadata
        .lineage
        .derived(LineageRef::Operation(operation_id));
    let acknowledgement = OperationAck {
        operation_id,
        event_id,
        lineage: event_lineage,
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
            event_id,
            lineage: event_lineage,
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
    let altered_retry = CommitOperation {
        operation_lineage: LineageContext {
            correlation_id: CorrelationId::new(),
            caused_by: commit.operation_lineage.caused_by,
        },
        ..commit.clone()
    };
    if store.commit_operation(altered_retry).await.is_ok() {
        return Err(AdapterContractError::Violation(
            "a retry must not change lineage for an existing operation ID",
        ));
    }
    let mut altered_payload_retry = commit.clone();
    altered_payload_retry.payload.push(0xff);
    altered_payload_retry.command_digest = *blake3::hash(&altered_payload_retry.payload).as_bytes();
    if store.commit_operation(altered_payload_retry).await.is_ok() {
        return Err(AdapterContractError::Violation(
            "a retry must not change canonical payload for an existing operation ID",
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
    verify_correlation_lookup(store, &commit).await?;
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

async fn verify_correlation_lookup<S>(
    store: &S,
    commit: &CommitOperation,
) -> Result<(), AdapterContractError>
where
    S: CorrelationLog,
{
    let page = store
        .read_correlation(
            commit.tenant_id,
            commit.operation_lineage.correlation_id,
            AuditOffset(0),
            1_024,
        )
        .await?;
    if page.records.len() != 1
        || page.records[0].operation_id != commit.operation_id
        || page.records[0].operation_lineage != commit.operation_lineage
    {
        return Err(AdapterContractError::Violation(
            "correlation lookup must return the exact durable lineage record",
        ));
    }
    let other_tenant = store
        .read_correlation(
            TenantId::new(),
            commit.operation_lineage.correlation_id,
            AuditOffset(0),
            1_024,
        )
        .await?;
    if !other_tenant.records.is_empty() {
        return Err(AdapterContractError::Violation(
            "correlation lookup must not cross tenant boundaries",
        ));
    }
    Ok(())
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
        authority: None,
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
        authority: None,
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
        authority: None,
        operation_id: OperationId::new(),
        entity,
        expected_version: None,
        next_version: EntityVersion::INITIAL,
        payload: [baseline.payload.as_slice(), b"-race-left"].concat(),
        ..baseline.clone()
    };
    left_commit.command_digest = *blake3::hash(&left_commit.payload).as_bytes();
    let mut right_commit = CommitOperation {
        authority: None,
        operation_id: OperationId::new(),
        payload: [baseline.payload.as_slice(), b"-race-right"].concat(),
        ..left_commit.clone()
    };
    right_commit.command_digest = *blake3::hash(&right_commit.payload).as_bytes();

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
        && left.event_id == right.event_id
        && left.lineage == right.lineage
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
        || matching[0].event_id != acknowledgement.event_id
        || matching[0].lineage != acknowledgement.lineage
        || matching[0].event_id != commit.event_id
        || matching[0].lineage != commit.event_lineage()
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
    if matching.len() != 1
        || matching[0].event_id != commit.event_id
        || matching[0].operation_lineage != commit.operation_lineage
        || matching[0].command_digest != commit.command_digest
    {
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
