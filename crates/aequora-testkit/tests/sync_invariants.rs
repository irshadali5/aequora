use aequora_client::{
    ClientConfig, ClientError, ClientSyncEngine, ClientSyncEngineBuilder, RetryConfig,
    SyncCoordinator, SyncCoordinatorConfig, SyncStatus, SyncTrigger,
};
use aequora_clock::TestClock;
use aequora_compute::{ComputeConfig, ComputePool};
use aequora_conflict::{
    ConflictPolicyRegistry, FieldSet, FieldSetMerger, FieldValue, RejectConflicts, TypedOperation,
};
use aequora_executor::{
    AuthContext, AuthenticatedOperation, AuthoritativeMutation, AuthorizedOperation, CurrentEntity,
    ExecutableOperation, ExecutionError, OperationExecutor,
};
use aequora_observability::AtomicMetrics;
use aequora_protocol::{
    BootstrapRequest, Capability, ChangeKind, ClientLimits, ConflictPolicy, OperationEnvelope,
    OperationKind, OperationMetadata, PushHint, PushHintReason, ResyncReason, SessionMetadata,
    SnapshotLimits, SyncDirective, SyncRequest,
};
use aequora_server::{
    ExchangeService, ServerCommandOutcome, ServerConfig, SyncServer, SyncServerBuilder,
};
use aequora_store::{
    AuditLog, AuditOffset, ConflictInbox, ConflictResolution, CursorStore, JournalCompactor,
    OutboxState, OutboxStateStore, OutboxStore, ReconciliationStore,
};
use aequora_testkit::FaultInjectingTransport;
use aequora_testkit::ResponseDroppingTransport;
use aequora_testkit::{
    CommitFailPoint, InMemoryAuthoritativeStore, InMemoryLocalStore, InProcessTransport,
};
use aequora_transport::{SyncTransport, TransportError};
use aequora_types::{
    ActorId, DeviceId, EntityId, EntityRef, EntityType, HybridTimestamp, NodeId, OperationId,
    ProtocolVersion, RequestId, SchemaVersion, Sequence, SessionId, SyncScopeId, TenantId,
};
use async_trait::async_trait;
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};
use std::time::Duration;

struct CopyPayloadExecutor;

#[async_trait]
impl OperationExecutor for CopyPayloadExecutor {
    async fn authorize_scope(
        &self,
        _auth: &AuthContext,
        _session: &SessionMetadata,
    ) -> Result<(), ExecutionError> {
        Ok(())
    }

    async fn authorize<'a>(
        &self,
        _auth: &AuthContext,
        operation: AuthenticatedOperation<'a>,
    ) -> Result<AuthorizedOperation<'a>, ExecutionError> {
        Ok(operation.authorize())
    }

    async fn execute(
        &self,
        _auth: &AuthContext,
        operation: ExecutableOperation<'_>,
        _current: Option<&CurrentEntity>,
    ) -> Result<AuthoritativeMutation, ExecutionError> {
        let operation = operation.envelope();
        Ok(AuthoritativeMutation {
            payload: operation.payload.clone(),
            change_kind: if operation.operation_kind == OperationKind(2) {
                ChangeKind::Tombstone
            } else {
                ChangeKind::Upsert
            },
        })
    }
}

struct DenyScopeExecutor;

struct HintOnlyTransport(PushHint);

enum ResponseCorruption {
    MissingTerminalResult,
    CursorLeap,
}

struct CorruptingTransport {
    inner: InProcessTransport,
    corruption: ResponseCorruption,
}

struct CountingTransport {
    inner: InProcessTransport,
    exchanges: Arc<AtomicUsize>,
}

#[async_trait]
impl SyncTransport for CountingTransport {
    async fn exchange(
        &self,
        request: SyncRequest,
    ) -> Result<aequora_protocol::SyncResponse, TransportError> {
        self.exchanges.fetch_add(1, Ordering::Relaxed);
        self.inner.exchange(request).await
    }

    async fn bootstrap(
        &self,
        request: BootstrapRequest,
    ) -> Result<aequora_protocol::BootstrapResponse, TransportError> {
        self.inner.bootstrap(request).await
    }
}

#[async_trait]
impl SyncTransport for CorruptingTransport {
    async fn exchange(
        &self,
        request: SyncRequest,
    ) -> Result<aequora_protocol::SyncResponse, TransportError> {
        let mut response = self.inner.exchange(request).await?;
        match self.corruption {
            ResponseCorruption::MissingTerminalResult => response.acknowledged.clear(),
            ResponseCorruption::CursorLeap => {
                response.next_cursor.sequence =
                    Sequence(response.next_cursor.sequence.0.saturating_add(1));
            }
        }
        Ok(response)
    }
}

#[async_trait]
impl SyncTransport for HintOnlyTransport {
    async fn exchange(
        &self,
        _request: SyncRequest,
    ) -> Result<aequora_protocol::SyncResponse, TransportError> {
        Err(TransportError::permanent(
            "exchange is not used by this test",
        ))
    }

    async fn next_push_hint(&self) -> Result<PushHint, TransportError> {
        Ok(self.0)
    }
}

#[async_trait]
impl OperationExecutor for DenyScopeExecutor {
    async fn authorize_scope(
        &self,
        _auth: &AuthContext,
        _session: &SessionMetadata,
    ) -> Result<(), ExecutionError> {
        Err(ExecutionError::unauthorized("scope denied"))
    }

