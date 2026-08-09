use aequora_protocol::ChangeKind;
use aequora_store::{AuditOffset, CommitOperation};
use aequora_store_postgres::{
    POSTGRES_SCHEMA_VERSION, PostgresBackend, PostgresPoolConfig, PostgresStore,
    SqlxPostgresBackend,
};
use aequora_testkit::contracts::verify_authoritative_store;
use aequora_types::{
    ActorId, DeviceId, EntityId, EntityRef, EntityType, EntityVersion, HybridTimestamp, NodeId,
    OperationId, Sequence, SyncScopeId, TenantId,
};

#[tokio::test]
async fn authoritative_transaction_snapshot_and_compaction_are_real() {
    let Ok(database_url) = std::env::var("AEQUORA_TEST_POSTGRES_URL") else {
        return;
    };
    let backend = SqlxPostgresBackend::connect_with_migration_url(
        &database_url,
        &database_url,
        PostgresPoolConfig::new(2),
    )
    .await
    .unwrap_or_else(|error| panic!("{error}"));
    exercise_backend(&backend).await;
}

#[tokio::test]
async fn neon_pooled_runtime_and_direct_migration_endpoints_are_real() {
    let (Ok(pooled_url), Ok(direct_url)) = (
        std::env::var("AEQUORA_TEST_NEON_POOLED_URL"),
        std::env::var("AEQUORA_TEST_NEON_DIRECT_URL"),
    ) else {
        return;
    };
    let backend = SqlxPostgresBackend::connect_neon(&pooled_url, &direct_url, 2)
        .await
        .unwrap_or_else(|error| panic!("{error}"));
    exercise_backend(&backend).await;
}

async fn exercise_backend(backend: &SqlxPostgresBackend) {
    verify_schema(backend).await;
    let tenant = TenantId::new();
    let scope = SyncScopeId::new();
    let operation_id = OperationId::new();
    let entity = EntityRef {
        entity_type: EntityType::new(7).unwrap_or_else(|error| panic!("{error}")),
        entity_id: EntityId::new(),
    };
    let payload = b"authoritative attendance".to_vec();
    let timestamp = HybridTimestamp {
        physical_ms: 123,
        logical: 1,
        node: NodeId::new(),
    };
    let commit = CommitOperation {
        operation_id,
        actor_id: ActorId::new(),
        device_id: DeviceId::new(),
        operation_kind: 9,
        tenant_id: tenant,
        scope_id: scope,
        entity,
        expected_version: None,
        next_version: EntityVersion::INITIAL,
        payload: payload.clone(),
        change_kind: ChangeKind::Upsert,
        timestamp,
        command_digest: *blake3::hash(&payload).as_bytes(),
    };

    let store = PostgresStore::new(backend.clone());
    let report = verify_authoritative_store(&store, commit)
        .await
        .unwrap_or_else(|error| panic!("{error}"));
    assert_eq!(report.acknowledgement.operation_id, operation_id);

    verify_data_path(backend, tenant, scope, operation_id, entity, payload).await;
}

async fn verify_schema(backend: &SqlxPostgresBackend) {
    backend
        .migrate()
        .await
        .unwrap_or_else(|error| panic!("{error}"));
    backend
        .health_check()
        .await
        .unwrap_or_else(|error| panic!("{error}"));
    let schema = backend
        .schema_status()
        .await
        .unwrap_or_else(|error| panic!("{error}"));
    assert!(schema.is_current());
    assert_eq!(schema.applied_version, POSTGRES_SCHEMA_VERSION);
}

async fn verify_data_path(
    backend: &SqlxPostgresBackend,
    tenant: TenantId,
    scope: SyncScopeId,
    operation_id: OperationId,
    entity: EntityRef,
    payload: Vec<u8>,
) {
    let stored = backend
        .read_entity(tenant, entity)
        .await
        .unwrap_or_else(|error| panic!("{error}"))
        .unwrap_or_else(|| panic!("entity missing"));
    assert_eq!(stored.current.payload, payload);
    let page = backend
        .read_changes_after(tenant, scope, Sequence(0), 10, 1024)
        .await
        .unwrap_or_else(|error| panic!("{error}"));
    assert_eq!(page.changes.len(), 1);

    let descriptor = backend
        .create_snapshot(tenant, scope, &[])
        .await
        .unwrap_or_else(|error| panic!("{error}"));
    let snapshot = backend
        .read_snapshot(tenant, descriptor.snapshot_id, 0, 10, 1024)
        .await
        .unwrap_or_else(|error| panic!("{error}"));
    assert_eq!(snapshot.entities.len(), 1);
    assert_eq!(snapshot.descriptor.cursor.sequence, Sequence(1));

    let removed = backend
        .compact_journal(tenant, scope, Sequence(1))
        .await
        .unwrap_or_else(|error| panic!("{error}"));
    assert_eq!(removed, 1);
    assert_eq!(
        backend
            .minimum_retained_cursor(tenant, scope)
            .await
            .unwrap_or_else(|error| panic!("{error}")),
        Sequence(1)
    );
    assert!(
        backend
            .operation_result(tenant, operation_id)
            .await
            .unwrap_or_else(|error| panic!("{error}"))
            .is_some()
    );
    let audit = backend
        .read_audit_after(tenant, AuditOffset(0), 10)
        .await
        .unwrap_or_else(|error| panic!("{error}"));
    assert_eq!(audit.records.len(), 1);
    assert_eq!(
        audit.records[0].command_digest,
        *blake3::hash(&payload).as_bytes()
    );
}
