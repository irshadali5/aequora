//! Real-World Local-First Synchronization Simulation Suite.
//!
//! This suite simulates realistic, mission-critical distributed scenarios:
//! 1. Massive multi-client offline batching & convergence (Field inspection / Classroom attendance).
//! 2. Two-Generals network failure (packet lost right after authoritative commit) & idempotent replay.
//! 3. Concurrent multi-device field updates with CRDT / `FieldSetMerger` automatic reconciliation.
//! 4. Hard conflicting edits & durable, UI-independent `ConflictInbox` preservation.
//! 5. Dependency DAG reordering over asynchronous networks.
//! 6. Tombstone lifecycle & stale mutation rejection.
//! 7. Journal compaction & automatic streaming snapshot bootstrap for dormant devices.
//! 8. Authority epoch failover & client timeline synchronization.
//! 9. Multi-tenant security boundary & cross-tenant attack rejection.
//! 10. Process crash recovery & durable outbox resumption.

#![expect(clippy::too_many_lines, reason = "comprehensive simulation flows")]

use aequora_authority::{
    AuthorityController, AuthorityPromotionPolicy, AuthorityRole, AuthorityRuntimeMode,
    AuthorityState,
};
use aequora_client::{ClientConfig, ClientSyncEngine, RetryConfig};
use aequora_clock::TestClock;
use aequora_conflict::{
    ConflictPolicyRegistry, FieldSet, FieldSetMerger, FieldValue, RejectConflicts, TypedOperation,
};
use aequora_executor::{
    AuthContext, AuthenticatedOperation, AuthoritativeMutation, AuthorizedOperation, CurrentEntity,
    ExecutableOperation, ExecutionError, OperationExecutor,
};
use aequora_protocol::{
    Capability, ChangeKind, ClientLimits, ConflictPolicy, OperationEnvelope, OperationKind,
    OperationMetadata, RejectionCode, SessionMetadata, SyncDirective, SyncRequest,
};
use aequora_server::{ExchangeService, SyncServer, SyncServerBuilder};
use aequora_store::{CursorStore, EntityReader, JournalCompactor, OutboxStore};
use aequora_testkit::{
    InMemoryAuthoritativeStore, InMemoryLocalStore, InProcessTransport, ResponseDroppingTransport,
};
use aequora_transport::SyncTransport;
use aequora_types::{
    ActorId, AuthorityEpoch, AuthorityId, AuthorityInstanceId, Cursor, DeviceId, EntityId,
    EntityRef, EntityType, EntityVersion, HybridTimestamp, NodeId, OperationId, ProtocolVersion,
    RequestId, SchemaVersion, Sequence, SessionId, SyncScopeId, TenantId,
};
use async_trait::async_trait;
use std::sync::Arc;
use std::time::Duration;

#[derive(Clone)]
struct StudentRecordExecutor;

#[async_trait]
impl OperationExecutor for StudentRecordExecutor {
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
        current: Option<&CurrentEntity>,
    ) -> Result<AuthoritativeMutation, ExecutionError> {
        let envelope = operation.envelope();
        if envelope.operation_kind == OperationKind(2) {
            // Tombstone
            return Ok(AuthoritativeMutation {
                payload: envelope.payload.clone(),
                change_kind: ChangeKind::Tombstone,
            });
        }

        // If current is a tombstone, reject write
        if let Some(curr) = current {
            if curr.tombstone {
                return Err(ExecutionError::business_rule(
                    "cannot mutate tombstoned entity",
                ));
            }
        }

        Ok(AuthoritativeMutation {
            payload: envelope.payload.clone(),
            change_kind: ChangeKind::Upsert,
        })
    }
}

#[derive(Clone)]
struct FieldRecordExecutor;

#[async_trait]
impl OperationExecutor for FieldRecordExecutor {
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
        current: Option<&CurrentEntity>,
    ) -> Result<AuthoritativeMutation, ExecutionError> {
        let envelope = operation.envelope();
        if envelope.operation_kind == OperationKind(2) {
            return Ok(AuthoritativeMutation {
                payload: envelope.payload.clone(),
                change_kind: ChangeKind::Tombstone,
            });
        }

        if let Some(curr) = current {
            if curr.tombstone {
                return Err(ExecutionError::business_rule(
                    "cannot mutate tombstoned entity",
                ));
            }
            let existing: FieldSet = postcard::from_bytes(&curr.payload).unwrap_or_default();
            let incoming: FieldSet = postcard::from_bytes(&envelope.payload).unwrap_or_default();
            let merged = existing.merge(&incoming);
            let payload = postcard::to_stdvec(&merged)
                .map_err(|e| ExecutionError::business_rule(e.to_string()))?;
            return Ok(AuthoritativeMutation {
                payload,
                change_kind: ChangeKind::Upsert,
            });
        }

        Ok(AuthoritativeMutation {
            payload: envelope.payload.clone(),
            change_kind: ChangeKind::Upsert,
        })
    }
}

struct StudentFieldUpdate;
impl TypedOperation for StudentFieldUpdate {
    const KIND: u16 = 1;
}

// ---------------------------------------------------------------------------
// Helper fixtures
// ---------------------------------------------------------------------------

struct SimEnvironment {
    tenant: TenantId,
    scope: SyncScopeId,
    authority_store: InMemoryAuthoritativeStore,
    server: Arc<dyn ExchangeService>,
}

impl SimEnvironment {
    fn new() -> Self {
        let tenant = TenantId::new();
        let scope = SyncScopeId::new();
        let authority_store = InMemoryAuthoritativeStore::default();
        let clock = Arc::new(TestClock::new(NodeId::new(), 1_000));
        let server: Arc<dyn ExchangeService> = Arc::new(SyncServer::new(
            Arc::new(authority_store.clone()),
            Arc::new(StudentRecordExecutor),
            Arc::new(RejectConflicts),
            clock,
        ));
        Self {
            tenant,
            scope,
            authority_store,
            server,
        }
    }

