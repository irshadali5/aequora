use aequora::{
    client::{ClientConfig, ClientSyncEngine},
    clock::TestClock,
    conflict::RejectConflicts,
    executor::AuthContext,
    protocol::{OperationEnvelope, OperationKind, OperationMetadata, SessionMetadata},
    server::{ExchangeService, SyncServer},
    store::OutboxStore,
    testkit::{
        AllowAllExecutor, InMemoryAuthoritativeStore, InMemoryLocalStore, InProcessTransport,
    },
    types::{
        ActorId, DeviceId, EntityId, EntityRef, EntityType, HybridTimestamp, NodeId, OperationId,
        ProtocolVersion, SchemaVersion, SessionId, SyncScopeId, TenantId,
    },
};
use std::sync::Arc;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let tenant = TenantId::new();
    let actor = ActorId::new();
    let device = DeviceId::new();
    let scope = SyncScopeId::new();
    let session = SessionMetadata {
        session_id: SessionId::new(),
        device_id: device,
        actor_id: actor,
        tenant_id: tenant,
        scope_id: scope,
        partitions: Vec::new(),
    };
    let auth = AuthContext {
        actor_id: actor,
        tenant_id: tenant,
        device_id: device,
    };
    let authoritative = InMemoryAuthoritativeStore::default();
    let service: Arc<dyn ExchangeService> = Arc::new(SyncServer::new(
        Arc::new(authoritative),
        Arc::new(AllowAllExecutor),
        Arc::new(RejectConflicts),
        Arc::new(TestClock::new(NodeId::new(), 1_000)),
    ));
    let local = InMemoryLocalStore::default();
    local
        .append_operation(OperationEnvelope {
            protocol_version: ProtocolVersion::V1,
            operation_id: OperationId::new(),
            tenant_id: tenant,
            actor_id: actor,
            device_id: device,
            entity: EntityRef {
                entity_type: EntityType::new(1)?,
                entity_id: EntityId::new(),
            },
            base_version: None,
            created_at: HybridTimestamp {
                physical_ms: 1_000,
                logical: 0,
                node: NodeId::new(),
            },
            schema_version: SchemaVersion(1),
            operation_kind: OperationKind(1),
            payload: b"hello from an offline write".to_vec(),
            metadata: OperationMetadata::default(),
        })
        .await?;
    let engine = ClientSyncEngine::new(
        local,
        InProcessTransport::new(service, auth),
        ClientConfig::new(session),
    );
    let outcome = engine.run_once().await?;
    println!(
        "acknowledged={}, changes={}",
        outcome.acknowledged, outcome.changes
    );
    Ok(())
}
