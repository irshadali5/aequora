use aequora::{
    client::{ClientConfig, ClientSyncEngine},
    clock::TestClock,
    conflict::RejectConflicts,
    executor::{
        AuthContext, AuthoritativeMutation, CurrentEntity, DomainOperation, ExecutionError,
        OperationHandler, OperationRegistry, ScopeAuthorizer,
    },
    protocol::{ChangeKind, OperationEnvelope, OperationKind, OperationMetadata, SessionMetadata},
    server::{ExchangeService, SyncServer},
    stoolap::{StoolapDatabase, StoolapStore},
    testkit::{InMemoryAuthoritativeStore, InProcessTransport},
    types::{
        ActorId, DeviceId, EntityId, EntityRef, EntityType, HybridTimestamp, NodeId, OperationId,
        ProtocolVersion, SchemaVersion, SessionId, SyncScopeId, TenantId,
    },
};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

#[derive(Clone, Debug, Deserialize, Serialize)]
struct MarkAttendance {
    student_id: EntityId,
    teacher_id: ActorId,
    school_day: i64,
    present: bool,
}

impl DomainOperation for MarkAttendance {
    const KIND: u16 = 100;
    const CURRENT_SCHEMA: u16 = 1;
}

struct SchoolScopeAuthorizer;

#[async_trait]
impl ScopeAuthorizer for SchoolScopeAuthorizer {
    async fn authorize_scope(
        &self,
        auth: &AuthContext,
        session: &SessionMetadata,
    ) -> Result<(), ExecutionError> {
        if auth.tenant_id == session.tenant_id {
            Ok(())
        } else {
            Err(ExecutionError::unauthorized("school scope is not assigned"))
        }
    }
}

struct AttendanceCommandHandler;

#[async_trait]
impl OperationHandler<MarkAttendance> for AttendanceCommandHandler {
    async fn authorize(
        &self,
        auth: &AuthContext,
        command: &MarkAttendance,
        _envelope: &OperationEnvelope,
    ) -> Result<(), ExecutionError> {
        if command.teacher_id == auth.actor_id {
            Ok(())
        } else {
            Err(ExecutionError::unauthorized(
                "teacher cannot submit attendance for another actor",
            ))
        }
    }

    async fn execute(
        &self,
        _auth: &AuthContext,
        command: &MarkAttendance,
        _envelope: &OperationEnvelope,
        _current: Option<&CurrentEntity>,
    ) -> Result<AuthoritativeMutation, ExecutionError> {
        if command.school_day <= 0 {
            return Err(ExecutionError::business_rule("school day must be positive"));
        }
        let payload = postcard::to_stdvec(command)
            .map_err(|_| ExecutionError::invalid_operation("attendance encoding failed"))?;
        Ok(AuthoritativeMutation {
            payload,
            change_kind: ChangeKind::Upsert,
        })
    }
}

fn read_attendance(
    database: &StoolapDatabase,
    scope: SyncScopeId,
    entity: EntityRef,
) -> Result<(MarkAttendance, bool), Box<dyn std::error::Error>> {
    let mut rows = database.database().query(
        "SELECT payload, provisional FROM aequora_local_entities WHERE scope_id = $1 AND entity_type = $2 AND entity_id = $3",
        (
            scope.to_string(),
            i64::from(entity.entity_type.get()),
            entity.entity_id.to_string(),
        ),
    )?;
    let row = rows.next().ok_or("attendance row is missing")??;
    let encoded: String = row.get(0)?;
    let provisional: bool = row.get(1)?;
    let bytes = hex::decode(encoded)?;
    Ok((postcard::from_bytes(&bytes)?, provisional))
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let tenant = TenantId::new();
    let teacher = ActorId::new();
    let device = DeviceId::new();
    let scope = SyncScopeId::new();
    let student = EntityId::new();
    let entity = EntityRef {
        entity_type: EntityType::new(20)?,
        entity_id: student,
    };
    let command = MarkAttendance {
        student_id: student,
        teacher_id: teacher,
        school_day: 20_260_809,
        present: true,
    };
    let payload = postcard::to_stdvec(&command)?;
    let operation = OperationEnvelope {
        protocol_version: ProtocolVersion::V1,
        operation_id: OperationId::new(),
        tenant_id: tenant,
        actor_id: teacher,
        device_id: device,
        entity,
        base_version: None,
        created_at: HybridTimestamp {
            physical_ms: 1_000,
            logical: 0,
            node: NodeId::new(),
        },
        schema_version: SchemaVersion(1),
        operation_kind: OperationKind(MarkAttendance::KIND),
        payload: payload.clone(),
        metadata: OperationMetadata::default(),
    };

    let local_backend = StoolapDatabase::open_in_memory()?;
    local_backend.transact_local_mutation(&operation, |transaction| {
        transaction
            .execute(
                "INSERT INTO aequora_local_entities (scope_id, entity_type, entity_id, version, payload, tombstone, provisional) VALUES ($1, $2, $3, 1, $4, 0, 1)",
                (
                    scope.to_string(),
                    i64::from(entity.entity_type.get()),
                    entity.entity_id.to_string(),
                    hex::encode(&payload),
                ),
            )
            .map_err(|error| aequora::store::StoreError::transient(error.to_string()))?;
        Ok(())
    })?;
    let (offline_value, provisional) = read_attendance(&local_backend, scope, entity)?;
    assert!(offline_value.present && provisional);

    let mut registry = OperationRegistry::new(SchoolScopeAuthorizer);
    registry.register::<MarkAttendance, _>(AttendanceCommandHandler)?;
    let authoritative = InMemoryAuthoritativeStore::default();
    let service: Arc<dyn ExchangeService> = Arc::new(SyncServer::new(
        Arc::new(authoritative.clone()),
        Arc::new(registry),
        Arc::new(RejectConflicts),
        Arc::new(TestClock::new(NodeId::new(), 2_000)),
    ));
    let auth = AuthContext {
        actor_id: teacher,
        tenant_id: tenant,
        device_id: device,
    };
    let session = SessionMetadata {
        session_id: SessionId::new(),
        device_id: device,
        actor_id: teacher,
        tenant_id: tenant,
        scope_id: scope,
        partitions: Vec::new(),
    };
    let local = StoolapStore::new(local_backend);
    let engine = ClientSyncEngine::new(
        local,
        InProcessTransport::new(service, auth),
        ClientConfig::new(session),
    );
    let outcome = engine.run_once().await?;
    assert_eq!(outcome.acknowledged, 1);
    assert_eq!(authoritative.applied_operation_count(), 1);
    let (accepted_value, provisional) = read_attendance(engine.store().backend(), scope, entity)?;
    assert!(accepted_value.present && !provisional);
    println!("offline attendance accepted and reconciled at sequence 1");
    Ok(())
}
