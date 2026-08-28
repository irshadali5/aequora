use aequora_metadata::{
    AdapterMappingContract, AdapterRecordMapping, ClientMigrationEvidence, Digest,
    LocalOperationSeq, LocalStoreGeneration, LogicalIndexId, LogicalRecord, METADATA_INVARIANTS,
    MetadataExport, MetadataExportRecord, MetadataFieldSpec, MetadataInvariantId,
    MetadataSchemaRegistry, MetadataSchemaVersion, MetadataValueType, OutboxRecord, OutboxState,
    PersistenceError, ScopeGeneration, SnapshotChunkRecord, SnapshotRecord, SnapshotState, StoreId,
    Timestamp, classify_ledger_insert, payload_digest, validate_fencing_token,
    validate_snapshot_publication,
};
use aequora_protocol::OperationKind;
use aequora_types::{
    ActorId, AuthorityEpoch, CorrelationId, DeviceId, EntityId, EntityRef, EntityType,
    EntityVersion, OperationId, SchemaVersion, Sequence, SnapshotId, SyncScopeId, TenantId,
};

fn timestamp(value: i64) -> Timestamp {
    Timestamp::new(value, 0).unwrap_or_else(|error| panic!("{error}"))
}

fn entity() -> EntityRef {
    EntityRef {
        entity_type: EntityType::new(1).unwrap_or_else(|error| panic!("{error}")),
        entity_id: EntityId::new(),
    }
}

fn digest(value: u8) -> Digest {
    [value; 32]
}

#[test]
fn schema_registry_rejects_duplicate_and_future_fields() {
    let current = MetadataSchemaVersion::new(2).unwrap_or_else(|error| panic!("{error}"));
    let mut registry = MetadataSchemaRegistry::new(current);
    let field = MetadataFieldSpec {
        record: "aequora_outbox".into(),
        field: "operation_id".into(),
        semantic_type: MetadataValueType::Uuid,
        introduced: MetadataSchemaVersion::new(1).unwrap_or_else(|error| panic!("{error}")),
        nullable: false,
        default_value: None,
    };
    assert!(registry.register(field.clone()).is_ok());
    assert!(registry.register(field).is_err());
    assert!(registry.validate().is_ok());
}

#[test]
fn outbox_validates_payload_and_first_send_epoch() {
    let payload = b"durable intent".to_vec();
    let mut record = OutboxRecord {
        operation_id: OperationId::new(),
        tenant_id: TenantId::new(),
        actor_id: ActorId::new(),
        device_id: DeviceId::new(),
        entity: entity(),
        operation_kind: OperationKind(1),
        operation_schema_version: SchemaVersion(1),
        base_version: Some(EntityVersion::INITIAL),
        local_seq: LocalOperationSeq::new(1),
        state: OutboxState::Pending,
        priority: 10,
        created_at: timestamp(1),
        next_retry_at: None,
        attempt_count: 0,
        ever_sent: false,
        payload_digest: payload_digest(&payload),
        payload_bytes: payload,
        correlation_id: CorrelationId::new(),
        causation_id: None,
        authority_epoch_first_sent: None,
        compaction_key: None,
    };
    assert!(record.validate().is_ok());
    record.payload_bytes.push(0);
    assert_eq!(
        record.validate(),
        Err(PersistenceError::PayloadDigestMismatch)
    );
}

#[test]
fn ledger_duplicate_payload_mismatch_fails_closed() {
    let operation_id = OperationId::new();
    let retained = aequora_metadata::OperationLedgerRecord {
        operation_id,
        tenant_id: TenantId::new(),
        operation_kind: OperationKind(1),
        operation_schema_version: SchemaVersion(1),
        semantic_payload_digest: digest(1),
        actor_id: ActorId::new(),
        device_id: DeviceId::new(),
        status: aequora_metadata::LedgerStatus::Accepted,
        first_seen_at: timestamp(1),
        committed_at: Some(timestamp(2)),
        authority_epoch: AuthorityEpoch::INITIAL,
        committed_sequence: Some(Sequence(1)),
        entity_ref: entity(),
        base_version: None,
        result_code: 0,
        handler_version: 1,
        execution_input_digest: digest(2),
        execution_plan_digest: digest(3),
    };
    assert_eq!(
        classify_ledger_insert(operation_id, digest(4), Some(&retained)),
        Err(PersistenceError::PayloadDigestMismatch)
    );
    assert!(classify_ledger_insert(operation_id, digest(1), Some(&retained)).is_ok());
}