    async fn authorize<'a>(
        &self,
        _auth: &AuthContext,
        operation: AuthenticatedOperation<'a>,
    ) -> Result<AuthorizedOperation<'a>, ExecutionError> {
        Ok(operation.authorize())
    }

    async fn execute(
        &self,
        _auth: &AuthContext,
        _operation: ExecutableOperation<'_>,
        _current: Option<&CurrentEntity>,
    ) -> Result<AuthoritativeMutation, ExecutionError> {
        Err(ExecutionError::business_rule(
            "execution should not be reached",
        ))
    }
}

struct Fixture {
    tenant: TenantId,
    actor: ActorId,
    device: DeviceId,
    scope: SyncScopeId,
    entity: EntityRef,
    auth: AuthContext,
    session: SessionMetadata,
}

impl Fixture {
    fn new() -> Self {
        let tenant = TenantId::new();
        let actor = ActorId::new();
        let device = DeviceId::new();
        let scope = SyncScopeId::new();
        Self {
            tenant,
            actor,
            device,
            scope,
            entity: EntityRef {
                entity_type: EntityType::new(1).unwrap_or_else(|error| panic!("{error}")),
                entity_id: EntityId::new(),
            },
            auth: AuthContext {
                actor_id: actor,
                tenant_id: tenant,
                device_id: device,
            },
            session: SessionMetadata {
                session_id: SessionId::new(),
                device_id: device,
                actor_id: actor,
                tenant_id: tenant,
                scope_id: scope,
                partitions: Vec::new(),
            },
        }
    }

    fn operation(&self) -> OperationEnvelope {
        OperationEnvelope {
            protocol_version: ProtocolVersion::V1,
            operation_id: OperationId::new(),
            tenant_id: self.tenant,
            actor_id: self.actor,
            device_id: self.device,
            entity: self.entity,
            base_version: None,
            created_at: HybridTimestamp {
                physical_ms: 1_000,
                logical: 0,
                node: NodeId::new(),
            },
            schema_version: SchemaVersion(1),
            operation_kind: OperationKind(1),
            payload: b"authoritative student".to_vec(),
            metadata: OperationMetadata::default(),
        }
    }

    fn request(&self, operation: OperationEnvelope) -> SyncRequest {
        SyncRequest {
            protocol: ProtocolVersion::V1,
            request_id: RequestId::new(),
            session: self.session.clone(),
            cursor: None,
            operations: vec![operation],
            limits: ClientLimits::default(),
            capabilities: vec![Capability::PostcardV1],
        }
    }
}

fn server(store: &InMemoryAuthoritativeStore) -> Arc<dyn ExchangeService> {
    Arc::new(
        SyncServerBuilder::new()
            .store(Arc::new(store.clone()))
            .executor(Arc::new(CopyPayloadExecutor))
            .conflicts(Arc::new(RejectConflicts))
            .clock(Arc::new(TestClock::new(NodeId::new(), 10_000)))
            .build(),
    )
}

#[tokio::test]
async fn server_originated_commands_cannot_bypass_the_sync_journal() {
    let fixture = Fixture::new();
    let authoritative = InMemoryAuthoritativeStore::default();
    let service = SyncServer::new(
        Arc::new(authoritative.clone()),
        Arc::new(CopyPayloadExecutor),
        Arc::new(RejectConflicts),
        Arc::new(TestClock::new(NodeId::new(), 10_000)),
    );
    let operation = fixture.operation();
    let outcome = service
        .execute_server_command(fixture.auth, fixture.scope, operation.clone())
        .await
        .unwrap_or_else(|error| panic!("{error}"));

    assert!(matches!(outcome, ServerCommandOutcome::Acknowledged(_)));
    assert_eq!(authoritative.applied_operation_count(), 1);
    assert_eq!(authoritative.journal_len(), 1);
    assert_eq!(authoritative.audit_len(), 1);
    assert_eq!(
        authoritative
            .entity(fixture.tenant, fixture.entity)
            .map(|snapshot| snapshot.current.payload),
        Some(operation.payload)
    );
}

#[tokio::test]
async fn client_push_pull_reconciles_atomically() {
    let fixture = Fixture::new();
    let authoritative = InMemoryAuthoritativeStore::default();
    let local = InMemoryLocalStore::default();
    let operation = fixture.operation();
    let operation_id = operation.operation_id;
    local
        .append_operation(operation)
        .await
        .unwrap_or_else(|error| panic!("{error}"));
    let transport = InProcessTransport::new(server(&authoritative), fixture.auth);
    let engine = ClientSyncEngineBuilder::new()
        .store(local.clone())
        .transport(transport)
        .config(ClientConfig::new(fixture.session))
        .build()
        .unwrap_or_else(|error| panic!("{error}"));

    let outcome = engine
        .run_once()
        .await
        .unwrap_or_else(|error| panic!("{error}"));

    assert_eq!(outcome.acknowledged, 1);
    assert_eq!(outcome.changes, 1);
    assert_eq!(local.pending_count(), 0);
    assert_eq!(
        local
            .operation_state(operation_id)
            .await
            .unwrap_or_else(|error| panic!("{error}")),
        Some(OutboxState::Acknowledged)
    );
    assert_eq!(authoritative.applied_operation_count(), 1);
    let local_entity = local
        .entity(fixture.entity)
        .unwrap_or_else(|| panic!("missing entity"));
    assert_eq!(local_entity.payload, b"authoritative student");
    let cursor = local
        .load_cursor(fixture.scope)
        .await
        .unwrap_or_else(|error| panic!("{error}"))
        .unwrap_or_else(|| panic!("missing cursor"));
    assert_eq!(cursor.sequence.0, 1);
}

