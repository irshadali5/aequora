#![cfg(all(
    feature = "axum",
    feature = "http-client",
    feature = "postgres",
    feature = "stoolap",
    feature = "testkit"
))]

use aequora::{
    client::{ClientConfig, ClientSyncEngine},
    clock::TestClock,
    conflict::RejectConflicts,
    executor::AuthContext,
    http_client::{HttpTransport, HttpTransportConfig, NoRequestHeaders},
    postgres::{PostgresPoolConfig, PostgresStore, SqlxPostgresBackend},
    protocol::{OperationEnvelope, OperationKind, OperationMetadata, SessionMetadata},
    server::{ExchangeService, SyncServer},
    stoolap::{StoolapDatabase, StoolapStore},
    store::{EntityReader, OutboxState, OutboxStateStore},
    testkit::AllowAllExecutor,
    types::{
        ActorId, DeviceId, EntityId, EntityRef, EntityType, HybridTimestamp, NodeId, OperationId,
        ProtocolVersion, SchemaVersion, SessionId, SyncScopeId, TenantId,
    },
};
use axum::Extension;
use reqwest::Url;
use std::sync::Arc;
use tempfile::tempdir;
use tokio::net::TcpListener;

#[tokio::test]
async fn stoolap_client_to_postgres_authority_over_http_is_real()
-> Result<(), Box<dyn std::error::Error>> {
    let Ok(database_url) = std::env::var("AEQUORA_TEST_POSTGRES_URL") else {
        return Ok(());
    };
    let backend = SqlxPostgresBackend::connect_with_migration_url(
        &database_url,
        &database_url,
        PostgresPoolConfig::new(2),
    )
    .await?;
    exercise_database_independent_stack(backend).await
}

#[tokio::test]
async fn stoolap_client_to_neon_authority_over_http_is_real()
-> Result<(), Box<dyn std::error::Error>> {
    let (Ok(pooled_url), Ok(direct_url)) = (
        std::env::var("AEQUORA_TEST_NEON_POOLED_URL"),
        std::env::var("AEQUORA_TEST_NEON_DIRECT_URL"),
    ) else {
        return Ok(());
    };
    let backend = SqlxPostgresBackend::connect_neon(&pooled_url, &direct_url, 2).await?;
    exercise_database_independent_stack(backend).await
}

async fn exercise_database_independent_stack(
    backend: SqlxPostgresBackend,
) -> Result<(), Box<dyn std::error::Error>> {
    let tenant = TenantId::new();
    let actor = ActorId::new();
    let device = DeviceId::new();
    let scope = SyncScopeId::new();
    let entity = EntityRef {
        entity_type: EntityType::new(91)?,
        entity_id: EntityId::new(),
    };
    let payload = b"database-neutral end-to-end operation".to_vec();
    let operation = OperationEnvelope {
        protocol_version: ProtocolVersion::V1,
        operation_id: OperationId::new(),
        tenant_id: tenant,
        actor_id: actor,
        device_id: device,
        entity,
        base_version: None,
        created_at: HybridTimestamp {
            physical_ms: 1_000,
            logical: 0,
            node: NodeId::new(),
        },
        schema_version: SchemaVersion(1),
        operation_kind: OperationKind(1),
        payload: payload.clone(),
        metadata: OperationMetadata::default(),
    };

    let (_directory, local_backend) = prepare_local_database(&operation, scope, &payload)?;

    let authority = Arc::new(PostgresStore::new(backend));
    authority.backend().health_check().await?;
    let auth = AuthContext {
        actor_id: actor,
        tenant_id: tenant,
        device_id: device,
    };
    let service: Arc<dyn ExchangeService> = Arc::new(SyncServer::new(
        authority.clone(),
        Arc::new(AllowAllExecutor),
        Arc::new(RejectConflicts),
        Arc::new(TestClock::new(NodeId::new(), 2_000)),
    ));
    let app = aequora::axum::router(service, 1024 * 1024).layer(Extension(auth));
    let listener = TcpListener::bind(("127.0.0.1", 0)).await?;
    let address = listener.local_addr()?;
    let server_task = tokio::spawn(async move { axum::serve(listener, app).await });

    let base_url = Url::parse(&format!("http://{address}/"))?;
    let transport = HttpTransport::new(
        reqwest::Client::new(),
        &base_url,
        NoRequestHeaders,
        HttpTransportConfig::default(),
    )?;
    let session = SessionMetadata {
        session_id: SessionId::new(),
        device_id: device,
        actor_id: actor,
        tenant_id: tenant,
        scope_id: scope,
        partitions: Vec::new(),
    };
    let engine = ClientSyncEngine::new(
        StoolapStore::new(local_backend),
        transport,
        ClientConfig::new(session),
    );
    let outcome = engine.run_once().await?;

    assert_eq!(outcome.acknowledged, 1);
    verify_reconciled(&engine, &authority, &operation, scope, &payload).await?;

    server_task.abort();
    let _server_result = server_task.await;
    Ok(())
}

fn prepare_local_database(
    operation: &OperationEnvelope,
    scope: SyncScopeId,
    payload: &[u8],
) -> Result<(tempfile::TempDir, StoolapDatabase), Box<dyn std::error::Error>> {
    let directory = tempdir()?;
    let dsn = format!("file://{}", directory.path().join("client").display());
    let backend = StoolapDatabase::open(&dsn)?;
    backend.transact_local_mutation(operation, |transaction| {
        transaction
            .execute(
                "INSERT INTO aequora_local_entities (scope_id, entity_type, entity_id, version, payload, tombstone, provisional) VALUES ($1, $2, $3, 1, $4, 0, 1)",
                (
                    scope.to_string(),
                    i64::from(operation.entity.entity_type.get()),
                    operation.entity.entity_id.to_string(),
                    hex::encode(payload),
                ),
            )
            .map_err(|error| aequora::store::StoreError::transient(error.to_string()))?;
        Ok(())
    })?;
    Ok((directory, backend))
}

async fn verify_reconciled(
    engine: &ClientSyncEngine<StoolapStore<StoolapDatabase>, HttpTransport>,
    authority: &PostgresStore<SqlxPostgresBackend>,
    operation: &OperationEnvelope,
    scope: SyncScopeId,
    payload: &[u8],
) -> Result<(), Box<dyn std::error::Error>> {
    assert_eq!(
        engine
            .store()
            .operation_state(operation.operation_id)
            .await?,
        Some(OutboxState::Acknowledged)
    );
    let authoritative = authority
        .read_entity(operation.tenant_id, operation.entity)
        .await?
        .ok_or("authoritative entity is missing")?;
    assert_eq!(authoritative.current.payload, payload);
    let provisional = engine
        .store()
        .backend()
        .database()
        .query_one::<bool, _>(
            "SELECT provisional FROM aequora_local_entities WHERE scope_id = $1 AND entity_type = $2 AND entity_id = $3",
            (
                scope.to_string(),
                i64::from(operation.entity.entity_type.get()),
                operation.entity.entity_id.to_string(),
            ),
        )?;
    assert!(!provisional);
    Ok(())
}
