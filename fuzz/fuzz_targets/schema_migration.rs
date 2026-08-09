#![no_main]

use aequora_executor::{
    AuthContext, AuthoritativeMutation, AuthorizedOperation, CurrentEntity, DomainOperation,
    ExecutionError, IncomingOperation, OperationExecutor, OperationHandler, OperationRegistry,
    PayloadMigrator, ScopeAuthorizer,
};
use aequora_protocol::{
    ChangeKind, OperationEnvelope, OperationKind, OperationMetadata, SessionMetadata,
};
use aequora_types::{
    ActorId, DeviceId, EntityId, EntityRef, EntityType, HybridTimestamp, NodeId, OperationId,
    ProtocolVersion, SchemaVersion, TenantId,
};
use async_trait::async_trait;
use libfuzzer_sys::fuzz_target;
use serde::Deserialize;

#[derive(Deserialize)]
struct Command {
    value: u32,
}

impl DomainOperation for Command {
    const KIND: u16 = 7;
    const CURRENT_SCHEMA: u16 = 3;
}

struct Handler;

#[async_trait]
impl OperationHandler<Command> for Handler {
    async fn authorize(
        &self,
        _auth: &AuthContext,
        operation: &Command,
        _envelope: &OperationEnvelope,
    ) -> Result<(), ExecutionError> {
        let _value = operation.value;
        Ok(())
    }

    async fn execute(
        &self,
        _auth: &AuthContext,
        operation: &Command,
        _envelope: &OperationEnvelope,
        _current: Option<&CurrentEntity>,
    ) -> Result<AuthoritativeMutation, ExecutionError> {
        Ok(AuthoritativeMutation {
            payload: operation.value.to_be_bytes().to_vec(),
            change_kind: ChangeKind::Upsert,
        })
    }
}

struct Migrator;

impl PayloadMigrator for Migrator {
    fn migrate(
        &self,
        _from: SchemaVersion,
        payload: &[u8],
    ) -> Result<Vec<u8>, ExecutionError> {
        Ok(payload.to_vec())
    }
}

struct Scope;

#[async_trait]
impl ScopeAuthorizer for Scope {
    async fn authorize_scope(
        &self,
        _auth: &AuthContext,
        _session: &SessionMetadata,
    ) -> Result<(), ExecutionError> {
        Ok(())
    }
}

fuzz_target!(|data: &[u8]| {
    if data.is_empty() {
        return;
    }
    let tenant = TenantId::new();
    let actor = ActorId::new();
    let device = DeviceId::new();
    let auth = AuthContext {
        actor_id: actor,
        tenant_id: tenant,
        device_id: device,
    };
    let operation = OperationEnvelope {
        protocol_version: ProtocolVersion::V1,
        operation_id: OperationId::new(),
        tenant_id: tenant,
        actor_id: actor,
        device_id: device,
        entity: EntityRef {
            entity_type: EntityType::new(1).unwrap_or_else(|_| unreachable!()),
            entity_id: EntityId::new(),
        },
        base_version: None,
        created_at: HybridTimestamp {
            physical_ms: 0,
            logical: 0,
            node: NodeId::new(),
        },
        schema_version: SchemaVersion(u16::from(data[0])),
        operation_kind: OperationKind(Command::KIND),
        payload: data[1..].to_vec(),
        metadata: OperationMetadata::default(),
    };
    let mut registry = OperationRegistry::new(Scope);
    if registry
        .register_with_migration::<Command, _, _>(1, Handler, Migrator)
        .is_err()
    {
        return;
    }
    let authenticated = IncomingOperation::new(&operation).authenticate(&auth).ok();
    if let Some(authenticated) = authenticated {
        if let Ok(runtime) = tokio::runtime::Builder::new_current_thread().build() {
            let _authorization: Result<AuthorizedOperation<'_>, _> =
                runtime.block_on(registry.authorize(&auth, authenticated));
        }
    }
});
