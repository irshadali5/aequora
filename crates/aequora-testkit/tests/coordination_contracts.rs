use aequora_client::{
    ClientConfig, ClientSyncEngineBuilder, MultiProcessCoordinatorConfig, SyncCoordinator,
    SyncCoordinatorConfig, SyncTrigger,
};
use aequora_protocol::{SessionMetadata, SyncRequest};
use aequora_testkit::{InMemoryLocalStore, contracts::verify_local_coordination};
use aequora_transport::{SyncTransport, TransportError};
use aequora_types::{ActorId, DeviceId, SessionId, SyncScopeId, TenantId};
use async_trait::async_trait;
use std::{sync::Arc, time::Duration};

#[derive(Clone, Copy)]
struct OfflineTransport;

#[async_trait]
impl SyncTransport for OfflineTransport {
    async fn exchange(
        &self,
        _request: SyncRequest,
    ) -> Result<aequora_protocol::SyncResponse, TransportError> {
        Err(TransportError::transient("offline coordination fixture"))
    }
}

fn session() -> SessionMetadata {
    SessionMetadata {
        session_id: SessionId::new(),
        device_id: DeviceId::new(),
        actor_id: ActorId::new(),
        tenant_id: TenantId::new(),
        scope_id: SyncScopeId::new(),
        partitions: Vec::new(),
    }
}

#[tokio::test]
async fn reference_store_passes_local_coordination_contract() {
    let report = verify_local_coordination(&InMemoryLocalStore::default())
        .await
        .unwrap_or_else(|error| panic!("{error}"));
    assert!(report.fencing_token.0 >= 3);
    assert!(report.store_generation.0 > 1);
}

#[tokio::test]
async fn two_runtime_coordinators_elect_exactly_one_leader() {
    let store = InMemoryLocalStore::default();
    let coordinator_config = SyncCoordinatorConfig {
        periodic_interval: None,
        ..SyncCoordinatorConfig::default()
    };
    let election = MultiProcessCoordinatorConfig {
        lease_ttl: Duration::from_millis(300),
        heartbeat_interval: Duration::from_millis(50),
        ..MultiProcessCoordinatorConfig::default()
    };
    let first = ClientSyncEngineBuilder::new()
        .store(store.clone())
        .transport(OfflineTransport)
        .config(ClientConfig::new(session()))
        .build()
        .unwrap_or_else(|error| panic!("{error}"));
    let second = ClientSyncEngineBuilder::new()
        .store(store)
        .transport(OfflineTransport)
        .config(ClientConfig::new(session()))
        .build()
        .unwrap_or_else(|error| panic!("{error}"));
    let (first, first_handle) = SyncCoordinator::new(Arc::new(first), coordinator_config);
    let (second, second_handle) = SyncCoordinator::new(Arc::new(second), coordinator_config);
    let first_task = tokio::spawn(first.run_multi_process(election));
    let second_task = tokio::spawn(second.run_multi_process(MultiProcessCoordinatorConfig {
        process_id: aequora_coordination::ProcessInstanceId::new(),
        ..election
    }));
    tokio::time::sleep(Duration::from_millis(100)).await;
    let statuses = [
        first_handle.coordinator_status(),
        second_handle.coordinator_status(),
    ];
    assert_eq!(
        statuses
            .iter()
            .filter(|status| matches!(status, aequora_client::CoordinatorStatus::Leader { .. }))
            .count(),
        1
    );
    first_handle
        .trigger(SyncTrigger::Shutdown)
        .await
        .unwrap_or_else(|error| panic!("{error}"));
    second_handle
        .trigger(SyncTrigger::Shutdown)
        .await
        .unwrap_or_else(|error| panic!("{error}"));
    first_task
        .await
        .unwrap_or_else(|error| panic!("{error}"))
        .unwrap_or_else(|error| panic!("{error}"));
    second_task
        .await
        .unwrap_or_else(|error| panic!("{error}"))
        .unwrap_or_else(|error| panic!("{error}"));
}
