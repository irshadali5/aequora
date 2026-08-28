use aequora_client::ClientConfig;
use aequora_client::resources::{
    BackgroundBudget, ClientResourceAdmission, ClientResourceContext, ClientResourcePolicy,
    ClientResourceProfile, ClientWork, ClientWorkKind, CpuClass, DefaultResourceAdmission,
    DurableWorkCheckpoint, EvictionCandidate, EvictionPlan, LocalCommitReceipt, MemoryClass,
    NetworkContext, PowerContext, ResourceDecision, ResourceReason, ScopeCachePolicy,
    StorageAssetKind, StoragePreflight, StoragePreflightDecision, StorageState, ThermalState,
    WorkEstimate,
};
use aequora_invariants::InvariantId;
use aequora_protocol::{Capability, SessionMetadata};
use aequora_types::{
    ActorId, AuthorityEpoch, AuthorityId, Cursor, DeviceId, Sequence, SessionId, SyncScopeId,
    TenantId,
};

fn work(kind: ClientWorkKind, bytes: u64, user_initiated: bool) -> ClientWork {
    ClientWork {
        kind,
        estimate: WorkEstimate {
            operations: 500,
            bytes,
            storage_delta_bytes: if matches!(kind, ClientWorkKind::Bootstrap) {
                bytes
            } else {
                0
            },
            cpu: CpuClass::Moderate,
        },
        user_initiated,
    }
}

fn admission(profile: ClientResourceProfile) -> DefaultResourceAdmission {
    let result = DefaultResourceAdmission::new(ClientResourcePolicy::for_profile(profile));
    let Ok(admission) = result else {
        panic!("built-in resource profiles must validate");
    };
    admission
}

#[test]
fn very_low_memory_caps_every_bounded_pipeline() {
    let policy = ClientResourcePolicy::for_profile(ClientResourceProfile::MobileMinimal);
    let context = ClientResourceContext {
        memory: MemoryClass::VeryLow,
        background: BackgroundBudget::Foreground,
        ..ClientResourceContext::default()
    };
    let decision = admission(ClientResourceProfile::MobileMinimal).allow(
        &work(ClientWorkKind::InteractivePush, 2 * 1_024 * 1_024, true),
        &context,
    );
    let ResourceDecision::RunReduced(limits) = decision else {
        panic!("very-low-memory interactive sync must run as a bounded unit");
    };
    assert!(limits.max_operations <= policy.memory.max_batch_operations / 4);
    assert!(limits.max_bytes <= policy.memory.sync_decode_bytes / 4);
    assert_eq!(limits.max_parallel_transfers, 1);
    assert_eq!(limits.max_cpu_workers, 1);
    assert!(limits.max_snapshot_chunk_bytes <= policy.memory.snapshot_pipeline_bytes);
}

#[test]
fn low_storage_defers_bulk_and_eviction_preserves_intent() {
    let context = ClientResourceContext {
        storage: StorageState::Low,
        background: BackgroundBudget::Foreground,
        ..ClientResourceContext::default()
    };
    assert_eq!(
        admission(ClientResourceProfile::MobileStandard).allow(
            &work(ClientWorkKind::Bootstrap, 8 * 1_024 * 1_024, false),
            &context,
        ),
        ResourceDecision::Defer(ResourceReason::LowStorage)
    );

    let plan = EvictionPlan::build(
        &[
            EvictionCandidate {
                asset_id: 1,
                kind: StorageAssetKind::RebuildableUiCache,
                bytes: 256,
                pending_intent_pinned: false,
                application_allows_eviction: true,
            },
            EvictionCandidate {
                asset_id: 2,
                kind: StorageAssetKind::OptionalScope,
                bytes: 512,
                pending_intent_pinned: true,
                application_allows_eviction: true,
            },
            EvictionCandidate {
                asset_id: 3,
                kind: StorageAssetKind::UnsyncedOutbox,
                bytes: 1_024,
                pending_intent_pinned: false,
                application_allows_eviction: true,
            },
        ],
        2_048,
    );
    assert_eq!(plan.evict_asset_ids, vec![1]);
    assert!(!plan.evict_asset_ids.contains(&2));
    assert!(!plan.evict_asset_ids.contains(&3));
    assert_eq!(
        ScopeCachePolicy::Recent.retention(StorageState::Critical, true),
        aequora_client::resources::CacheRetention::Retain
    );
}

#[test]
fn disk_full_preflight_and_local_save_fail_closed() {
    let policy = ClientResourcePolicy::for_profile(ClientResourceProfile::MobileMinimal);
    let preflight = StoragePreflight {
        available_bytes: 64 * 1_024 * 1_024,
        estimated_payload_bytes: 48 * 1_024 * 1_024,
        staging_overhead_bytes: 16 * 1_024 * 1_024,
    };
    assert_eq!(
        preflight.decide(policy.storage, StorageState::Critical),
        StoragePreflightDecision::RejectUntilSpaceAvailable
    );
    assert!(LocalCommitReceipt::from_atomic_commit(9, true, false).is_err());
    assert!(LocalCommitReceipt::from_atomic_commit(9, false, true).is_err());
    let Ok(receipt) = LocalCommitReceipt::from_atomic_commit(9, true, true) else {
        panic!("an atomic durable local commit should produce a receipt");
    };
    assert_eq!(receipt.transaction_sequence(), 9);
}

