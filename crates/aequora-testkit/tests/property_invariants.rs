use aequora_conflict::{FieldSet, FieldValue};
use aequora_journal::{CompactionInputs, plan_compaction};
use aequora_protocol::{
    ChangeKind, OperationAck, OperationEnvelope, OperationKind, OperationMetadata, RemoteChange,
    SnapshotEntity, SyncDirective, SyncResponse,
};
use aequora_store::{
    CommitOperation, CommitOutcome, CursorStore, OperationLedger, OutboxState, OutboxStateStore,
    OutboxStore, ReconciliationStore,
};
use aequora_testkit::{InMemoryAuthoritativeStore, InMemoryLocalStore};
use aequora_types::{
    ActorId, Cursor, DeviceId, EntityId, EntityRef, EntityType, EntityVersion, HybridTimestamp,
    NodeId, OperationId, ProtocolVersion, SchemaVersion, Sequence, SyncScopeId, TenantId,
};
use proptest::prelude::*;

fn field_set(values: Vec<(u16, i64, Vec<u8>)>, node: NodeId) -> FieldSet {
    FieldSet::canonical(
        values
            .into_iter()
            .map(|(field, physical_ms, value)| FieldValue {
                field,
                timestamp: HybridTimestamp {
                    physical_ms,
                    logical: 0,
                    node,
                },
                value,
            })
            .collect(),
    )
}

fn timestamp(physical_ms: i64) -> HybridTimestamp {
    HybridTimestamp {
        physical_ms,
        logical: 0,
        node: NodeId::new(),
    }
}

fn operation(tenant: TenantId, entity: EntityRef, operation_id: OperationId) -> OperationEnvelope {
    OperationEnvelope {
        protocol_version: ProtocolVersion::V1,
        operation_id,
        tenant_id: tenant,
        actor_id: ActorId::new(),
        device_id: DeviceId::new(),
        entity,
        base_version: None,
        created_at: timestamp(1),
        schema_version: SchemaVersion(1),
        operation_kind: OperationKind(1),
        payload: b"generated".to_vec(),
        metadata: OperationMetadata::default(),
    }
}

fn entity() -> EntityRef {
    EntityRef {
        entity_type: EntityType::new(1).unwrap_or_else(|error| panic!("{error}")),
        entity_id: EntityId::new(),
    }
}