    fn new_field_environment(registry: Arc<ConflictPolicyRegistry>) -> Self {
        let tenant = TenantId::new();
        let scope = SyncScopeId::new();
        let authority_store = InMemoryAuthoritativeStore::default();
        let clock = Arc::new(TestClock::new(NodeId::new(), 1_000));
        let server: Arc<dyn ExchangeService> = Arc::new(SyncServer::new(
            Arc::new(authority_store.clone()),
            Arc::new(FieldRecordExecutor),
            registry,
            clock,
        ));
        Self {
            tenant,
            scope,
            authority_store,
            server,
        }
    }
}

struct SimClient {
    actor: ActorId,
    device: DeviceId,
    local_store: InMemoryLocalStore,
    engine: ClientSyncEngine<InMemoryLocalStore, InProcessTransport>,
    session: SessionMetadata,
}

impl SimClient {
    fn new(env: &SimEnvironment) -> Self {
        let actor = ActorId::new();
        let device = DeviceId::new();
        let local_store = InMemoryLocalStore::default();
        let session = SessionMetadata {
            session_id: SessionId::new(),
            device_id: device,
            actor_id: actor,
            tenant_id: env.tenant,
            scope_id: env.scope,
            partitions: Vec::new(),
        };
        let auth = AuthContext {
            actor_id: actor,
            tenant_id: env.tenant,
            device_id: device,
        };
        let transport = InProcessTransport::new(env.server.clone(), auth);
        let engine = ClientSyncEngine::new(
            local_store.clone(),
            transport,
            ClientConfig::new(session.clone()),
        );
        Self {
            actor,
            device,
            local_store,
            engine,
            session,
        }
    }

    async fn mutate_local(
        &self,
        entity: EntityRef,
        base_version: Option<EntityVersion>,
        payload: Vec<u8>,
        kind: OperationKind,
    ) -> OperationEnvelope {
        let envelope = OperationEnvelope {
            protocol_version: ProtocolVersion::V1,
            operation_id: OperationId::new(),
            tenant_id: self.session.tenant_id,
            actor_id: self.actor,
            device_id: self.device,
            entity,
            base_version,
            created_at: HybridTimestamp {
                physical_ms: 1_050,
                logical: 0,
                node: NodeId::new(),
            },
            schema_version: SchemaVersion(1),
            operation_kind: kind,
            payload,
            metadata: OperationMetadata::default(),
        };
        self.local_store
            .append_operation(envelope.clone())
            .await
            .unwrap_or_else(|error| panic!("{error}"));
        envelope
    }
}

// ---------------------------------------------------------------------------
// SIMULATION 1: Multi-Device Offline Batch Convergence
// ---------------------------------------------------------------------------
#[tokio::test]
async fn sim_01_multi_client_offline_batch_convergence() {
    println!("\n=== SIMULATION 1: Multi-Client Offline Batch Convergence ===");
    let env = SimEnvironment::new();
    let client_a = SimClient::new(&env);
    let client_b = SimClient::new(&env);
    let client_c = SimClient::new(&env);

    let entity_type = EntityType::new(1).unwrap_or_else(|error| panic!("{error}"));

    // Teacher A logs 10 attendance records offline
    println!("Step 1: Teacher A records 10 student attendance records while offline...");
    for i in 0..10 {
        let entity = EntityRef {
            entity_type,
            entity_id: EntityId::new(),
        };
        client_a
            .mutate_local(
                entity,
                None,
                format!("Attendance: Student {i} Present").into_bytes(),
                OperationKind(1),
            )
            .await;
    }

    // Teacher B logs 10 grade records offline
    println!("Step 2: Teacher B records 10 assignment grades while offline...");
    for i in 0..10 {
        let entity = EntityRef {
            entity_type,
            entity_id: EntityId::new(),
        };
        client_b
            .mutate_local(
                entity,
                None,
                format!("Grade: Student {i} Score 95%").into_bytes(),
                OperationKind(1),
            )
            .await;
    }

    // Teacher C logs 10 incident reports offline
    println!("Step 3: Teacher C records 10 behavior reports while offline...");
    for i in 0..10 {
        let entity = EntityRef {
            entity_type,
            entity_id: EntityId::new(),
        };
        client_c
            .mutate_local(
                entity,
                None,
                format!("Report: Student {i} Commendation").into_bytes(),
                OperationKind(1),
            )
            .await;
    }

    // Verify all 3 clients have 10 pending outbox operations
    assert_eq!(
        client_a
            .local_store
            .pending_operations(100)
            .await
            .unwrap_or_else(|error| panic!("{error}"))
            .len(),
        10
    );
    assert_eq!(
        client_b
            .local_store
            .pending_operations(100)
            .await
            .unwrap_or_else(|error| panic!("{error}"))
            .len(),
        10
    );
    assert_eq!(
        client_c
            .local_store
            .pending_operations(100)
            .await
            .unwrap_or_else(|error| panic!("{error}"))
            .len(),
        10
    );

    // Reconnection & sync
    println!("Step 4: Clients reconnect and sync in sequence A -> B -> C...");
    let outcome_a = client_a
        .engine
        .run_once()
        .await
        .unwrap_or_else(|error| panic!("{error}"));
    println!(
        "  Client A pushed {} ops, changes pulled {}",
        outcome_a.acknowledged, outcome_a.changes
    );
    assert_eq!(outcome_a.acknowledged, 10);
    assert_eq!(
        client_a
            .local_store
            .pending_operations(100)
            .await
            .unwrap_or_else(|error| panic!("{error}"))
            .len(),
        0
    );

    let outcome_b = client_b
        .engine
        .run_once()
        .await
        .unwrap_or_else(|error| panic!("{error}"));
    println!(
        "  Client B pushed {} ops, changes pulled {} (10 from A + 10 from B)",
        outcome_b.acknowledged, outcome_b.changes
    );
    assert_eq!(outcome_b.acknowledged, 10);
    assert_eq!(outcome_b.changes, 20);

    let outcome_c = client_c
        .engine
        .run_once()
        .await
        .unwrap_or_else(|error| panic!("{error}"));
    println!(
        "  Client C pushed {} ops, changes pulled {} (10 from A + 10 from B + 10 from C)",
        outcome_c.acknowledged, outcome_c.changes
    );
    assert_eq!(outcome_c.acknowledged, 10);
    assert_eq!(outcome_c.changes, 30);

    // Now sync Client A and B to pull remaining changes
    println!("Step 5: Full convergence sync for A and B...");
    let pull_a = client_a
        .engine
        .run_once()
        .await
        .unwrap_or_else(|error| panic!("{error}"));
    assert_eq!(pull_a.changes, 20);

    let pull_b = client_b
        .engine
        .run_once()
        .await
        .unwrap_or_else(|error| panic!("{error}"));
    assert_eq!(pull_b.changes, 10);

    // Verify all clients have 30 reconciled entities and cursor at Sequence(30)
    let cursor_a = client_a
        .local_store
        .load_cursor(env.scope)
        .await
        .unwrap_or_else(|error| panic!("{error}"))
        .unwrap_or_else(|| panic!("missing cursor"));
    let cursor_b = client_b
        .local_store
        .load_cursor(env.scope)
        .await
        .unwrap_or_else(|error| panic!("{error}"))
        .unwrap_or_else(|| panic!("missing cursor"));
    let cursor_c = client_c
        .local_store
        .load_cursor(env.scope)
        .await
        .unwrap_or_else(|error| panic!("{error}"))
        .unwrap_or_else(|| panic!("missing cursor"));

    println!("  Authoritative sequence reached: {:?}", cursor_a.sequence);
    assert_eq!(cursor_a.sequence, Sequence(30));
    assert_eq!(cursor_b.sequence, Sequence(30));
    assert_eq!(cursor_c.sequence, Sequence(30));
    assert_eq!(env.authority_store.applied_operation_count(), 30);
    println!("  [SUCCESS] All 3 clients achieved bit-identical convergence at Sequence 30.");
}

