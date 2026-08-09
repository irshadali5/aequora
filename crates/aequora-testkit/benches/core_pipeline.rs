use aequora_codec::MessageKind;
use aequora_compute::{ComputeConfig, ComputePool};
use aequora_conflict::check_version;
use aequora_executor::plan_dependencies;
use aequora_journal::{CompactionInputs, plan_compaction};
use aequora_protocol::{
    Capability, ClientLimits, OperationEnvelope, OperationKind, OperationMetadata, SessionMetadata,
    SyncDirective, SyncRequest, SyncResponse,
};
use aequora_store::{ChangeJournal, ReconciliationStore, SnapshotStore};
use aequora_testkit::{InMemoryAuthoritativeStore, InMemoryLocalStore};
use aequora_types::{
    ActorId, Cursor, DeviceId, EntityId, EntityRef, EntityType, EntityVersion, HybridTimestamp,
    NodeId, OperationId, ProtocolVersion, RequestId, SchemaVersion, Sequence, SessionId,
    SyncScopeId, TenantId,
};
use aequora_validator::{ProtocolLimits, validate_request};
use criterion::{BatchSize, Criterion, black_box, criterion_group, criterion_main};

fn session() -> SessionMetadata {
    SessionMetadata {
        session_id: SessionId::new(),
        device_id: DeviceId::new(),
        actor_id: ActorId::new(),
        tenant_id: TenantId::new(),
        scope_id: SyncScopeId::new(),
        partitions: Vec::new(),
    }
}

fn operation(session: &SessionMetadata, dependency: Option<OperationId>) -> OperationEnvelope {
    OperationEnvelope {
        protocol_version: ProtocolVersion::V1,
        operation_id: OperationId::new(),
        tenant_id: session.tenant_id,
        actor_id: session.actor_id,
        device_id: session.device_id,
        entity: EntityRef {
            entity_type: EntityType::new(1).unwrap_or_else(|error| panic!("{error}")),
            entity_id: EntityId::new(),
        },
        base_version: None,
        created_at: HybridTimestamp {
            physical_ms: 1,
            logical: 0,
            node: NodeId::new(),
        },
        schema_version: SchemaVersion(1),
        operation_kind: OperationKind(1),
        payload: vec![7; 128],
        metadata: OperationMetadata {
            trace_id: None,
            dependencies: dependency.into_iter().collect(),
        },
    }
}

fn request(operation_count: usize) -> SyncRequest {
    let session = session();
    let mut operations = Vec::with_capacity(operation_count);
    let mut dependency = None;
    for _ in 0..operation_count {
        let next = operation(&session, dependency);
        dependency = Some(next.operation_id);
        operations.push(next);
    }
    SyncRequest {
        protocol: ProtocolVersion::V1,
        request_id: RequestId::new(),
        session,
        cursor: None,
        operations,
        limits: ClientLimits::default(),
        capabilities: vec![Capability::PostcardV1],
    }
}

fn empty_response(scope: SyncScopeId) -> SyncResponse {
    SyncResponse {
        protocol: ProtocolVersion::V1,
        directive: SyncDirective::Continue,
        acknowledged: Vec::new(),
        rejected: Vec::new(),
        conflicts: Vec::new(),
        changes: Vec::new(),
        next_cursor: Cursor {
            scope,
            sequence: Sequence(0),
        },
        has_more: false,
        server_time: HybridTimestamp {
            physical_ms: 1,
            logical: 0,
            node: NodeId::new(),
        },
    }
}

fn core_pipeline(criterion: &mut Criterion) {
    let request = request(256);
    let frame = aequora_codec::encode(ProtocolVersion::V1, MessageKind::SyncRequest, &request)
        .unwrap_or_else(|error| panic!("{error}"));
    criterion.bench_function("postcard_encode_256_operations", |bencher| {
        bencher.iter(|| {
            aequora_codec::encode(
                ProtocolVersion::V1,
                MessageKind::SyncRequest,
                black_box(&request),
            )
        });
    });
    criterion.bench_function("postcard_decode_256_operations", |bencher| {
        bencher.iter(|| {
            aequora_codec::decode::<SyncRequest>(
                black_box(&frame),
                MessageKind::SyncRequest,
                frame.len(),
            )
        });
    });
    criterion.bench_function("structural_validation_256_operations", |bencher| {
        bencher.iter_batched(
            || request.clone(),
            |request| validate_request(request, ProtocolLimits::default()),
            BatchSize::SmallInput,
        );
    });
    criterion.bench_function("dependency_sort_256_operations", |bencher| {
        bencher.iter(|| plan_dependencies(black_box(&request.operations)));
    });
    criterion.bench_function("conflict_version_check", |bencher| {
        bencher.iter(|| {
            check_version(
                black_box(Some(EntityVersion::INITIAL)),
                black_box(Some(EntityVersion::INITIAL)),
            )
        });
    });
    criterion.bench_function("journal_compaction_plan", |bencher| {
        bencher.iter(|| {
            plan_compaction(black_box(CompactionInputs {
                snapshot_sequence: Sequence(10_000),
                minimum_active_cursor: Some(Sequence(9_000)),
                retention_sequence: Sequence(8_000),
                audit_sequence: Some(Sequence(7_000)),
            }))
        });
    });
    let compute = ComputePool::new(ComputeConfig {
        worker_threads: 2,
        parallel_threshold: 128,
    })
    .unwrap_or_else(|error| panic!("{error}"));
    criterion.bench_function("rayon_threshold_decision", |bencher| {
        bencher.iter(|| compute.should_parallelize(black_box(256)));
    });

    let runtime = tokio::runtime::Runtime::new().unwrap_or_else(|error| panic!("{error}"));
    let local = InMemoryLocalStore::default();
    let response = empty_response(request.session.scope_id);
    criterion.bench_function("atomic_reconciliation_empty_page", |bencher| {
        bencher
            .to_async(&runtime)
            .iter(|| local.reconcile(&response));
    });
    let authoritative = InMemoryAuthoritativeStore::default();
    criterion.bench_function("journal_query_empty_page", |bencher| {
        bencher.to_async(&runtime).iter(|| {
            authoritative.read_changes_after(
                request.session.tenant_id,
                request.session.scope_id,
                Sequence(0),
                1_024,
                4 * 1_024 * 1_024,
            )
        });
    });
    criterion.bench_function("snapshot_construction_empty_scope", |bencher| {
        bencher.to_async(&runtime).iter_batched(
            InMemoryAuthoritativeStore::default,
            |store| async move {
                store
                    .create_snapshot(request.session.tenant_id, request.session.scope_id, &[])
                    .await
            },
            BatchSize::SmallInput,
        );
    });
}

criterion_group!(benches, core_pipeline);
criterion_main!(benches);