#[tokio::test]
async fn client_rejects_incomplete_terminal_results_and_cursor_leaps() {
    for corruption in [
        ResponseCorruption::MissingTerminalResult,
        ResponseCorruption::CursorLeap,
    ] {
        let fixture = Fixture::new();
        let authoritative = InMemoryAuthoritativeStore::default();
        let local = InMemoryLocalStore::default();
        let operation = fixture.operation();
        let operation_id = operation.operation_id;
        local
            .append_operation(operation)
            .await
            .unwrap_or_else(|error| panic!("{error}"));
        let engine = ClientSyncEngine::new(
            local.clone(),
            CorruptingTransport {
                inner: InProcessTransport::new(server(&authoritative), fixture.auth),
                corruption,
            },
            ClientConfig::new(fixture.session),
        );

        let Err(error) = engine.run_once().await else {
            panic!("corrupt response must not reconcile")
        };
        assert!(matches!(
            error,
            ClientError::OperationResults | ClientError::ChangeSequence
        ));
        assert_eq!(
            local
                .operation_state(operation_id)
                .await
                .unwrap_or_else(|error| panic!("{error}")),
            Some(OutboxState::Retry)
        );
        assert!(
            local
                .load_cursor(fixture.scope)
                .await
                .unwrap_or_else(|error| panic!("{error}"))
                .is_none()
        );
    }
}

#[tokio::test]
async fn outgoing_batches_stop_at_the_framed_byte_limit() {
    let fixture = Fixture::new();
    let authoritative = InMemoryAuthoritativeStore::default();
    let local = InMemoryLocalStore::default();
    let first = fixture.operation();
    let mut second = fixture.operation();
    second.entity.entity_id = EntityId::new();
    local
        .append_operation(first.clone())
        .await
        .unwrap_or_else(|error| panic!("{error}"));
    local
        .append_operation(second)
        .await
        .unwrap_or_else(|error| panic!("{error}"));
    let mut config = ClientConfig::new(fixture.session);
    config.push_batch_size = 2;
    let one_operation_request = SyncRequest {
        protocol: config.protocol,
        request_id: RequestId::new(),
        session: config.session.clone(),
        cursor: None,
        operations: vec![first],
        limits: config.limits,
        capabilities: config.capabilities.clone(),
    };
    let one_operation_bytes = aequora_codec::encode(
        ProtocolVersion::V1,
        aequora_codec::MessageKind::SyncRequest,
        &one_operation_request,
    )
    .unwrap_or_else(|error| panic!("{error}"))
    .len();
    config.push_batch_bytes = one_operation_bytes;
    let engine = ClientSyncEngine::new(
        local.clone(),
        InProcessTransport::new(server(&authoritative), fixture.auth),
        config,
    );

    let outcome = engine
        .run_once()
        .await
        .unwrap_or_else(|error| panic!("{error}"));
    assert_eq!(outcome.acknowledged, 1);
    assert_eq!(authoritative.applied_operation_count(), 1);
    assert_eq!(local.pending_count(), 1);
}

#[tokio::test]
async fn server_pages_by_the_exact_advertised_response_frame_limit() {
    let fixture = Fixture::new();
    let authoritative = InMemoryAuthoritativeStore::default();
    let service = server(&authoritative);
    for _ in 0..2 {
        let mut operation = fixture.operation();
        operation.entity.entity_id = EntityId::new();
        service
            .exchange(fixture.auth, fixture.request(operation))
            .await
            .unwrap_or_else(|error| panic!("{error}"));
    }
    let mut request = fixture.request(fixture.operation());
    request.operations.clear();
    let full = service
        .exchange(fixture.auth, request.clone())
        .await
        .unwrap_or_else(|error| panic!("{error}"));
    assert_eq!(full.changes.len(), 2);
    let mut one = full;
    one.changes.truncate(1);
    one.next_cursor.sequence = one.changes[0].sequence;
    one.has_more = true;
    request.limits.max_response_bytes = u32::try_from(
        aequora_codec::encode(
            ProtocolVersion::V1,
            aequora_codec::MessageKind::SyncResponse,
            &one,
        )
        .unwrap_or_else(|error| panic!("{error}"))
        .len(),
    )
    .unwrap_or(u32::MAX);

    let bounded = service
        .exchange(fixture.auth, request)
        .await
        .unwrap_or_else(|error| panic!("{error}"));
    assert_eq!(bounded.changes.len(), 1);
    assert!(bounded.has_more);
    assert_eq!(bounded.next_cursor.sequence, bounded.changes[0].sequence);
}

#[tokio::test]
async fn protocol_windows_return_a_typed_upgrade_instruction() {
    let fixture = Fixture::new();
    let authoritative = InMemoryAuthoritativeStore::default();
    let mut server_config = ServerConfig::default();
    server_config.limits.minimum_protocol = ProtocolVersion(2);
    server_config.limits.current_protocol = ProtocolVersion(3);
    let service: Arc<dyn ExchangeService> = Arc::new(
        SyncServerBuilder::new()
            .store(Arc::new(authoritative.clone()))
            .executor(Arc::new(CopyPayloadExecutor))
            .conflicts(Arc::new(RejectConflicts))
            .clock(Arc::new(TestClock::new(NodeId::new(), 10_000)))
            .config(server_config)
            .build(),
    );
    let local = InMemoryLocalStore::default();
    let operation = fixture.operation();
    local
        .append_operation(operation)
        .await
        .unwrap_or_else(|error| panic!("{error}"));
    let engine = ClientSyncEngine::new(
        local,
        InProcessTransport::new(service, fixture.auth),
        ClientConfig::new(fixture.session),
    );

    assert!(matches!(
        engine.run_once().await,
        Err(ClientError::UpgradeRequired {
            minimum: ProtocolVersion(2),
            current: ProtocolVersion(3),
        })
    ));
    assert_eq!(authoritative.applied_operation_count(), 0);
}