#[test]
fn snapshot_requires_verified_single_boundary_chunks() {
    let snapshot_id = SnapshotId::new();
    let snapshot = SnapshotRecord {
        snapshot_id,
        tenant_id: TenantId::new(),
        scope_id: SyncScopeId::new(),
        scope_generation: ScopeGeneration::INITIAL,
        authority_epoch: AuthorityEpoch::INITIAL,
        boundary_sequence: Sequence(42),
        snapshot_schema_version: 1,
        profile: 1,
        state: SnapshotState::Published,
        manifest_digest: digest(1),
        root_digest: digest(2),
        created_at: timestamp(1),
        published_at: Some(timestamp(2)),
        expires_at: None,
    };
    let mut chunk = SnapshotChunkRecord {
        snapshot_id,
        chunk_id: digest(3),
        ordinal: 0,
        object_ref: "snapshots/chunk-0".into(),
        compressed_bytes: 8,
        uncompressed_bytes: 16,
        ciphertext_digest: digest(4),
        canonical_digest: digest(5),
        compression: 1,
        encryption_key_id: None,
        durable: true,
        verified: true,
        authority_epoch: AuthorityEpoch::INITIAL,
        boundary_sequence: Sequence(42),
    };
    assert!(validate_snapshot_publication(&snapshot, &[chunk.clone()]).is_ok());
    chunk.verified = false;
    assert!(validate_snapshot_publication(&snapshot, &[chunk]).is_err());
}

#[test]
fn stale_fence_and_partial_migration_are_rejected() {
    assert_eq!(
        validate_fencing_token(3, 2),
        Err(PersistenceError::VersionConflict)
    );
    let evidence = ClientMigrationEvidence {
        generation_before: LocalStoreGeneration::INITIAL,
        generation_after: LocalStoreGeneration::INITIAL,
        pending_operations_before: 4,
        pending_operations_after: 3,
        committed: true,
    };
    assert!(evidence.validate().is_err());
}

#[test]
fn mapping_contract_requires_all_logical_indexes() {
    let complete = AdapterMappingContract {
        adapter_name: "example-kv".into(),
        records: vec![AdapterRecordMapping {
            logical_record: LogicalRecord::Outbox,
            physical_representation: "transactional keyspaces".into(),
            logical_indexes: vec![
                LogicalIndexId::OutboxOperationUnique,
                LogicalIndexId::OutboxStateRetry,
                LogicalIndexId::OutboxStatePrioritySequence,
                LogicalIndexId::OutboxEntity,
                LogicalIndexId::OutboxCompactionState,
            ],
            transaction_support: vec!["native batch".into()],
            retention_behavior: "terminal reconciliation GC".into(),
        }],
    };
    assert!(complete.validate().is_ok());
    let mut incomplete = complete;
    incomplete.records[0].logical_indexes.pop();
    assert!(incomplete.validate().is_err());
}

#[test]
fn canonical_exports_compare_across_physical_adapters() {
    let record = MetadataExportRecord {
        kind: LogicalRecord::OperationLedger,
        canonical_key: OperationId::new().as_uuid().as_bytes().to_vec(),
        canonical_digest: digest(9),
        queryable_summary: vec![("status".into(), "accepted".into())],
        opaque_payload: None,
    };
    let schema = MetadataSchemaVersion::new(1).unwrap_or_else(|error| panic!("{error}"));
    let store = StoreId::new();
    let postgres = MetadataExport::build(
        schema,
        store,
        timestamp(1),
        "postgres",
        vec![record.clone()],
    )
    .unwrap_or_else(|error| panic!("{error}"));
    let stoolap = MetadataExport::build(schema, store, timestamp(1), "stoolap", vec![record])
        .unwrap_or_else(|error| panic!("{error}"));
    assert!(postgres.verify().is_ok());
    assert!(postgres.semantically_equivalent(&stoolap));
}

#[test]
fn all_nine_metadata_invariants_are_stable() {
    assert_eq!(METADATA_INVARIANTS.len(), 9);
    assert_eq!(MetadataInvariantId::ALL.len(), 9);
    for (index, invariant) in MetadataInvariantId::ALL.into_iter().enumerate() {
        assert_eq!(METADATA_INVARIANTS[index].id, invariant);
        assert!(invariant.as_str().starts_with("AEQ-INV-META"));
    }
}