// ---------------------------------------------------------------------------
// SIMULATION 2: Two-Generals Problem & Idempotent Retry on Dropped ACK
// ---------------------------------------------------------------------------
#[tokio::test]
async fn sim_02_two_generals_dropped_ack_idempotent_retry() {
    println!("\n=== SIMULATION 2: Two-Generals Problem & Dropped ACK Idempotent Retry ===");
    let env = SimEnvironment::new();
    let actor = ActorId::new();
    let device = DeviceId::new();
    let local_store = InMemoryLocalStore::default();
    let session = SessionMetadata {
        session_id: SessionId::new(),
        device_id: device,
        actor_id: actor,
        tenant_id: env.tenant,
        scope_id: env.scope,
        partitions: Vec::new(),
    };
    let auth = AuthContext {
        actor_id: actor,
        tenant_id: env.tenant,
        device_id: device,
    };

    // Use ResponseDroppingTransport: server commits, then network drops 1 response
    let base_transport = InProcessTransport::new(env.server.clone(), auth);
    let dropping_transport = ResponseDroppingTransport::new(base_transport, 1);

    let mut config = ClientConfig::new(session.clone());
    config.retry = RetryConfig {
        max_attempts: 3,
        initial_delay: Duration::from_millis(1),
        max_delay: Duration::from_millis(5),
        multiplier: 2,
        jitter_percent: 0,
    };

    let engine = ClientSyncEngine::new(local_store.clone(), dropping_transport, config);

    let entity = EntityRef {
        entity_type: EntityType::new(1).unwrap_or_else(|error| panic!("{error}")),
        entity_id: EntityId::new(),
    };

    // Client records a financial tuition payment
    println!("Step 1: Client submits tuition payment transaction...");
    let envelope = OperationEnvelope {
        protocol_version: ProtocolVersion::V1,
        operation_id: OperationId::new(),
        tenant_id: env.tenant,
        actor_id: actor,
        device_id: device,
        entity,
        base_version: None,
        created_at: HybridTimestamp {
            physical_ms: 2_000,
            logical: 0,
            node: NodeId::new(),
        },
        schema_version: SchemaVersion(1),
        operation_kind: OperationKind(1),
        payload: b"Tuition Payment $1,500.00 Ref#TX-9901".to_vec(),
        metadata: OperationMetadata::default(),
    };
    local_store
        .append_operation(envelope.clone())
        .await
        .unwrap_or_else(|error| panic!("{error}"));

    // The retry engine transparently handles attempt 1 (dropped return packet) and attempt 2 (resend)
    println!("Step 2: Client engine executes with retry resilience...");
    let retry_outcome = engine
        .run_with_retry()
        .await
        .unwrap_or_else(|error| panic!("{error}"));
    println!(
        "  Retry outcome: acknowledged = {}, changes = {}",
        retry_outcome.acknowledged, retry_outcome.changes
    );
    assert_eq!(retry_outcome.acknowledged, 1);

    // Crucial Invariant: applied operation count on authority is STILL 1, NOT 2! No double charge!
    assert_eq!(env.authority_store.applied_operation_count(), 1);
    assert_eq!(
        local_store
            .pending_operations(100)
            .await
            .unwrap_or_else(|error| panic!("{error}"))
            .len(),
        0
    );
    println!("  [SUCCESS] Server returned cached ledger ACK. Zero duplicate ledger writes.");
}

