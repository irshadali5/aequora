use aequora_protocol::{ChangeKind, OperationEnvelope, OperationKind, OperationMetadata};
use aequora_store::CommitOperation;
use aequora_testkit::{
    InMemoryAuthoritativeStore, InMemoryLocalStore,
    contracts::{verify_authoritative_store, verify_local_store},
};
use aequora_types::{
    ActorId, DeviceId, EntityId, EntityRef, EntityType, EntityVersion, HybridTimestamp, NodeId,
    OperationId, ProtocolVersion, SchemaVersion, SyncScopeId, TenantId,
};

fn fixture() -> Result<(OperationEnvelope, SyncScopeId), Box<dyn std::error::Error>> {
    let scope = SyncScopeId::new();
    let operation = OperationEnvelope {
        protocol_version: ProtocolVersion::V1,
        operation_id: OperationId::new(),
        tenant_id: TenantId::new(),
        actor_id: ActorId::new(),
        device_id: DeviceId::new(),
        entity: EntityRef {
            entity_type: EntityType::new(77)?,
            entity_id: EntityId::new(),
        },
        base_version: None,
        created_at: HybridTimestamp {
            physical_ms: 100,
            logical: 0,
            node: NodeId::new(),
        },
        schema_version: SchemaVersion(1),
        operation_kind: OperationKind(1),
        payload: b"adapter contract fixture".to_vec(),
        metadata: OperationMetadata::default(),
    };
    Ok((operation, scope))
}

#[tokio::test]
async fn reference_local_store_passes_public_adapter_contract()
-> Result<(), Box<dyn std::error::Error>> {
    let (operation, scope) = fixture()?;
    let report = verify_local_store(
        &InMemoryLocalStore::default(),
        operation.clone(),
        scope,
        operation.created_at,
    )
    .await?;
    assert_eq!(report.operation_id, operation.operation_id);
    assert_eq!(report.cursor.scope, scope);
    Ok(())
}

#[tokio::test]
async fn reference_authority_passes_public_adapter_contract()
-> Result<(), Box<dyn std::error::Error>> {
    let (operation, scope) = fixture()?;
    let commit = CommitOperation {
        operation_id: operation.operation_id,
        actor_id: operation.actor_id,
        device_id: operation.device_id,
        operation_kind: operation.operation_kind.0,
        tenant_id: operation.tenant_id,
        scope_id: scope,
        entity: operation.entity,
        expected_version: None,
        next_version: EntityVersion::INITIAL,
        payload: operation.payload.clone(),
        change_kind: ChangeKind::Upsert,
        timestamp: operation.created_at,
        command_digest: *blake3::hash(&operation.payload).as_bytes(),
    };
    let report = verify_authoritative_store(&InMemoryAuthoritativeStore::default(), commit).await?;
    assert_eq!(report.acknowledgement.operation_id, operation.operation_id);
    Ok(())
}