#[tokio::test]
async fn an_expired_cursor_automatically_bootstraps_before_incremental_sync() {
    let fixture = Fixture::new();
    let authoritative = InMemoryAuthoritativeStore::default();
    server(&authoritative)
        .exchange(fixture.auth, fixture.request(fixture.operation()))
        .await
        .unwrap_or_else(|error| panic!("{error}"));
    authoritative
        .compact_journal(fixture.tenant, fixture.scope, Sequence(1))
        .await
        .unwrap_or_else(|error| panic!("{error}"));
    let direct = server(&authoritative)
        .exchange(
            fixture.auth,
            SyncRequest {
                protocol: ProtocolVersion::V1,
                request_id: RequestId::new(),
                session: fixture.session.clone(),
                cursor: None,
                operations: Vec::new(),
                limits: ClientLimits::default(),
                capabilities: Vec::new(),
            },
        )
        .await
        .unwrap_or_else(|error| panic!("{error}"));
    assert_eq!(
        direct.directive,
        SyncDirective::ResyncRequired {
            reason: ResyncReason::CursorExpired,
        }
    );

    let local = InMemoryLocalStore::default();
    let engine = ClientSyncEngine::new(
        local.clone(),
        InProcessTransport::new(server(&authoritative), fixture.auth),
        ClientConfig::new(fixture.session),
    );
    engine
        .sync()
        .await
        .unwrap_or_else(|error| panic!("{error}"));
    assert!(local.entity(fixture.entity).is_some());
    assert_eq!(
        local
            .load_cursor(fixture.scope)
            .await
            .unwrap_or_else(|error| panic!("{error}"))
            .map(|cursor| cursor.sequence),
        Some(Sequence(1))
    );
}

#[tokio::test]
async fn a_new_client_bootstraps_instead_of_replaying_the_journal() {
    let fixture = Fixture::new();
    let authoritative = InMemoryAuthoritativeStore::default();
    server(&authoritative)
        .exchange(fixture.auth, fixture.request(fixture.operation()))
        .await
        .unwrap_or_else(|error| panic!("{error}"));
    let local = InMemoryLocalStore::default();
    let engine = ClientSyncEngine::new(
        local.clone(),
        InProcessTransport::new(server(&authoritative), fixture.auth),
        ClientConfig::new(fixture.session),
    );

    let summary = engine
        .sync()
        .await
        .unwrap_or_else(|error| panic!("{error}"));
    assert_eq!(summary.changes, 0, "state must arrive in the snapshot");
    assert_eq!(local.entity_count(fixture.scope), 1);
    assert_eq!(
        local
            .load_cursor(fixture.scope)
            .await
            .unwrap_or_else(|error| panic!("{error}"))
            .map(|cursor| cursor.sequence),
        Some(Sequence(1))
    );
}

#[tokio::test]
async fn manual_conflicts_remain_in_a_durable_ui_independent_inbox() {
    let fixture = Fixture::new();
    let authoritative = InMemoryAuthoritativeStore::default();
    server(&authoritative)
        .exchange(fixture.auth, fixture.request(fixture.operation()))
        .await
        .unwrap_or_else(|error| panic!("{error}"));
    let local = InMemoryLocalStore::default();
    let stale = fixture.operation();
    let operation_id = stale.operation_id;
    local
        .append_operation(stale)
        .await
        .unwrap_or_else(|error| panic!("{error}"));
    let engine = ClientSyncEngine::new(
        local.clone(),
        InProcessTransport::new(server(&authoritative), fixture.auth),
        ClientConfig::new(fixture.session),
    );

    let outcome = engine
        .run_once()
        .await
        .unwrap_or_else(|error| panic!("{error}"));
    assert_eq!(outcome.conflicts, 1);
    assert_eq!(
        local
            .operation_state(operation_id)
            .await
            .unwrap_or_else(|error| panic!("{error}")),
        Some(OutboxState::Conflict)
    );
    let inbox = local
        .unresolved_conflicts(10)
        .await
        .unwrap_or_else(|error| panic!("{error}"));
    assert_eq!(inbox.len(), 1);
    local
        .resolve_conflict(operation_id, ConflictResolution::AcceptServer)
        .await
        .unwrap_or_else(|error| panic!("{error}"));
    assert!(
        local
            .unresolved_conflicts(10)
            .await
            .unwrap_or_else(|error| panic!("{error}"))
            .is_empty()
    );
}

#[tokio::test]
async fn retrying_an_operation_returns_the_ledger_result_without_a_second_effect() {
    let fixture = Fixture::new();
    let authoritative = InMemoryAuthoritativeStore::default();
    let service = server(&authoritative);
    let request = fixture.request(fixture.operation());

    let first = service
        .exchange(fixture.auth, request.clone())
        .await
        .unwrap_or_else(|error| panic!("{error}"));
    let second = service
        .exchange(fixture.auth, request)
        .await
        .unwrap_or_else(|error| panic!("{error}"));

    assert!(!first.acknowledged[0].duplicate);
    assert!(second.acknowledged[0].duplicate);
    assert_eq!(
        first.acknowledged[0].sequence,
        second.acknowledged[0].sequence
    );
    assert_eq!(authoritative.applied_operation_count(), 1);
}