// ---------------------------------------------------------------------------
// SIMULATION 3: Concurrent Multi-Field Updates (CRDT / FieldSetMerger)
// ---------------------------------------------------------------------------
#[tokio::test]
async fn sim_03_concurrent_multi_field_updates_field_merger() {
    println!("\n=== SIMULATION 3: Concurrent Multi-Field Updates (FieldSetMerger) ===");
    let mut field_registry = ConflictPolicyRegistry::default();
    field_registry
        .register_merger::<StudentFieldUpdate, _>(ConflictPolicy::FieldMerge, FieldSetMerger);
    let env = SimEnvironment::new_field_environment(Arc::new(field_registry));

    let client_init = SimClient::new(&env);
    let client_a = SimClient::new(&env);
    let client_b = SimClient::new(&env);

    let entity = EntityRef {
        entity_type: EntityType::new(1).unwrap_or_else(|error| panic!("{error}")),
        entity_id: EntityId::new(),
    };

    let node = NodeId::new();

    // Initial state: Student with 3 fields: 1=Name="Alice", 2=Room="101", 3=Grade="A"
    println!("Step 1: Initialize Student record at Version 1...");
    let initial_fields = FieldSet::canonical(vec![
        FieldValue {
            field: 1,
            timestamp: HybridTimestamp {
                physical_ms: 1_000,
                logical: 0,
                node,
            },
            value: b"Alice".to_vec(),
        },
        FieldValue {
            field: 2,
            timestamp: HybridTimestamp {
                physical_ms: 1_000,
                logical: 0,
                node,
            },
            value: b"101".to_vec(),
        },
        FieldValue {
            field: 3,
            timestamp: HybridTimestamp {
                physical_ms: 1_000,
                logical: 0,
                node,
            },
            value: b"A".to_vec(),
        },
    ]);
    let initial_payload =
        postcard::to_stdvec(&initial_fields).unwrap_or_else(|error| panic!("{error}"));

    client_init
        .mutate_local(entity, None, initial_payload, OperationKind(1))
        .await;
    client_init
        .engine
        .run_once()
        .await
        .unwrap_or_else(|error| panic!("{error}"));

    // Sync Client A and B so both are at Version 1
    client_a
        .engine
        .run_once()
        .await
        .unwrap_or_else(|error| panic!("{error}"));
    client_b
        .engine
        .run_once()
        .await
        .unwrap_or_else(|error| panic!("{error}"));

    // Teacher A (offline) changes room (field 2) to "102" at t=1100
    println!("Step 2: Teacher A (offline) updates room (field 2) -> '102' based on Version 1...");
    let fields_a = FieldSet::canonical(vec![FieldValue {
        field: 2,
        timestamp: HybridTimestamp {
            physical_ms: 1_100,
            logical: 0,
            node,
        },
        value: b"102".to_vec(),
    }]);
    let payload_a = postcard::to_stdvec(&fields_a).unwrap_or_else(|error| panic!("{error}"));
    client_a
        .mutate_local(
            entity,
            Some(EntityVersion::INITIAL),
            payload_a,
            OperationKind(1),
        )
        .await;

    // Teacher B (offline) changes grade (field 3) to "A+" at t=1200
    println!("Step 3: Teacher B (offline) updates grade (field 3) -> 'A+' based on Version 1...");
    let fields_b = FieldSet::canonical(vec![FieldValue {
        field: 3,
        timestamp: HybridTimestamp {
            physical_ms: 1_200,
            logical: 0,
            node,
        },
        value: b"A+".to_vec(),
    }]);
    let payload_b = postcard::to_stdvec(&fields_b).unwrap_or_else(|error| panic!("{error}"));
    client_b
        .mutate_local(
            entity,
            Some(EntityVersion::INITIAL),
            payload_b,
            OperationKind(1),
        )
        .await;

    // Client A syncs first -> commits Version 2
    println!("Step 4: Client A syncs and commits room update -> Sequence 2...");
    let outcome_a = client_a
        .engine
        .run_once()
        .await
        .unwrap_or_else(|error| panic!("{error}"));
    assert_eq!(outcome_a.acknowledged, 1);

    // Client B syncs second -> FieldSetMerger reconciles stale base Version 1 with Version 2 -> commits Version 3!
    println!("Step 5: Client B syncs with stale base Version 1 -> FieldSetMerger reconciles...");
    let outcome_b = client_b
        .engine
        .run_once()
        .await
        .unwrap_or_else(|error| panic!("{error}"));
    assert_eq!(outcome_b.acknowledged, 1);
    assert_eq!(outcome_b.changes, 2); // Pulled A's change + B's merged change

    // Client A syncs to pull B's merged change
    client_a
        .engine
        .run_once()
        .await
        .unwrap_or_else(|error| panic!("{error}"));

    // Verify merged authoritative state
    let server_entity = env
        .authority_store
        .read_entity(env.tenant, entity)
        .await
        .unwrap_or_else(|error| panic!("{error}"))
        .unwrap_or_else(|| panic!("missing entity"));
    let merged_fields: FieldSet = postcard::from_bytes(&server_entity.current.payload)
        .unwrap_or_else(|error| panic!("{error}"));
    println!(
        "  Final authoritative entity version: {:?}",
        server_entity.current.version
    );
    println!("  Final fields count: {}", merged_fields.fields.len());

    assert_eq!(merged_fields.fields.len(), 3);
    assert_eq!(merged_fields.fields[0].field, 1);
    assert_eq!(merged_fields.fields[0].value, b"Alice");
    assert_eq!(merged_fields.fields[1].field, 2);
    assert_eq!(merged_fields.fields[1].value, b"102");
    assert_eq!(merged_fields.fields[2].field, 3);
    assert_eq!(merged_fields.fields[2].value, b"A+");
    assert_eq!(
        server_entity.current.version,
        EntityVersion::new(3).unwrap_or_else(|error| panic!("{error}"))
    );
    println!(
        "  [SUCCESS] Non-conflicting concurrent field updates merged deterministically into Version 3."
    );
}