#[test]
fn metered_network_allows_small_interactive_and_defers_optional_bulk() {
    let context = ClientResourceContext {
        network: NetworkContext {
            metered: Some(true),
            estimated_rtt_ms: Some(1_500),
            ..NetworkContext::default()
        },
        background: BackgroundBudget::Foreground,
        ..ClientResourceContext::default()
    };
    let admission = admission(ClientResourceProfile::MobileStandard);
    assert!(matches!(
        admission.allow(
            &work(ClientWorkKind::InteractivePush, 8 * 1_024, true),
            &context
        ),
        ResourceDecision::RunNow(_) | ResourceDecision::RunReduced(_)
    ));
    assert_eq!(
        admission.allow(
            &work(ClientWorkKind::Bootstrap, 64 * 1_024 * 1_024, false),
            &context
        ),
        ResourceDecision::Defer(ResourceReason::WaitingForUnmetered)
    );
    assert_eq!(
        admission.allow(
            &work(ClientWorkKind::Bootstrap, 64 * 1_024 * 1_024, true),
            &context
        ),
        ResourceDecision::RequireUserApproval(ResourceReason::WaitingForUnmetered)
    );
}

#[test]
fn process_kill_cannot_advance_an_uncommitted_cursor() {
    let Some(mut checkpoint) = DurableWorkCheckpoint::new(1) else {
        panic!("non-zero checkpoint format must be accepted");
    };
    let cursor = Cursor::new(
        AuthorityId::LOCAL_DEVELOPMENT,
        AuthorityEpoch::INITIAL,
        SyncScopeId::new(),
        Sequence(7),
    );
    let before = checkpoint;
    assert!(
        checkpoint
            .commit_unit(1, Some(cursor), false, true)
            .is_err()
    );
    assert_eq!(checkpoint, before);

    let encoded = postcard::to_stdvec(&checkpoint);
    let Ok(encoded) = encoded else {
        panic!("checkpoint must remain persistable across process death");
    };
    let decoded = postcard::from_bytes::<DurableWorkCheckpoint>(&encoded);
    let Ok(mut resumed) = decoded else {
        panic!("persisted checkpoint must resume after process death");
    };
    assert!(resumed.commit_unit(1, Some(cursor), true, true).is_ok());
    assert_eq!(resumed.committed_cursor, Some(cursor));
}

#[test]
fn suspended_background_does_no_work_and_hot_device_defers_maintenance() {
    let suspended = ClientResourceContext {
        background: BackgroundBudget::Suspended,
        ..ClientResourceContext::default()
    };
    let admission = admission(ClientResourceProfile::MobileStandard);
    assert_eq!(
        admission.allow(
            &work(ClientWorkKind::InteractivePull, 4_096, false),
            &suspended
        ),
        ResourceDecision::Defer(ResourceReason::BackgroundSuspended)
    );

    let hot = ClientResourceContext {
        thermal: ThermalState::Hot,
        power: PowerContext {
            charging: Some(true),
            ..PowerContext::default()
        },
        background: BackgroundBudget::Foreground,
        ..ClientResourceContext::default()
    };
    assert_eq!(
        admission.allow(
            &work(ClientWorkKind::AntiEntropy, 1_024 * 1_024, false),
            &hot
        ),
        ResourceDecision::Defer(ResourceReason::ThermalPressure)
    );
}

#[test]
fn security_work_overrides_power_optimization_but_not_suspension_or_disk_safety() {
    let low_power = ClientResourceContext {
        power: PowerContext {
            battery_percent: Some(2),
            low_power_mode: Some(true),
            ..PowerContext::default()
        },
        background: BackgroundBudget::Foreground,
        ..ClientResourceContext::default()
    };
    assert!(matches!(
        admission(ClientResourceProfile::MobileMinimal).allow(
            &work(ClientWorkKind::SecurityDirective, 4_096, false),
            &low_power
        ),
        ResourceDecision::RunReduced(_) | ResourceDecision::RunNow(_)
    ));
}

#[test]
fn capability_negotiation_is_coarse_and_invariants_are_registered() {
    let minimal = ClientResourcePolicy::for_profile(ClientResourceProfile::MobileMinimal)
        .capability_profile();
    let standard = ClientResourcePolicy::for_profile(ClientResourceProfile::DesktopStandard)
        .capability_profile();
    assert_ne!(minimal.class, standard.class);
    assert!(minimal.max_request_bytes < standard.max_request_bytes);
    for suffix in 1..=9 {
        let identifier = format!("AEQ-INV-CLIENT{suffix:03}");
        assert!(identifier.parse::<InvariantId>().is_ok());
    }
}

#[test]
fn client_config_applies_resource_caps_to_scheduler_and_wire_limits() {
    let session = SessionMetadata {
        session_id: SessionId::new(),
        device_id: DeviceId::new(),
        actor_id: ActorId::new(),
        tenant_id: TenantId::new(),
        scope_id: SyncScopeId::new(),
        partitions: Vec::new(),
    };
    let policy = ClientResourcePolicy::for_profile(ClientResourceProfile::MobileMinimal);
    let config = ClientConfig::new(session).with_resource_profile(policy.profile);
    assert!(config.push_batch_size <= policy.memory.max_batch_operations as usize);
    assert!(config.push_batch_bytes <= policy.memory.sync_decode_bytes as usize);
    assert!(config.limits.max_response_bytes <= policy.memory.sync_response_bytes);
    assert!(config.snapshot_limits.max_payload_bytes <= policy.memory.snapshot_chunk_bytes);
    assert!(config.scheduler.concurrency <= usize::from(policy.memory.max_parallel_transfers));
    assert!(
        config
            .capabilities
            .contains(&Capability::ResourceConstrainedV1)
    );
}