#[tokio::test]
async fn stale_creation_is_reported_as_a_conflict() {
    let fixture = Fixture::new();
    let authoritative = InMemoryAuthoritativeStore::default();
    let service = server(&authoritative);
    let first = fixture.request(fixture.operation());
    service
        .exchange(fixture.auth, first)
        .await
        .unwrap_or_else(|error| panic!("{error}"));

    let stale = fixture.request(fixture.operation());
    let response = service
        .exchange(fixture.auth, stale)
        .await
        .unwrap_or_else(|error| panic!("{error}"));

    assert_eq!(response.acknowledged.len(), 0);
    assert_eq!(response.conflicts.len(), 1);
    assert_eq!(response.conflicts[0].policy, ConflictPolicy::Reject);
    assert_eq!(authoritative.applied_operation_count(), 1);
}

#[tokio::test]
async fn registered_field_merger_commits_a_deterministic_stale_update() {
    struct ProfileUpdate;
    impl TypedOperation for ProfileUpdate {
        const KIND: u16 = 1;
    }

    let fixture = Fixture::new();
    let authoritative = InMemoryAuthoritativeStore::default();
    let mut policies = ConflictPolicyRegistry::default();
    policies.register_merger::<ProfileUpdate, _>(ConflictPolicy::FieldMerge, FieldSetMerger);
    let service: Arc<dyn ExchangeService> = Arc::new(SyncServer::new(
        Arc::new(authoritative.clone()),
        Arc::new(CopyPayloadExecutor),
        Arc::new(policies),
        Arc::new(TestClock::new(NodeId::new(), 10_000)),
    ));
    let timestamp = |physical_ms| HybridTimestamp {
        physical_ms,
        logical: 0,
        node: NodeId::new(),
    };
    let mut initial = fixture.operation();
    initial.payload = postcard::to_stdvec(&FieldSet::canonical(vec![FieldValue {
        field: 1,
        timestamp: timestamp(1),
        value: b"Ada".to_vec(),
    }]))
    .unwrap_or_else(|error| panic!("{error}"));
    service
        .exchange(fixture.auth, fixture.request(initial))
        .await
        .unwrap_or_else(|error| panic!("{error}"));
    let mut stale = fixture.operation();
    stale.payload = postcard::to_stdvec(&FieldSet::canonical(vec![FieldValue {
        field: 2,
        timestamp: timestamp(2),
        value: b"ada@example.test".to_vec(),
    }]))
    .unwrap_or_else(|error| panic!("{error}"));

    let response = service
        .exchange(fixture.auth, fixture.request(stale))
        .await
        .unwrap_or_else(|error| panic!("{error}"));

    assert_eq!(response.acknowledged.len(), 1);
    assert!(response.conflicts.is_empty());
    let stored = authoritative
        .entity(fixture.tenant, fixture.entity)
        .unwrap_or_else(|| panic!("missing merged entity"));
    let merged: FieldSet =
        postcard::from_bytes(&stored.current.payload).unwrap_or_else(|error| panic!("{error}"));
    assert_eq!(merged.fields.len(), 2);
    assert_eq!(stored.current.version.get(), 2);
}

#[tokio::test]
async fn spoofed_operation_tenant_is_rejected_without_an_effect() {
    let fixture = Fixture::new();
    let authoritative = InMemoryAuthoritativeStore::default();
    let service = server(&authoritative);
    let mut operation = fixture.operation();
    operation.tenant_id = TenantId::new();

    let response = service
        .exchange(fixture.auth, fixture.request(operation))
        .await
        .unwrap_or_else(|error| panic!("{error}"));

    assert_eq!(response.rejected.len(), 1);
    assert_eq!(authoritative.applied_operation_count(), 0);
}

#[tokio::test]
async fn dependencies_execute_in_topological_order_even_when_submitted_in_reverse() {
    let fixture = Fixture::new();
    let authoritative = InMemoryAuthoritativeStore::default();
    let service = server(&authoritative);
    let parent = fixture.operation();
    let mut child = fixture.operation();
    child.entity.entity_id = EntityId::new();
    child.metadata.dependencies.push(parent.operation_id);
    let mut request = fixture.request(child);
    request.operations.push(parent);

    let response = service
        .exchange(fixture.auth, request)
        .await
        .unwrap_or_else(|error| panic!("{error}"));

    assert_eq!(response.acknowledged.len(), 2);
    assert_eq!(response.rejected.len(), 0);
    assert_eq!(authoritative.applied_operation_count(), 2);
}

#[tokio::test]
async fn dependency_cycles_fail_before_any_effect_commits() {
    let fixture = Fixture::new();
    let authoritative = InMemoryAuthoritativeStore::default();
    let service = server(&authoritative);
    let mut first = fixture.operation();
    let mut second = fixture.operation();
    second.entity.entity_id = EntityId::new();
    first.metadata.dependencies.push(second.operation_id);
    second.metadata.dependencies.push(first.operation_id);
    let mut request = fixture.request(first);
    request.operations.push(second);

    let result = service.exchange(fixture.auth, request).await;

    assert!(matches!(
        result,
        Err(aequora_server::ServerError::Dependency(_))
    ));
    assert_eq!(authoritative.applied_operation_count(), 0);
}

#[tokio::test]
async fn transient_transport_failures_retry_without_duplicate_effects() {
    let fixture = Fixture::new();
    let authoritative = InMemoryAuthoritativeStore::default();
    let local = InMemoryLocalStore::default();
    local
        .append_operation(fixture.operation())
        .await
        .unwrap_or_else(|error| panic!("{error}"));
    let transport = FaultInjectingTransport::new(
        InProcessTransport::new(server(&authoritative), fixture.auth),
        [TransportError::transient("simulated disconnect")],
    );
    let mut config = ClientConfig::new(fixture.session);
    config.retry = RetryConfig {
        max_attempts: 2,
        initial_delay: Duration::ZERO,
        max_delay: Duration::ZERO,
        multiplier: 2,
        jitter_percent: 0,
    };
    let metrics = Arc::new(AtomicMetrics::default());
    let engine = ClientSyncEngine::new(local, transport, config).with_observer(metrics.clone());

    let outcome = engine
        .run_with_retry()
        .await
        .unwrap_or_else(|error| panic!("{error}"));

    assert_eq!(outcome.acknowledged, 1);
    assert_eq!(engine.store().pending_count(), 0);
    assert_eq!(authoritative.applied_operation_count(), 1);
    assert_eq!(metrics.snapshot().retries, 1);
}