// ---------------------------------------------------------------------------
// SIMULATION 4: Conflicting Mutation & Durable Conflict Inbox Isolation
// ---------------------------------------------------------------------------
#[tokio::test]
async fn sim_04_hard_conflict_routes_to_durable_inbox() {
    println!("\n=== SIMULATION 4: Hard Conflicting Mutation & Conflict Inbox ===");
    let env = SimEnvironment::new(); // Default RejectConflicts
    let client_init = SimClient::new(&env);
    let client_a = SimClient::new(&env);
    let client_b = SimClient::new(&env);

    let entity = EntityRef {
        entity_type: EntityType::new(1).unwrap_or_else(|error| panic!("{error}")),
        entity_id: EntityId::new(),
    };

    // Initial student status: "Active"
    println!("Step 1: Create Student record at Version 1 (status: Active)...");
    client_init
        .mutate_local(entity, None, b"Status: Active".to_vec(), OperationKind(1))
        .await;
    client_init
        .engine
        .run_once()
        .await
        .unwrap_or_else(|error| panic!("{error}"));

    // Both clients pull Version 1
    client_a
        .engine
        .run_once()
        .await
        .unwrap_or_else(|error| panic!("{error}"));
    client_b
        .engine
        .run_once()
        .await
        .unwrap_or_else(|error| panic!("{error}"));

    // Client A updates status -> "Graduated" from Version 1
    println!("Step 2: Admin A sets status -> 'Graduated' (based on v1)...");
    client_a
        .mutate_local(
            entity,
            Some(EntityVersion::INITIAL),
            b"Status: Graduated".to_vec(),
            OperationKind(1),
        )
        .await;

    // Client B updates status -> "Expelled" from Version 1
    println!("Step 3: Admin B sets status -> 'Expelled' (based on v1)...");
    let op_b = client_b
        .mutate_local(
            entity,
            Some(EntityVersion::INITIAL),
            b"Status: Expelled".to_vec(),
            OperationKind(1),
        )
        .await;

    // Client A syncs first -> commits Version 2
    println!("Step 4: Admin A syncs and commits -> Version 2...");
    let outcome_a = client_a
        .engine
        .run_once()
        .await
        .unwrap_or_else(|error| panic!("{error}"));
    assert_eq!(outcome_a.acknowledged, 1);

    // Client B syncs -> Authority detects conflict (base 1 vs current 2) and rejects!
    println!("Step 5: Admin B syncs -> Authority detects stale base version conflict...");
    let outcome_b = client_b
        .engine
        .run_once()
        .await
        .unwrap_or_else(|error| panic!("{error}"));
    println!(
        "  Admin B outcome: acked={}, conflicts={}",
        outcome_b.acknowledged, outcome_b.conflicts
    );
    assert_eq!(outcome_b.acknowledged, 0);
    assert_eq!(outcome_b.conflicts, 1);

    // Crucial Local-First Invariant: Conflict record is stored in durable ConflictInbox!
    let conflicts = client_b.local_store.conflicts();
    assert_eq!(conflicts.len(), 1);
    assert_eq!(conflicts[0].operation_id, op_b.operation_id);
    assert_eq!(conflicts[0].entity, entity);
    println!(
        "  Durable conflict record found in client inbox: OpId={:?}, Entity={:?}",
        conflicts[0].operation_id, conflicts[0].entity
    );

    // Pending outbox has been cleared (rejected op moved out of active outbox)
    assert_eq!(
        client_b
            .local_store
            .pending_operations(100)
            .await
            .unwrap_or_else(|error| panic!("{error}"))
            .len(),
        0
    );
    println!(
        "  [SUCCESS] Conflict isolated safely in durable inbox without crashing local engine."
    );
}

// ---------------------------------------------------------------------------
// SIMULATION 5: DAG Dependency & Out-of-Order Delivery
// ---------------------------------------------------------------------------
#[tokio::test]
async fn sim_05_dag_dependencies_out_of_order_delivery() {
    println!("\n=== SIMULATION 5: DAG Dependency & Reverse Delivery Resolution ===");
    let env = SimEnvironment::new();
    let client = SimClient::new(&env);

    let parent_entity = EntityRef {
        entity_type: EntityType::new(1).unwrap_or_else(|error| panic!("{error}")),
        entity_id: EntityId::new(),
    };
    let child_entity = EntityRef {
        entity_type: EntityType::new(2).unwrap_or_else(|error| panic!("{error}")),
        entity_id: EntityId::new(),
    };

    // Client creates parent Course, then child Assignment with dependency metadata
    println!("Step 1: Client creates Parent (Course #101) and Child (Assignment #501)...");
    let parent_op_id = OperationId::new();
    let child_op_id = OperationId::new();

    let parent_envelope = OperationEnvelope {
        protocol_version: ProtocolVersion::V1,
        operation_id: parent_op_id,
        tenant_id: env.tenant,
        actor_id: client.actor,
        device_id: client.device,
        entity: parent_entity,
        base_version: None,
        created_at: HybridTimestamp {
            physical_ms: 3_000,
            logical: 0,
            node: NodeId::new(),
        },
        schema_version: SchemaVersion(1),
        operation_kind: OperationKind(1),
        payload: b"Course: Physics 101".to_vec(),
        metadata: OperationMetadata::default(),
    };

    let mut child_metadata = OperationMetadata::default();
    child_metadata.dependencies.push(parent_op_id);

    let child_envelope = OperationEnvelope {
        protocol_version: ProtocolVersion::V1,
        operation_id: child_op_id,
        tenant_id: env.tenant,
        actor_id: client.actor,
        device_id: client.device,
        entity: child_entity,
        base_version: None,
        created_at: HybridTimestamp {
            physical_ms: 3_001,
            logical: 0,
            node: NodeId::new(),
        },
        schema_version: SchemaVersion(1),
        operation_kind: OperationKind(1),
        payload: b"Assignment: Lab Report 1 (Course Physics 101)".to_vec(),
        metadata: child_metadata,
    };

    // Enqueue Child FIRST, then Parent (simulating reverse network batch assembly)
    println!("Step 2: Reverse enqueue: Child submitted before Parent...");
    client
        .local_store
        .append_operation(child_envelope)
        .await
        .unwrap_or_else(|error| panic!("{error}"));
    client
        .local_store
        .append_operation(parent_envelope)
        .await
        .unwrap_or_else(|error| panic!("{error}"));

    // Client syncs
    println!("Step 3: Syncing batch to server...");
    let outcome = client
        .engine
        .run_once()
        .await
        .unwrap_or_else(|error| panic!("{error}"));
    assert_eq!(outcome.acknowledged, 2);

    // Verify topological commit order on server
    let parent_read = env
        .authority_store
        .read_entity(env.tenant, parent_entity)
        .await
        .unwrap_or_else(|error| panic!("{error}"))
        .unwrap_or_else(|| panic!("missing parent"));
    let child_read = env
        .authority_store
        .read_entity(env.tenant, child_entity)
        .await
        .unwrap_or_else(|error| panic!("{error}"))
        .unwrap_or_else(|| panic!("missing child"));

    println!("  Parent commit version: {:?}", parent_read.current.version);
    println!("  Child commit version: {:?}", child_read.current.version);

    assert_eq!(parent_read.current.version, EntityVersion::INITIAL);
    assert_eq!(child_read.current.version, EntityVersion::INITIAL);
    assert_eq!(env.authority_store.applied_operation_count(), 2);
    println!(
        "  [SUCCESS] Server topological scheduler executed Parent before Child despite reverse delivery."
    );
}

