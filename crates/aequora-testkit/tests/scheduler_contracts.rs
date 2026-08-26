use std::{sync::Arc, time::Duration};

use aequora_client::{ClientConfig, ClientError, ClientSyncEngineBuilder};
use aequora_protocol::{SessionMetadata, SyncRequest};
use aequora_scheduler::{
    BatchController, DeferralReason, SchedulerPolicy, SchedulingContext, SyncProfile,
};
use aequora_testkit::InMemoryLocalStore;
use aequora_transport::{SyncTransport, TransportError};
use aequora_types::{ActorId, DeviceId, SessionId, SyncScopeId, TenantId};
use async_trait::async_trait;
use std::sync::atomic::{AtomicUsize, Ordering};

#[derive(Clone)]
struct CountingOfflineTransport(Arc<AtomicUsize>);

#[async_trait]
impl SyncTransport for CountingOfflineTransport {
    async fn exchange(
        &self,
        _request: SyncRequest,
    ) -> Result<aequora_protocol::SyncResponse, TransportError> {
        self.0.fetch_add(1, Ordering::Relaxed);
        Err(TransportError::transient("offline scheduler fixture"))
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
async fn pause_defers_network_without_touching_durable_store() {
    let exchanges = Arc::new(AtomicUsize::new(0));
    let store = InMemoryLocalStore::default();
    let client = ClientSyncEngineBuilder::new()
        .store(store.clone())
        .transport(CountingOfflineTransport(Arc::clone(&exchanges)))
        .config(ClientConfig::new(session()))
        .build()
        .unwrap_or_else(|error| panic!("{error}"));
    client.pause();
    assert!(matches!(
        client.run_once().await,
        Err(ClientError::SchedulerDeferred {
            reason: DeferralReason::Paused
        })
    ));
    assert_eq!(exchanges.load(Ordering::Relaxed), 0);
    assert_eq!(store.pending_count(), 0);
    client.resume();
}

#[test]
fn scheduler_state_round_trips_and_profiles_remain_hard_bounded() {
    for profile in [
        SyncProfile::Desktop,
        SyncProfile::Mobile,
        SyncProfile::HighLatency,
        SyncProfile::LowBandwidth,
        SyncProfile::EnterpriseLan,
    ] {
        let policy = SchedulerPolicy::for_profile(profile);
        policy.validate().unwrap_or_else(|error| panic!("{error}"));
        let mut controller =
            BatchController::new(policy.batch).unwrap_or_else(|error| panic!("{error}"));
        for _ in 0..32 {
            controller.record_overload();
        }
        assert!(controller.target_ops >= policy.batch.min_ops);
        assert!(controller.target_bytes >= policy.batch.min_bytes);
    }

    let context = SchedulingContext {
        remaining_execution_ms: Some(
            u64::try_from(Duration::from_secs(5).as_millis()).unwrap_or(u64::MAX),
        ),
        ..SchedulingContext::default()
    };
    let encoded = postcard::to_stdvec(&context).unwrap_or_else(|error| panic!("{error}"));
    let decoded: SchedulingContext =
        postcard::from_bytes(&encoded).unwrap_or_else(|error| panic!("{error}"));
    assert_eq!(decoded, context);
}
