use aequora_client::{ClientConfig, ClientSyncEngine, RetryConfig};
use aequora_clock::TestClock;
use aequora_conflict::RejectConflicts;
use aequora_executor::AuthContext;
use aequora_protocol::{OperationEnvelope, OperationKind, OperationMetadata, SessionMetadata};
use aequora_server::{ExchangeService, SyncServer};
use aequora_store::OutboxStore;
use aequora_testkit::{
    AllowAllExecutor, InMemoryAuthoritativeStore, InMemoryLocalStore, InProcessTransport,
};
use aequora_transport::{SyncTransport, TransportError};
use aequora_types::{
    ActorId, DeviceId, EntityId, EntityRef, EntityType, HybridTimestamp, NodeId, OperationId,
    ProtocolVersion, SchemaVersion, SessionId, SyncScopeId, TenantId,
};
use async_trait::async_trait;
use proptest::prelude::*;
use std::collections::VecDeque;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use std::time::Duration;

#[derive(Clone)]
struct DroppableTransport {
    inner: InProcessTransport,
    drop_next: Arc<AtomicBool>,
}

impl DroppableTransport {
    fn new(inner: InProcessTransport) -> Self {
        Self {
            inner,
            drop_next: Arc::new(AtomicBool::new(false)),
        }
    }

    fn drop_next_response(&self) {
        self.drop_next.store(true, Ordering::Relaxed);
    }
}

#[async_trait]
impl SyncTransport for DroppableTransport {
    async fn exchange(
        &self,
        request: aequora_protocol::SyncRequest,
    ) -> Result<aequora_protocol::SyncResponse, TransportError> {
        let response = self.inner.exchange(request).await?;
        if self.drop_next.swap(false, Ordering::Relaxed) {
            Err(TransportError::transient(
                "model dropped a response after authoritative commit",
            ))
        } else {
            Ok(response)
        }
    }

    async fn bootstrap(
        &self,
        request: aequora_protocol::BootstrapRequest,
    ) -> Result<aequora_protocol::BootstrapResponse, TransportError> {
        self.inner.bootstrap(request).await
    }
}

#[derive(Clone)]
struct ModelMutation {
    base: Option<u64>,
    payload: Vec<u8>,
    tombstone: bool,
}

#[derive(Default)]
struct ReferenceClient {
    seen_version: Option<u64>,
    queue: VecDeque<ModelMutation>,
}

#[derive(Default)]
struct ReferenceAuthority {
    version: Option<u64>,
    payload: Vec<u8>,
    tombstone: bool,
}

struct Device {
    local: InMemoryLocalStore,
    transport: DroppableTransport,
    engine: ClientSyncEngine<InMemoryLocalStore, DroppableTransport>,
    actor: ActorId,
    id: DeviceId,
}

struct ModelFixture {
    tenant: TenantId,
    entity: EntityRef,
    node: NodeId,
    authoritative: InMemoryAuthoritativeStore,
    first: Device,
    second: Device,
}

impl ModelFixture {
    fn new() -> Self {
        let tenant = TenantId::new();
        let scope = SyncScopeId::new();
        let entity = EntityRef {
            entity_type: EntityType::new(1).unwrap_or_else(|error| panic!("{error}")),
            entity_id: EntityId::new(),
        };
        let node = NodeId::new();
        let authoritative = InMemoryAuthoritativeStore::default();
        let server: Arc<dyn ExchangeService> = Arc::new(SyncServer::new(
            Arc::new(authoritative.clone()),
            Arc::new(AllowAllExecutor),
            Arc::new(RejectConflicts),
            Arc::new(TestClock::new(node, 1_000)),
        ));
        let first = device(tenant, scope, server.clone());
        let second = device(tenant, scope, server);
        Self {
            tenant,
            entity,
            node,
            authoritative,
            first,
            second,
        }
    }

    async fn append(
        &self,
        device: &Device,
        reference: &mut ReferenceClient,
        payload: Vec<u8>,
        tombstone: bool,
    ) -> Result<(), String> {
        let operation = OperationEnvelope {
            protocol_version: ProtocolVersion::V1,
            operation_id: OperationId::new(),
            tenant_id: self.tenant,
            actor_id: device.actor,
            device_id: device.id,
            entity: self.entity,
            base_version: device
                .local
                .entity(self.entity)
                .map(|entity| entity.version),
            created_at: HybridTimestamp {
                physical_ms: 1_000,
                logical: 0,
                node: self.node,
            },
            schema_version: SchemaVersion(1),
            operation_kind: OperationKind(if tombstone { 2 } else { 1 }),
            payload: payload.clone(),
            metadata: OperationMetadata::default(),
        };
        device
            .local
            .append_operation(operation)
            .await
            .map_err(|error| error.to_string())?;
        reference.queue.push_back(ModelMutation {
            base: reference.seen_version,
            payload,
            tombstone,
        });
        Ok(())
    }
}