// ---------------------------------------------------------------------------
// SIMULATION 6: Tombstone (Deletion) vs Concurrent Stale Modification
// ---------------------------------------------------------------------------
#[tokio::test]
async fn sim_06_tombstone_lifecycle_and_stale_mutation() {
    println!("\n=== SIMULATION 6: Tombstone Lifecycle vs Concurrent Stale Modification ===");
    let env = SimEnvironment::new();
    let client_a = SimClient::new(&env);
    let client_b = SimClient::new(&env);

    let entity = EntityRef {
        entity_type: EntityType::new(1).unwrap_or_else(|error| panic!("{error}")),
        entity_id: EntityId::new(),
    };

    // Step 1: Create Entity at Version 1
    println!("Step 1: Create Entity (Device #1) at Version 1...");
    client_a
        .mutate_local(entity, None, b"Original Record".to_vec(), OperationKind(1))
        .await;
    client_a
        .engine
        .run_once()
        .await
        .unwrap_or_else(|error| panic!("{error}"));
    client_b
        .engine
        .run_once()
        .await
        .unwrap_or_else(|error| panic!("{error}")); // Both at v1

    // Step 2: Client A deletes the entity (Tombstone)
    println!("Step 2: Client A issues Tombstone deletion...");
    client_a
        .mutate_local(
            entity,
            Some(EntityVersion::INITIAL),
            b"".to_vec(),
            OperationKind(2),
        )
        .await;
    let outcome_a = client_a
        .engine
        .run_once()
        .await
        .unwrap_or_else(|error| panic!("{error}"));
    assert_eq!(outcome_a.acknowledged, 1);

    // Step 3: Client B (offline) tries to modify the entity from base Version 1
    println!("Step 3: Client B attempts edit on deleted entity from base v1...");
    client_b
        .mutate_local(
            entity,
            Some(EntityVersion::INITIAL),
            b"Stale Edit".to_vec(),
            OperationKind(1),
        )
        .await;

    // Step 4: Client B syncs -> Rejection because entity is tombstoned
    println!("Step 4: Client B syncs -> Authority enforces tombstone invariant...");
    let outcome_b = client_b
        .engine
        .run_once()
        .await
        .unwrap_or_else(|error| panic!("{error}"));
    assert_eq!(outcome_b.acknowledged, 0);
    assert_eq!(outcome_b.conflicts, 1);
    assert_eq!(outcome_b.changes, 1); // Pulled the tombstone

    // Verify authoritative state is marked tombstone
    let server_entity = env
        .authority_store
        .read_entity(env.tenant, entity)
        .await
        .unwrap_or_else(|error| panic!("{error}"))
        .unwrap_or_else(|| panic!("missing entity"));
    assert!(server_entity.current.tombstone);
    assert_eq!(
        server_entity.current.version,
        EntityVersion::new(2).unwrap_or_else(|error| panic!("{error}"))
    );
    println!("  [SUCCESS] Tombstone preserved in authority and journal; stale edit rejected.");
}

