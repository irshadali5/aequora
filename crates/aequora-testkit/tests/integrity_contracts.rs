use aequora_integrity::{
    CURRENT_HASH_SCHEMA, CURRENT_INTEGRITY_GENERATION, CanonicalEntity, PartitionScheme,
    RepairPlan, RepairStrategy,
};
use aequora_protocol::{
    ChangeKind, OperationEnvelope, OperationKind, OperationMetadata, SyncDirective, SyncResponse,
};
use aequora_queue::{
    CompactionPolicy, OperationClassId, OperationOptimization, OptimizationRegistry, RebasePolicy,
    RebaseTarget,
};
use aequora_store::{
    ChangeJournal, CommitOperation, CommitOutcome, OperationLedger, OutboxStore,
    ReconciliationStore,
};
use aequora_testkit::{
    InMemoryAuthoritativeStore, InMemoryLocalStore,
    contracts::{IntegrityContractRequest, verify_integrity_pair, verify_replica_repair},
};
use aequora_types::{
    ActorId, Cursor, DeviceId, EntityId, EntityRef, EntityType, EntityVersion, EventId,
    HybridTimestamp, NodeId, OperationId, ProtocolVersion, SchemaVersion, Sequence, SyncScopeId,
    TenantId,
};

fn operation(
    tenant: TenantId,
    scope: SyncScopeId,
    entity: EntityRef,
    payload: &[u8],
) -> OperationEnvelope {
    let _ = scope;
    OperationEnvelope {
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
        payload: payload.to_vec(),
        metadata: OperationMetadata::default(),
    }
}

async fn verify_rebase_after_repair(
    local: &InMemoryLocalStore,
    entity: EntityRef,
    pending_id: OperationId,
    pending_kind: OperationKind,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut registry = OptimizationRegistry::default();
    registry.register(
        pending_kind,
        OperationOptimization {
            compaction: CompactionPolicy::Never,
            rebase: RebasePolicy::ReapplyIntent,
            operation_class: OperationClassId(pending_kind.0),
            ..OperationOptimization::default()
        },
    );
    let rebase = local
        .rebase_outbox(
            &registry,
            &[RebaseTarget {
                entity,
                version: Some(EntityVersion::INITIAL),
                changed_fields: std::collections::BTreeSet::new(),
            }],
            100,
        )
        .await?;
    assert_eq!(rebase.rewrites[0].operation_id, pending_id);
    Ok(())
}

#[tokio::test]
async fn reference_adapters_share_root_and_repair_preserves_pending_intent()
-> Result<(), Box<dyn std::error::Error>> {
    let authority = InMemoryAuthoritativeStore::default();
    let local = InMemoryLocalStore::default();
    let tenant = TenantId::new();
    let scope = SyncScopeId::new();
    let entity = EntityRef {
        entity_type: EntityType::new(9)?,
        entity_id: EntityId::new(),
    };
    let initial_operation = operation(tenant, scope, entity, b"canonical-state");
    let commit = CommitOperation {
        authority: None,
        operation_id: initial_operation.operation_id,
        event_id: EventId::new(),
        operation_lineage: initial_operation.metadata.lineage,
        actor_id: initial_operation.actor_id,
        device_id: initial_operation.device_id,
        operation_kind: initial_operation.operation_kind.0,
        tenant_id: tenant,
        scope_id: scope,
        entity,
        expected_version: None,
        next_version: EntityVersion::INITIAL,
        payload: initial_operation.payload.clone(),
        change_kind: ChangeKind::Upsert,
        timestamp: initial_operation.created_at,
        command_digest: *blake3::hash(&initial_operation.payload).as_bytes(),
    };
    let CommitOutcome::Applied(acknowledgement) = authority.commit_operation(commit).await? else {
        return Err("fresh integrity fixture commit was not applied".into());
    };
    let page = authority
        .read_changes_after(tenant, scope, Sequence(0), 10, 1_024)
        .await?;
    let boundary = Cursor::legacy(scope, acknowledgement.sequence);
    local
        .reconcile(&SyncResponse {
            protocol: ProtocolVersion::V1,
            directive: SyncDirective::Continue,
            acknowledged: vec![acknowledgement],
            rejected: Vec::new(),
            conflicts: Vec::new(),
            changes: page.changes,
            next_cursor: boundary,
            has_more: false,
            server_time: initial_operation.created_at,
        })
        .await?;

    let scheme = PartitionScheme::new(8)?;
    let report = verify_integrity_pair(
        &authority,
        &local,
        IntegrityContractRequest {
            tenant,
            scope,
            boundary,
            generation: CURRENT_INTEGRITY_GENERATION,
            scheme,
            max_entities: 100,
        },
    )
    .await?;
    assert_eq!(
        report.authority.manifest.root_hash,
        report.local.manifest.root_hash
    );

    let pending = operation(tenant, scope, entity, b"pending-user-intent");
    let pending_id = pending.operation_id;
    let pending_kind = pending.operation_kind;
    local.append_operation(pending).await?;
    let plan = RepairPlan {
        repair_id: aequora_types::RepairId::new(),
        boundary,
        affected_entities: vec![entity],
        strategy: RepairStrategy::ReplaceEntities,
    };
    let replacement = CanonicalEntity {
        entity,
        version: EntityVersion::INITIAL,
        hash_schema: CURRENT_HASH_SCHEMA,
        payload: b"canonical-state".to_vec(),
        tombstone: false,
    };
    verify_replica_repair(&local, &plan, &[replacement], &[]).await?;
    verify_rebase_after_repair(&local, entity, pending_id, pending_kind).await?;
    Ok(())
}