fn device(tenant: TenantId, scope: SyncScopeId, server: Arc<dyn ExchangeService>) -> Device {
    let actor = ActorId::new();
    let device_id = DeviceId::new();
    let auth = AuthContext {
        actor_id: actor,
        tenant_id: tenant,
        device_id,
    };
    let session = SessionMetadata {
        session_id: SessionId::new(),
        device_id,
        actor_id: actor,
        tenant_id: tenant,
        scope_id: scope,
        partitions: Vec::new(),
    };
    let local = InMemoryLocalStore::default();
    let transport = DroppableTransport::new(InProcessTransport::new(server, auth));
    let mut config = ClientConfig::new(session);
    config.retry = RetryConfig {
        max_attempts: 2,
        initial_delay: Duration::ZERO,
        max_delay: Duration::ZERO,
        multiplier: 1,
        jitter_percent: 0,
    };
    let engine = ClientSyncEngine::new(local.clone(), transport.clone(), config);
    Device {
        local,
        transport,
        engine,
        actor,
        id: device_id,
    }
}

async fn synchronize(
    device: &Device,
    client: &mut ReferenceClient,
    authority: &mut ReferenceAuthority,
    drop_response: bool,
) -> Result<(), String> {
    if drop_response {
        device.transport.drop_next_response();
    }
    device
        .engine
        .sync()
        .await
        .map_err(|error| error.to_string())?;
    while let Some(operation) = client.queue.pop_front() {
        if operation.base == authority.version {
            authority.version = Some(authority.version.map_or(1, |version| version + 1));
            authority.payload = operation.payload;
            authority.tombstone = operation.tombstone;
        }
    }
    client.seen_version = authority.version;
    Ok(())
}

async fn run_model(actions: Vec<u8>) -> Result<(), String> {
    let fixture = ModelFixture::new();
    let mut authority = ReferenceAuthority::default();
    let mut first = ReferenceClient::default();
    let mut second = ReferenceClient::default();
    for (index, action) in actions.into_iter().enumerate() {
        let payload = vec![action, u8::try_from(index % 256).unwrap_or(0)];
        match action % 9 {
            0 => {
                fixture
                    .append(&fixture.first, &mut first, payload, false)
                    .await?;
            }
            1 => {
                fixture
                    .append(&fixture.second, &mut second, payload, false)
                    .await?;
            }
            2 => {
                fixture
                    .append(&fixture.first, &mut first, payload, true)
                    .await?;
            }
            3 => {
                fixture
                    .append(&fixture.second, &mut second, payload, true)
                    .await?;
            }
            4 => synchronize(&fixture.first, &mut first, &mut authority, false).await?,
            5 => synchronize(&fixture.second, &mut second, &mut authority, false).await?,
            6 => synchronize(&fixture.first, &mut first, &mut authority, true).await?,
            7 => synchronize(&fixture.second, &mut second, &mut authority, true).await?,
            _ => {}
        }
    }
    synchronize(&fixture.first, &mut first, &mut authority, false).await?;
    synchronize(&fixture.second, &mut second, &mut authority, false).await?;
    synchronize(&fixture.first, &mut first, &mut authority, false).await?;

    let stored = fixture.authoritative.entity(fixture.tenant, fixture.entity);
    match (stored, authority.version) {
        (None, None) => {}
        (Some(stored), Some(version)) => {
            if stored.current.version.get() != version
                || stored.current.payload != authority.payload
                || stored.current.tombstone != authority.tombstone
            {
                return Err("authoritative store diverged from reference model".to_owned());
            }
            for device in [&fixture.first, &fixture.second] {
                let local = device
                    .local
                    .entity(fixture.entity)
                    .ok_or_else(|| "converged client is missing the entity".to_owned())?;
                if local.version != stored.current.version
                    || local.payload != stored.current.payload
                    || local.tombstone != stored.current.tombstone
                {
                    return Err("client did not converge to authoritative state".to_owned());
                }
            }
        }
        _ => return Err("reference and authoritative existence diverged".to_owned()),
    }
    Ok(())
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]

    #[test]
    fn generated_two_client_history_matches_the_reference_model(
        actions in prop::collection::vec(0_u8..18, 1..64),
    ) {
        let runtime = tokio::runtime::Runtime::new()
            .unwrap_or_else(|error| panic!("{error}"));
        let result = runtime.block_on(run_model(actions));
        prop_assert_eq!(result, Ok(()));
    }
}