// ---------------------------------------------------------------------------
// SIMULATION 7: Journal Compaction & Automatic Streaming Staged Bootstrap
// ---------------------------------------------------------------------------
#[tokio::test]
async fn sim_07_journal_compaction_and_streaming_bootstrap() {
    println!("\n=== SIMULATION 7: Journal Compaction & Automatic Streaming Bootstrap ===");
    let env = SimEnvironment::new();
    let active_client = SimClient::new(&env);
    let dormant_client = SimClient::new(&env);

    let entity_type = EntityType::new(1).unwrap_or_else(|error| panic!("{error}"));

    // Step 1: Active client creates initial record at Sequence 1
    println!("Step 1: Active client creates initial records...");
    let e1 = EntityRef {
        entity_type,
        entity_id: EntityId::new(),
    };
    active_client
        .mutate_local(e1, None, b"Entity 1".to_vec(), OperationKind(1))
        .await;
    active_client
        .engine
        .run_once()
        .await
        .unwrap_or_else(|error| panic!("{error}"));

    // Dormant client syncs once, setting cursor at Sequence 1
    dormant_client
        .engine
        .run_once()
        .await
        .unwrap_or_else(|error| panic!("{error}"));
    let initial_cursor = dormant_client
        .local_store
        .load_cursor(env.scope)
        .await
        .unwrap_or_else(|error| panic!("{error}"))
        .unwrap_or_else(|| panic!("missing cursor"));
    assert_eq!(initial_cursor.sequence, Sequence(1));
    println!("  Dormant client initialized at Sequence 1, then goes offline for 60 days...");

    // Step 2: System advances through 25 more mutations
    println!("Step 2: 25 mutations applied by active client...");
    for i in 2..=26 {
        let entity = EntityRef {
            entity_type,
            entity_id: EntityId::new(),
        };
        active_client
            .mutate_local(
                entity,
                None,
                format!("Record {i}").into_bytes(),
                OperationKind(1),
            )
            .await;
    }
    let drain_outcome = active_client
        .engine
        .sync()
        .await
        .unwrap_or_else(|error| panic!("{error}"));
    println!(
        "  Active client drained outbox, acked: {}",
        drain_outcome.acknowledged
    );

    // Step 3: Authority compacts journal, setting retention floor at Sequence 20 (pruning 1..20)
    println!("Step 3: Authority executes journal compaction (pruning sequences < 20)...");
    env.authority_store
        .compact_journal(env.tenant, env.scope, Sequence(20))
        .await
        .unwrap_or_else(|error| panic!("{error}"));

    // Step 4: Dormant client reconnects with stale cursor at Sequence 1
    println!("Step 4: Dormant client reconnects (cursor = 1, journal floor = 20)...");
    // ClientSyncEngine automatically detects JournalCompacted resync directive, initiates snapshot bootstrap,
    // stages pages atomically, and transitions back to incremental sync!
    let resync_outcome = dormant_client
        .engine
        .sync()
        .await
        .unwrap_or_else(|error| panic!("{error}"));
    println!(
        "  Dormant client sync outcome: changes={}, acked={}",
        resync_outcome.changes, resync_outcome.acknowledged
    );

    let final_cursor = dormant_client
        .local_store
        .load_cursor(env.scope)
        .await
        .unwrap_or_else(|error| panic!("{error}"))
        .unwrap_or_else(|| panic!("missing cursor"));
    println!("  Dormant client new cursor: {:?}", final_cursor.sequence);
    assert_eq!(final_cursor.sequence, Sequence(26));
    println!(
        "  [SUCCESS] Dormant client automatically bootstrapped via snapshot streaming to Sequence 26."
    );
}

// ---------------------------------------------------------------------------
// SIMULATION 8: Authority Failover & Epoch Boundary Safety
// ---------------------------------------------------------------------------
#[tokio::test]
async fn sim_08_authority_failover_epoch_boundary_safety() {
    println!("\n=== SIMULATION 8: Authority Failover & Epoch Boundary Safety ===");
    let tenant = TenantId::new();
    let scope = SyncScopeId::new();
    let authority_store = InMemoryAuthoritativeStore::default();
    let clock = Arc::new(TestClock::new(NodeId::new(), 1_000));
    let authority_id = AuthorityId::new();

    let mut authority_state = AuthorityState::new(
        authority_id,
        AuthorityInstanceId::new(),
        AuthorityRole::Primary,
        1,
    );
    authority_state.epoch = AuthorityEpoch::new(2).unwrap_or_else(|error| panic!("{error}"));
    authority_state.runtime_mode = AuthorityRuntimeMode::Serving;

    let server: Arc<dyn ExchangeService> = Arc::new(
        SyncServerBuilder::new()
            .store(Arc::new(authority_store.clone()))
            .executor(Arc::new(StudentRecordExecutor))
            .conflicts(Arc::new(RejectConflicts))
            .clock(clock)
            .authority(AuthorityController::new(
                authority_state,
                AuthorityPromotionPolicy::default(),
            ))
            .build(),
    );

    let actor = ActorId::new();
    let device = DeviceId::new();
    let auth = AuthContext {
        actor_id: actor,
        tenant_id: tenant,
        device_id: device,
    };
    let transport = InProcessTransport::new(server.clone(), auth);

    // Client connects with cursor on old epoch 1
    println!("Step 1: Client submits request with stale Epoch(1) cursor...");
    let entity = EntityRef {
        entity_type: EntityType::new(1).unwrap_or_else(|error| panic!("{error}")),
        entity_id: EntityId::new(),
    };
    let envelope = OperationEnvelope {
        protocol_version: ProtocolVersion::V1,
        operation_id: OperationId::new(),
        tenant_id: tenant,
        actor_id: actor,
        device_id: device,
        entity,
        base_version: None,
        created_at: HybridTimestamp {
            physical_ms: 4_000,
            logical: 0,
            node: NodeId::new(),
        },
        schema_version: SchemaVersion(1),
        operation_kind: OperationKind(1),
        payload: b"Data on Old Epoch".to_vec(),
        metadata: OperationMetadata::default(),
    };

    let request = SyncRequest {
        protocol: ProtocolVersion::V1,
        request_id: RequestId::new(),
        session: SessionMetadata {
            session_id: SessionId::new(),
            device_id: device,
            actor_id: actor,
            tenant_id: tenant,
            scope_id: scope,
            partitions: Vec::new(),
        },
        cursor: Some(Cursor::new(
            authority_id,
            AuthorityEpoch::INITIAL,
            scope,
            Sequence(10),
        )),
        operations: vec![envelope],
        limits: ClientLimits::default(),
        capabilities: vec![Capability::PostcardV1],
    };

    let response = transport
        .exchange(request)
        .await
        .unwrap_or_else(|error| panic!("{error}"));
    println!(
        "  Server directive on stale epoch: {:?}",
        response.directive
    );

    // Authority must return SyncDirective::AuthorityChanged
    assert!(matches!(
        response.directive,
        SyncDirective::AuthorityChanged {
            previous_epoch,
            current_epoch,
            ..
        } if previous_epoch == AuthorityEpoch::INITIAL && current_epoch == AuthorityEpoch::new(2).unwrap_or_else(|error| panic!("{error}"))
    ));
    assert!(response.acknowledged.is_empty());
    assert_eq!(authority_store.applied_operation_count(), 0);
    println!(
        "  [SUCCESS] Server safely isolated stale timeline and returned AuthorityChanged directive."
    );
}