#[tokio::test]
async fn every_authoritative_transaction_phase_is_retry_safe() {
    let failpoints = [
        CommitFailPoint::BeforeWrite,
        CommitFailPoint::AfterWrite,
        CommitFailPoint::BeforeJournal,
        CommitFailPoint::AfterJournal,
        CommitFailPoint::BeforeLedger,
        CommitFailPoint::AfterLedger,
        CommitFailPoint::BeforeAudit,
        CommitFailPoint::AfterAudit,
        CommitFailPoint::BeforeCommit,
        CommitFailPoint::AfterCommit,
    ];
    for failpoint in failpoints {
        let fixture = Fixture::new();
        let authoritative = InMemoryAuthoritativeStore::default();
        authoritative.inject_commit_failure(failpoint);
        let local = InMemoryLocalStore::default();
        local
            .append_operation(fixture.operation())
            .await
            .unwrap_or_else(|error| panic!("{error}"));
        let mut config = ClientConfig::new(fixture.session);
        config.retry = RetryConfig {
            max_attempts: 2,
            initial_delay: Duration::ZERO,
            max_delay: Duration::ZERO,
            multiplier: 1,
            jitter_percent: 0,
        };
        let engine = ClientSyncEngine::new(
            local.clone(),
            InProcessTransport::new(server(&authoritative), fixture.auth),
            config,
        );

        engine
            .run_with_retry()
            .await
            .unwrap_or_else(|error| panic!("{failpoint:?}: {error}"));
        assert_eq!(authoritative.applied_operation_count(), 1, "{failpoint:?}");
        assert_eq!(authoritative.journal_len(), 1, "{failpoint:?}");
        assert_eq!(authoritative.audit_len(), 1, "{failpoint:?}");
        assert_eq!(local.pending_count(), 0, "{failpoint:?}");
    }
}

#[tokio::test]
async fn full_sync_drains_multiple_static_batches() {
    let fixture = Fixture::new();
    let authoritative = InMemoryAuthoritativeStore::default();
    let local = InMemoryLocalStore::default();
    for _ in 0..3 {
        let mut operation = fixture.operation();
        operation.entity.entity_id = EntityId::new();
        local
            .append_operation(operation)
            .await
            .unwrap_or_else(|error| panic!("{error}"));
    }
    let mut config = ClientConfig::new(fixture.session);
    config.push_batch_size = 1;
    let engine = ClientSyncEngine::new(
        local,
        InProcessTransport::new(server(&authoritative), fixture.auth),
        config,
    );

    let summary = engine
        .sync()
        .await
        .unwrap_or_else(|error| panic!("{error}"));

    assert_eq!(summary.exchanges, 3);
    assert_eq!(summary.acknowledged, 3);
    assert_eq!(summary.changes, 3);
    assert_eq!(engine.store().pending_count(), 0);
}

#[tokio::test]
async fn snapshot_bootstrap_resumes_from_durable_staging_progress() {
    let fixture = Fixture::new();
    let authoritative = InMemoryAuthoritativeStore::default();
    let service = server(&authoritative);
    for _ in 0..3 {
        let mut operation = fixture.operation();
        operation.entity.entity_id = EntityId::new();
        service
            .exchange(fixture.auth, fixture.request(operation))
            .await
            .unwrap_or_else(|error| panic!("{error}"));
    }
    let local = InMemoryLocalStore::default();
    let first = service
        .bootstrap(
            fixture.auth,
            BootstrapRequest {
                protocol: ProtocolVersion::V1,
                request_id: RequestId::new(),
                session: fixture.session.clone(),
                snapshot_id: None,
                offset: 0,
                limits: SnapshotLimits {
                    max_entities: 1,
                    max_payload_bytes: 1_024,
                },
                capabilities: vec![Capability::SnapshotV1],
            },
        )
        .await
        .unwrap_or_else(|error| panic!("{error}"));
    assert!(first.has_more);
    local
        .stage_snapshot(&first)
        .await
        .unwrap_or_else(|error| panic!("{error}"));
    let mut config = ClientConfig::new(fixture.session.clone());
    config.snapshot_limits = SnapshotLimits {
        max_entities: 1,
        max_payload_bytes: 1_024,
    };
    let engine = ClientSyncEngine::new(
        local,
        InProcessTransport::new(service, fixture.auth),
        config,
    );

    let outcome = engine
        .bootstrap()
        .await
        .unwrap_or_else(|error| panic!("{error}"));

    assert_eq!(outcome.pages, 2);
    assert_eq!(outcome.entities, 2);
    assert_eq!(outcome.cursor.sequence.0, 3);
    assert_eq!(engine.store().entity_count(fixture.scope), 3);
    let cursor = engine
        .store()
        .load_cursor(fixture.scope)
        .await
        .unwrap_or_else(|error| panic!("{error}"));
    assert_eq!(cursor, Some(outcome.cursor));
}

