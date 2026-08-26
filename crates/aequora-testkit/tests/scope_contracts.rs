use std::collections::BTreeSet;

use aequora_client::{ClientConfig, ClientSyncEngineBuilder};
use aequora_protocol::{
    OperationEnvelope, OperationKind, OperationMetadata, SessionMetadata, SyncRequest,
};
use aequora_scope::{
    MembershipRecord, ProjectionVersion, ResolvedScope, ScopeCursor, ScopeDefinitionId,
    ScopeDescriptor, ScopeGeneration, ScopeTransition, ScopeTransitionId, ScopeTransitionKind,
    ScopeVersion, Subscription, SubscriptionId, SubscriptionState,
};
use aequora_testkit::{InMemoryLocalStore, contracts::verify_scope_state_store};
use aequora_transport::{SyncTransport, TransportError};
use aequora_types::{
    ActorId, DeviceId, EntityId, EntityRef, EntityType, HybridTimestamp, NodeId, OperationId,
    ProtocolVersion, SchemaVersion, Sequence, SessionId, SyncScopeId, TenantId,
};

#[derive(Clone, Copy)]
struct UnusedTransport;

#[async_trait::async_trait]
impl SyncTransport for UnusedTransport {
    async fn exchange(
        &self,
        _request: SyncRequest,
    ) -> Result<aequora_protocol::SyncResponse, TransportError> {
        Err(TransportError::permanent(
            "scope transition must not use network transport",
        ))
    }
}

fn fixture()
-> Result<(Subscription, ScopeTransition, OperationEnvelope), Box<dyn std::error::Error>> {
    let tenant = TenantId::new();
    let scope_id = SyncScopeId::new();
    let entity = EntityRef {
        entity_type: EntityType::new(81)?,
        entity_id: EntityId::new(),
    };
    let definition = ScopeDefinitionId::new(1)?;
    let initial = ResolvedScope {
        scope_id,
        version: ScopeVersion::INITIAL,
        generation: ScopeGeneration::INITIAL,
        descriptor: ScopeDescriptor {
            tenant_id: tenant,
            definition,
            partitions: BTreeSet::new(),
            policy_version: 1,
            projection: ProjectionVersion(1),
        },
    };
    let subscription = Subscription {
        subscription_id: SubscriptionId::new(),
        scope: initial.clone(),
        state: SubscriptionState::Resolved,
        cursor: None,
        pending_transition: None,
    };
    let next_version = initial.version.next()?;
    let mut target = initial.clone();
    target.version = next_version;
    let transition = ScopeTransition {
        transition_id: ScopeTransitionId::new(),
        subscription_id: subscription.subscription_id,
        from_version: initial.version,
        from_generation: initial.generation,
        target: Some(target),
        boundary: Some(ScopeCursor {
            scope_id,
            version: next_version,
            generation: initial.generation,
            sequence: Sequence(12),
        }),
        kind: ScopeTransitionKind::FullBootstrap,
        additions: vec![MembershipRecord {
            scope_id,
            projection: ProjectionVersion(1),
            entity,
            membership_version: next_version,
        }],
        removals: Vec::new(),
        affected_pending_operations: Vec::new(),
        staging_complete: true,
    };
    let operation = OperationEnvelope {
        protocol_version: ProtocolVersion::V1,
        operation_id: OperationId::new(),
        tenant_id: tenant,
        actor_id: ActorId::new(),
        device_id: DeviceId::new(),
        entity,
        base_version: None,
        created_at: HybridTimestamp {
            physical_ms: 1,
            logical: 0,
            node: NodeId::new(),
        },
        schema_version: SchemaVersion(1),
        operation_kind: OperationKind(1),
        payload: b"scope contract".to_vec(),
        metadata: OperationMetadata::default(),
    };
    Ok((subscription, transition, operation))
}

#[tokio::test]
async fn reference_store_passes_scope_state_contract() -> Result<(), Box<dyn std::error::Error>> {
    let (subscription, transition, operation) = fixture()?;
    verify_scope_state_store(
        &InMemoryLocalStore::default(),
        subscription,
        transition,
        operation,
    )
    .await?;
    Ok(())
}

#[tokio::test]
async fn client_applies_scope_activation_without_a_network_exchange()
-> Result<(), Box<dyn std::error::Error>> {
    let (subscription, mut transition, _operation) = fixture()?;
    let session = SessionMetadata {
        session_id: SessionId::new(),
        device_id: DeviceId::new(),
        actor_id: ActorId::new(),
        tenant_id: subscription.scope.descriptor.tenant_id,
        scope_id: subscription.scope.scope_id,
        partitions: Vec::new(),
    };
    let client = ClientSyncEngineBuilder::new()
        .store(InMemoryLocalStore::default())
        .transport(UnusedTransport)
        .config(ClientConfig::new(session))
        .build()?;
    client.install_subscription(&subscription).await?;
    transition.staging_complete = false;
    assert!(!client.apply_scope_transition(&transition).await?.applied);
    transition.staging_complete = true;
    assert!(client.apply_scope_transition(&transition).await?.applied);
    Ok(())
}