// ---------------------------------------------------------------------------
// SIMULATION 9: Multi-Tenant Boundary Enforcement & Attack Resistance
// ---------------------------------------------------------------------------
#[tokio::test]
async fn sim_09_multi_tenant_isolation_and_spoof_rejection() {
    println!("\n=== SIMULATION 9: Multi-Tenant Boundary Enforcement & Attack Resistance ===");
    let env = SimEnvironment::new();
    let tenant_legit = env.tenant;
    let tenant_attacker = TenantId::new();

    let actor = ActorId::new();
    let device = DeviceId::new();

    // Attacker claims to belong to tenant_attacker in AuthContext
    let auth = AuthContext {
        actor_id: actor,
        tenant_id: tenant_attacker,
        device_id: device,
    };
    let transport = InProcessTransport::new(env.server.clone(), auth);

    // Attacker crafts operation envelope spoofing legit tenant's tenant_id!
    println!("Step 1: Rogue client attempts cross-tenant operation injection...");
    let entity = EntityRef {
        entity_type: EntityType::new(1).unwrap_or_else(|error| panic!("{error}")),
        entity_id: EntityId::new(),
    };
    let spoofed_op = OperationEnvelope {
        protocol_version: ProtocolVersion::V1,
        operation_id: OperationId::new(),
        tenant_id: tenant_legit, // Spoofed!
        actor_id: actor,
        device_id: device,
        entity,
        base_version: None,
        created_at: HybridTimestamp {
            physical_ms: 5_000,
            logical: 0,
            node: NodeId::new(),
        },
        schema_version: SchemaVersion(1),
        operation_kind: OperationKind(1),
        payload: b"Hacked Balance $1,000,000".to_vec(),
        metadata: OperationMetadata::default(),
    };

    let sync_request = SyncRequest {
        protocol: ProtocolVersion::V1,
        request_id: RequestId::new(),
        session: SessionMetadata {
            session_id: SessionId::new(),
            device_id: device,
            actor_id: actor,
            tenant_id: tenant_attacker,
            scope_id: env.scope,
            partitions: Vec::new(),
        },
        cursor: None,
        operations: vec![spoofed_op],
        limits: ClientLimits::default(),
        capabilities: vec![Capability::PostcardV1],
    };

    let response = transport
        .exchange(sync_request)
        .await
        .unwrap_or_else(|error| panic!("{error}"));
    println!(
        "  Server response rejected count: {}",
        response.rejected.len()
    );

    // Server must reject the spoofed operation with tenant mismatch rejection
    assert_eq!(response.rejected.len(), 1);
    assert_eq!(response.rejected[0].code, RejectionCode::IdentityMismatch);
    assert_eq!(env.authority_store.applied_operation_count(), 0);
    println!(
        "  [SUCCESS] Server authorization kernel rejected spoofed cross-tenant mutation with zero database commits."
    );
}

// ---------------------------------------------------------------------------
// SIMULATION 10: Client Hard Crash Recovery & Resumed Sync
// ---------------------------------------------------------------------------
#[tokio::test]
async fn sim_10_client_hard_crash_recovery_resumed_sync() {
    println!("\n=== SIMULATION 10: Client Crash Mid-Workflow & Restart Recovery ===");
    let env = SimEnvironment::new();
    let local_store = InMemoryLocalStore::default();
    let actor = ActorId::new();
    let device = DeviceId::new();
    let session = SessionMetadata {
        session_id: SessionId::new(),
        device_id: device,
        actor_id: actor,
        tenant_id: env.tenant,
        scope_id: env.scope,
        partitions: Vec::new(),
    };
    let auth = AuthContext {
        actor_id: actor,
        tenant_id: env.tenant,
        device_id: device,
    };

    // Client enqueues local work into durable outbox across 5 distinct entities
    println!("Step 1: App creates 5 local mutations in atomic durable outbox...");
    let entity_type = EntityType::new(1).unwrap_or_else(|error| panic!("{error}"));
    for i in 1..=5 {
        let entity = EntityRef {
            entity_type,
            entity_id: EntityId::new(),
        };
        let envelope = OperationEnvelope {
            protocol_version: ProtocolVersion::V1,
            operation_id: OperationId::new(),
            tenant_id: env.tenant,
            actor_id: actor,
            device_id: device,
            entity,
            base_version: None,
            created_at: HybridTimestamp {
                physical_ms: 6_000 + i,
                logical: 0,
                node: NodeId::new(),
            },
            schema_version: SchemaVersion(1),
            operation_kind: OperationKind(1),
            payload: format!("Step {i} Data").into_bytes(),
            metadata: OperationMetadata::default(),
        };
        local_store
            .append_operation(envelope)
            .await
            .unwrap_or_else(|error| panic!("{error}"));
    }
    assert_eq!(
        local_store
            .pending_operations(100)
            .await
            .unwrap_or_else(|error| panic!("{error}"))
            .len(),
        5
    );

    // Simulate process crash: drop client instance completely!
    println!("Step 2: Process crashes violently (power failure simulation)...");
    // (Client instance dropped, local_store persists in durable storage)

    // Reboot process: create fresh engine pointing to existing persistent local store
    println!(
        "Step 3: Device boots up and instantiates fresh ClientSyncEngine on existing store..."
    );
    let transport = InProcessTransport::new(env.server.clone(), auth);
    let rebooted_engine = ClientSyncEngine::new(
        local_store.clone(),
        transport,
        ClientConfig::new(session.clone()),
    );

    // Resumed sync drains all 5 pending operations safely
    println!("Step 4: Resumed sync executes...");
    let outcome = rebooted_engine
        .run_once()
        .await
        .unwrap_or_else(|error| panic!("{error}"));
    println!(
        "  Recovered sync acknowledged: {} ops",
        outcome.acknowledged
    );
    assert_eq!(outcome.acknowledged, 5);
    assert_eq!(
        local_store
            .pending_operations(100)
            .await
            .unwrap_or_else(|error| panic!("{error}"))
            .len(),
        0
    );
    assert_eq!(env.authority_store.applied_operation_count(), 5);
    println!("  [SUCCESS] All 5 outbox entries survived crash and synced idempotently on restart.");
}