#[tokio::test]
async fn streaming_snapshot_stages_bounded_pages_and_commits_once() {
    let fixture = Fixture::new();
    let authoritative = InMemoryAuthoritativeStore::default();
    let service = server(&authoritative);
    for _ in 0..3 {
        let mut operation = fixture.operation();
        operation.entity.entity_id = EntityId::new();
        service
            .exchange(fixture.auth, fixture.request(operation))
            .await
            .unwrap_or_else(|error| panic!("{error}"));
    }
    let local = InMemoryLocalStore::default();
    let mut config = ClientConfig::new(fixture.session.clone());
    config.snapshot_limits = SnapshotLimits {
        max_entities: 1,
        max_payload_bytes: 1_024,
    };
    let engine = ClientSyncEngine::new(
        local,
        InProcessTransport::new(service, fixture.auth),
        config,
    );

    let outcome = engine
        .bootstrap_streaming()
        .await
        .unwrap_or_else(|error| panic!("{error}"));

    assert_eq!(outcome.pages, 3);
    assert_eq!(outcome.entities, 3);
    assert_eq!(engine.store().entity_count(fixture.scope), 3);
    assert_eq!(
        engine
            .store()
            .load_cursor(fixture.scope)
            .await
            .unwrap_or_else(|error| panic!("{error}")),
        Some(outcome.cursor)
    );
}

#[tokio::test]
async fn tombstones_remain_in_authoritative_state_and_journal() {
    let fixture = Fixture::new();
    let authoritative = InMemoryAuthoritativeStore::default();
    let service = server(&authoritative);
    service
        .exchange(fixture.auth, fixture.request(fixture.operation()))
        .await
        .unwrap_or_else(|error| panic!("{error}"));
    let mut deletion = fixture.operation();
    deletion.base_version = Some(aequora_types::EntityVersion::INITIAL);
    deletion.operation_kind = OperationKind(2);
    deletion.payload.clear();

    let response = service
        .exchange(fixture.auth, fixture.request(deletion))
        .await
        .unwrap_or_else(|error| panic!("{error}"));

    let current = authoritative
        .entity(fixture.tenant, fixture.entity)
        .unwrap_or_else(|| panic!("missing tombstone"));
    assert!(current.current.tombstone);
    assert!(
        response
            .changes
            .iter()
            .any(|change| change.change_kind == ChangeKind::Tombstone)
    );
}

#[tokio::test]
async fn partial_scope_is_authorized_before_any_data_is_read() {
    let fixture = Fixture::new();
    let authoritative = InMemoryAuthoritativeStore::default();
    let service: Arc<dyn ExchangeService> = Arc::new(SyncServer::new(
        Arc::new(authoritative.clone()),
        Arc::new(DenyScopeExecutor),
        Arc::new(RejectConflicts),
        Arc::new(TestClock::new(NodeId::new(), 10_000)),
    ));

    let result = service
        .exchange(fixture.auth, fixture.request(fixture.operation()))
        .await;

    assert!(matches!(
        result,
        Err(aequora_server::ServerError::ScopeAuthorization(_))
    ));
    assert_eq!(authoritative.applied_operation_count(), 0);
}

#[tokio::test]
async fn lost_response_after_commit_retries_to_the_same_logical_effect() {
    let fixture = Fixture::new();
    let authoritative = InMemoryAuthoritativeStore::default();
    let local = InMemoryLocalStore::default();
    local
        .append_operation(fixture.operation())
        .await
        .unwrap_or_else(|error| panic!("{error}"));
    let transport = ResponseDroppingTransport::new(
        InProcessTransport::new(server(&authoritative), fixture.auth),
        1,
    );
    let mut config = ClientConfig::new(fixture.session.clone());
    config.retry = RetryConfig {
        max_attempts: 2,
        initial_delay: Duration::ZERO,
        max_delay: Duration::ZERO,
        multiplier: 2,
        jitter_percent: 0,
    };
    let engine = ClientSyncEngine::new(local, transport, config);

    let outcome = engine
        .run_with_retry()
        .await
        .unwrap_or_else(|error| panic!("{error}"));

    assert_eq!(outcome.acknowledged, 1);
    assert_eq!(authoritative.applied_operation_count(), 1);
    assert_eq!(engine.store().pending_count(), 0);
}

#[tokio::test]
async fn journal_compaction_preserves_the_idempotency_ledger() {
    let fixture = Fixture::new();
    let authoritative = InMemoryAuthoritativeStore::default();
    let service = server(&authoritative);
    for _ in 0..3 {
        let mut operation = fixture.operation();
        operation.entity.entity_id = EntityId::new();
        service
            .exchange(fixture.auth, fixture.request(operation))
            .await
            .unwrap_or_else(|error| panic!("{error}"));
    }

    let removed = authoritative
        .compact_journal(fixture.tenant, fixture.scope, aequora_types::Sequence(2))
        .await
        .unwrap_or_else(|error| panic!("{error}"));

    assert_eq!(removed, 2);
    assert_eq!(authoritative.journal_len(), 1);
    assert_eq!(authoritative.applied_operation_count(), 3);
    assert_eq!(authoritative.audit_len(), 3);
    let audit = authoritative
        .read_audit_after(fixture.tenant, AuditOffset(0), 10)
        .await
        .unwrap_or_else(|error| panic!("{error}"));
    assert_eq!(audit.records.len(), 3);
    assert!(!audit.has_more);
    assert_eq!(audit.next_offset, AuditOffset(3));
    assert!(
        audit
            .records
            .iter()
            .all(|record| record.tenant_id == fixture.tenant)
    );
    assert!(
        audit
            .records
            .iter()
            .all(|record| record.command_digest != [0; 32])
    );
}