fn response(
    tenant: TenantId,
    scope: SyncScopeId,
    operation_id: OperationId,
    entity: EntityRef,
    sequence: u64,
) -> SyncResponse {
    SyncResponse {
        protocol: ProtocolVersion::V1,
        directive: SyncDirective::Continue,
        acknowledged: vec![OperationAck {
            operation_id,
            entity_version: EntityVersion::INITIAL,
            sequence: Sequence(sequence),
            duplicate: false,
        }],
        rejected: Vec::new(),
        conflicts: Vec::new(),
        changes: vec![RemoteChange {
            tenant_id: tenant,
            scope_id: scope,
            sequence: Sequence(sequence),
            operation_id,
            entity,
            version: EntityVersion::INITIAL,
            change_kind: ChangeKind::Upsert,
            payload: b"authoritative".to_vec(),
            timestamp: timestamp(2),
        }],
        next_cursor: Cursor {
            scope,
            sequence: Sequence(sequence),
        },
        has_more: false,
        server_time: timestamp(2),
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(32))]

    #[test]
    fn entity_versions_strictly_increase_without_wrapping(
        start in 1_u64..=u64::MAX - 128,
        advances in 0_usize..128,
    ) {
        let mut version = EntityVersion::new(start)
            .unwrap_or_else(|error| panic!("{error}"));
        for _ in 0..advances {
            let next = version.checked_next()
                .unwrap_or_else(|| panic!("version unexpectedly overflowed"));
            prop_assert!(next > version);
            version = next;
        }
    }

    #[test]
    fn field_merge_is_commutative_and_idempotent(
        left in prop::collection::vec((1_u16..32, 0_i64..10_000, prop::collection::vec(any::<u8>(), 0..16)), 0..32),
        right in prop::collection::vec((1_u16..32, 0_i64..10_000, prop::collection::vec(any::<u8>(), 0..16)), 0..32),
    ) {
        let node = NodeId::new();
        let left = field_set(left, node);
        let right = field_set(right, node);
        prop_assert_eq!(left.merge(&right), right.merge(&left));
        prop_assert_eq!(left.merge(&left), left);
    }

    #[test]
    fn compaction_never_crosses_any_safety_watermark(
        snapshot in 0_u64..u64::MAX,
        active in 0_u64..u64::MAX,
        retention in 0_u64..u64::MAX,
        audit in prop::option::of(0_u64..u64::MAX),
    ) {
        let plan = plan_compaction(CompactionInputs {
            snapshot_sequence: Sequence(snapshot),
            minimum_active_cursor: Some(Sequence(active)),
            retention_sequence: Sequence(retention),
            audit_sequence: audit.map(Sequence),
        });
        if let Some(plan) = plan {
            prop_assert!(plan.through.0 <= snapshot);
            prop_assert!(plan.through.0 <= active);
            prop_assert!(plan.through.0 <= retention);
            if let Some(audit) = audit {
                prop_assert!(plan.through.0 <= audit);
            }
        }
    }

    #[test]
    fn retrying_one_operation_id_never_commits_state_twice(retries in 1_usize..16) {
        let runtime = tokio::runtime::Runtime::new()
            .unwrap_or_else(|error| panic!("{error}"));
        runtime.block_on(async move {
            let store = InMemoryAuthoritativeStore::default();
            let tenant = TenantId::new();
            let entity = entity();
            let operation_id = OperationId::new();
            let payload = b"one logical effect".to_vec();
            let commit = CommitOperation {
                operation_id,
                actor_id: ActorId::new(),
                device_id: DeviceId::new(),
                operation_kind: 1,
                tenant_id: tenant,
                scope_id: SyncScopeId::new(),
                entity,
                expected_version: None,
                next_version: EntityVersion::INITIAL,
                payload: payload.clone(),
                change_kind: ChangeKind::Upsert,
                timestamp: timestamp(1),
                command_digest: *blake3::hash(&payload).as_bytes(),
            };
            let first = store.commit_operation(commit.clone()).await
                .unwrap_or_else(|error| panic!("{error}"));
            prop_assert!(matches!(first, CommitOutcome::Applied(_)));
            for _ in 0..retries {
                let repeated = store.commit_operation(commit.clone()).await
                    .unwrap_or_else(|error| panic!("{error}"));
                prop_assert!(matches!(repeated, CommitOutcome::Duplicate(_)));
            }
            prop_assert_eq!(store.applied_operation_count(), 1);
            prop_assert_eq!(store.journal_len(), 1);
            prop_assert_eq!(store.audit_len(), 1);
            Ok(())
        })?;
    }

    #[test]
    fn cursor_never_moves_backward(start in 1_u64..10_000, regression in 0_u64..10_000) {
        prop_assume!(regression < start);
        let runtime = tokio::runtime::Runtime::new()
            .unwrap_or_else(|error| panic!("{error}"));
        runtime.block_on(async move {
            let store = InMemoryLocalStore::default();
            let scope = SyncScopeId::new();
            let tenant = TenantId::new();
            let operation_id = OperationId::new();
            let entity = entity();
            store.reconcile(&response(tenant, scope, operation_id, entity, start)).await
                .unwrap_or_else(|error| panic!("{error}"));
            let regressing = SyncResponse {
                protocol: ProtocolVersion::V1,
                directive: SyncDirective::Continue,
                acknowledged: Vec::new(),
                rejected: Vec::new(),
                conflicts: Vec::new(),
                changes: Vec::new(),
                next_cursor: Cursor { scope, sequence: Sequence(regression) },
                has_more: false,
                server_time: timestamp(3),
            };
            prop_assert!(store.reconcile(&regressing).await.is_err());
            prop_assert_eq!(
                store.load_cursor(scope).await.unwrap_or_else(|error| panic!("{error}")),
                Some(Cursor { scope, sequence: Sequence(start) })
            );
            Ok(())
        })?;
    }

    #[test]
    fn acknowledged_operations_leave_the_replayable_outbox(count in 1_usize..32) {
        let runtime = tokio::runtime::Runtime::new()
            .unwrap_or_else(|error| panic!("{error}"));
        runtime.block_on(async move {
            let store = InMemoryLocalStore::default();
            let tenant = TenantId::new();
            let scope = SyncScopeId::new();
            let mut acknowledgements = Vec::new();
            let mut operation_ids = Vec::new();
            for index in 0..count {
                let operation_id = OperationId::new();
                store.append_operation(operation(tenant, entity(), operation_id)).await
                    .unwrap_or_else(|error| panic!("{error}"));
                operation_ids.push(operation_id);
                acknowledgements.push(OperationAck {
                    operation_id,
                    entity_version: EntityVersion::INITIAL,
                    sequence: Sequence(u64::try_from(index).unwrap_or(u64::MAX).saturating_add(1)),
                    duplicate: false,
                });
            }
            let reconciliation = SyncResponse {
                protocol: ProtocolVersion::V1,
                directive: SyncDirective::Continue,
                acknowledged: acknowledgements,
                rejected: Vec::new(),
                conflicts: Vec::new(),
                changes: Vec::new(),
                next_cursor: Cursor { scope, sequence: Sequence(u64::try_from(count).unwrap_or(u64::MAX)) },
                has_more: false,
                server_time: timestamp(3),
            };
            store.reconcile(&reconciliation).await
                .unwrap_or_else(|error| panic!("{error}"));
            let pending = store
                .pending_operations(count)
                .await
                .unwrap_or_else(|error| panic!("{error}"));
            prop_assert!(pending.is_empty());
            for operation_id in operation_ids {
                prop_assert_eq!(
                    store.operation_state(operation_id).await
                        .unwrap_or_else(|error| panic!("{error}")),
                    Some(OutboxState::Acknowledged)
                );
            }
            Ok(())
        })?;
    }

    #[test]
    fn one_authoritative_event_cannot_apply_twice(replays in 1_usize..16) {
        let runtime = tokio::runtime::Runtime::new()
            .unwrap_or_else(|error| panic!("{error}"));
        runtime.block_on(async move {
            let store = InMemoryLocalStore::default();
            let tenant = TenantId::new();
            let scope = SyncScopeId::new();
            let entity = entity();
            let operation_id = OperationId::new();
            let response = response(tenant, scope, operation_id, entity, 1);
            for _ in 0..replays {
                store.reconcile(&response).await
                    .unwrap_or_else(|error| panic!("{error}"));
            }
            prop_assert_eq!(
                store.entity(entity),
                Some(SnapshotEntity {
                    entity,
                    version: EntityVersion::INITIAL,
                    payload: b"authoritative".to_vec(),
                    tombstone: false,
                })
            );
            prop_assert_eq!(
                store.load_cursor(scope).await.unwrap_or_else(|error| panic!("{error}")),
                Some(Cursor { scope, sequence: Sequence(1) })
            );
            Ok(())
        })?;
    }
}