#[tokio::test]
async fn observers_cover_client_server_and_compute_boundaries_without_payloads() {
    let fixture = Fixture::new();
    let authoritative = InMemoryAuthoritativeStore::default();
    let mut existing = fixture.operation();
    existing.entity.entity_id = EntityId::new();
    server(&authoritative)
        .exchange(fixture.auth, fixture.request(existing))
        .await
        .unwrap_or_else(|error| panic!("{error}"));
    let local = InMemoryLocalStore::default();
    local
        .append_operation(fixture.operation())
        .await
        .unwrap_or_else(|error| panic!("{error}"));
    let client_metrics = Arc::new(AtomicMetrics::default());
    let server_metrics = Arc::new(AtomicMetrics::default());
    let compute = Arc::new(
        ComputePool::new(ComputeConfig {
            worker_threads: 1,
            parallel_threshold: 1,
        })
        .unwrap_or_else(|error| panic!("{error}")),
    );
    let service: Arc<dyn ExchangeService> = Arc::new(
        SyncServer::new(
            Arc::new(authoritative),
            Arc::new(CopyPayloadExecutor),
            Arc::new(RejectConflicts),
            Arc::new(TestClock::new(NodeId::new(), 10_000)),
        )
        .with_compute_pool(compute)
        .with_observer(server_metrics.clone()),
    );
    let transport = InProcessTransport::new(service, fixture.auth);
    let mut config = ClientConfig::new(fixture.session);
    config.limits.max_changes = 1;
    let engine =
        ClientSyncEngine::new(local, transport, config).with_observer(client_metrics.clone());

    engine
        .run_once()
        .await
        .unwrap_or_else(|error| panic!("{error}"));

    let client = client_metrics.snapshot();
    assert_eq!(client.client_exchanges, 1);
    assert_eq!(client.operations, 1);
    assert_eq!(client.changes, 1);
    assert_eq!(client.failures, 0);
    let server = server_metrics.snapshot();
    assert_eq!(server.server_exchanges, 1);
    assert_eq!(server.operations, 1);
    assert_eq!(server.changes, 1);
    assert_eq!(server.compute_offloads, 1);
    assert_eq!(server.journal_lag, 1);
    assert_eq!(server.failures, 0);
}

#[tokio::test]
async fn push_hints_are_advisory_and_tenant_scope_bounded() {
    let fixture = Fixture::new();
    let expected = PushHint {
        protocol: ProtocolVersion::V1,
        tenant_id: fixture.tenant,
        scope_id: fixture.scope,
        sequence: Sequence(7),
        reason: PushHintReason::JournalAdvanced,
        region_id: None,
    };
    let engine = ClientSyncEngine::new(
        InMemoryLocalStore::default(),
        HintOnlyTransport(expected),
        ClientConfig::new(fixture.session.clone()),
    );

    let hint = engine
        .wait_for_push_hint()
        .await
        .unwrap_or_else(|error| panic!("{error}"));

    assert_eq!(hint, expected);
    assert_eq!(engine.store().pending_count(), 0);

    let invalid = PushHint {
        tenant_id: TenantId::new(),
        ..expected
    };
    let invalid_engine = ClientSyncEngine::new(
        InMemoryLocalStore::default(),
        HintOnlyTransport(invalid),
        ClientConfig::new(fixture.session),
    );
    assert!(matches!(
        invalid_engine.wait_for_push_hint().await,
        Err(ClientError::PushHintBoundary)
    ));
}

#[tokio::test]
async fn coordinator_reports_status_without_a_ui_framework_dependency() {
    let fixture = Fixture::new();
    let authoritative = InMemoryAuthoritativeStore::default();
    let local = InMemoryLocalStore::default();
    local
        .append_operation(fixture.operation())
        .await
        .unwrap_or_else(|error| panic!("{error}"));
    let exchanges = Arc::new(AtomicUsize::new(0));
    let engine = Arc::new(ClientSyncEngine::new(
        local.clone(),
        CountingTransport {
            inner: InProcessTransport::new(server(&authoritative), fixture.auth),
            exchanges: exchanges.clone(),
        },
        ClientConfig::new(fixture.session),
    ));
    let (coordinator, handle) = SyncCoordinator::new(
        engine,
        SyncCoordinatorConfig {
            channel_capacity: 4,
            periodic_interval: None,
            sync_on_start: false,
            mutation_debounce: Duration::from_millis(10),
        },
    );
    let mut status = handle.subscribe();
    let task = tokio::spawn(coordinator.run());

    handle
        .trigger(SyncTrigger::LocalMutation)
        .await
        .unwrap_or_else(|error| panic!("{error}"));
    handle
        .trigger(SyncTrigger::LocalMutation)
        .await
        .unwrap_or_else(|error| panic!("{error}"));
    handle
        .trigger(SyncTrigger::LocalMutation)
        .await
        .unwrap_or_else(|error| panic!("{error}"));
    loop {
        status
            .changed()
            .await
            .unwrap_or_else(|error| panic!("{error}"));
        if matches!(*status.borrow(), SyncStatus::Idle { last_sync: Some(_) }) {
            break;
        }
    }
    assert_eq!(local.pending_count(), 0);
    assert_eq!(exchanges.load(Ordering::Relaxed), 1);
    handle
        .trigger(SyncTrigger::Shutdown)
        .await
        .unwrap_or_else(|error| panic!("{error}"));
    task.await.unwrap_or_else(|error| panic!("{error}"));
    assert_eq!(handle.status(), SyncStatus::Shutdown);
    let health = handle.health();
    assert_eq!(health.pending_operations, 0);
    assert_eq!(health.conflicts_pending, 0);
    assert!(health.last_successful_sync_unix_ms.is_some());
}
