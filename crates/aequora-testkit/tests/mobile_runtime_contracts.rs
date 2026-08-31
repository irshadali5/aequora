use aequora_conformance::{
    CertificationTier, ConformanceDomain, ConformanceProfile, definitions_for,
};
use aequora_invariants::InvariantId;
use aequora_mobile_bindings::{
    BindingError, BindingErrorCode, MobileBinding, MobileBindingHost, MobileBuildIdentity,
    MobileCommand, MobileOutcome,
};
use aequora_mobile_runtime::{
    AppLifecycle, Completion, CoordinatorError, MobileCoordinator, MobilePlatformEvent,
    MobilePolicy, MobileResourceContext, MobileSyncBudget, NetworkContext, NetworkCost,
    PolicyDecision, PushHint, PushHintReason, RecoveryAction, RecoveryError, RecoverySnapshot,
    StoragePressure, StoreVersions, SyncTrigger, TransferKind, classify_startup,
};
use async_trait::async_trait;

fn ready_context() -> MobileResourceContext {
    MobileResourceContext {
        lifecycle: AppLifecycle::Foreground,
        network: NetworkContext::default(),
        storage: StoragePressure::Healthy,
        secure_keys_available: true,
        ..MobileResourceContext::default()
    }
}

#[test]
fn duplicate_pushes_coalesce_and_never_mutate_durable_progress() {
    let mut coordinator = MobileCoordinator::new(MobilePolicy::default(), ready_context());
    coordinator.request_sync(SyncTrigger::PushHint);
    coordinator.request_sync(SyncTrigger::PushHint);
    assert_eq!(coordinator.pending_trigger_count(), 1);

    let grant = coordinator
        .grant(TransferKind::StructuredSync, None)
        .ok()
        .flatten();
    assert!(grant.is_some());
    assert_eq!(coordinator.pending_trigger_count(), 0);
}

#[test]
fn untrusted_push_payload_is_bounded_before_scheduling() {
    let mut coordinator = MobileCoordinator::new(MobilePolicy::default(), ready_context());
    let result = coordinator.apply_platform_event(MobilePlatformEvent::PushHint(PushHint {
        store_hint: "x".repeat(513),
        scope_hint: None,
        reason: PushHintReason::SyncRequired,
    }));
    assert_eq!(result, Err(CoordinatorError::InvalidPlatformEvent));
    assert_eq!(coordinator.pending_trigger_count(), 0);
}

#[test]
fn competing_mobile_schedulers_are_rejected() {
    let mut coordinator = MobileCoordinator::new(MobilePolicy::default(), ready_context());
    coordinator.request_sync(SyncTrigger::Foreground);
    let grant = coordinator
        .grant(TransferKind::StructuredSync, None)
        .ok()
        .flatten();
    assert!(grant.is_some());
    assert_eq!(
        coordinator.grant(TransferKind::StructuredSync, None),
        Err(CoordinatorError::AlreadyRunning)
    );
}

#[test]
fn background_expiration_requires_checkpoint_and_schedules_resume() {
    let mut context = ready_context();
    context.lifecycle = AppLifecycle::Background;
    let mut coordinator = MobileCoordinator::new(MobilePolicy::default(), context);
    coordinator.request_sync(SyncTrigger::BackgroundTask);
    let grant = coordinator
        .grant(
            TransferKind::StructuredSync,
            Some(MobileSyncBudget::LOW_POWER),
        )
        .ok()
        .flatten();
    assert!(grant.is_some_and(|value| value.budget == MobileSyncBudget::LOW_POWER));
    if let Some(grant) = grant {
        assert_eq!(
            coordinator.complete(grant, Completion::MoreWorkCheckpointed),
            Ok(())
        );
    }
    assert_eq!(coordinator.pending_trigger_count(), 1);
}

#[test]
fn restart_recovers_in_flight_and_interrupted_bootstrap_from_durable_metadata() {
    let actions = classify_startup(
        StoreVersions {
            readable_min: 2,
            writable_current: 4,
            on_disk: 3,
        },
        RecoverySnapshot {
            stale_in_flight_operations: 2,
            pending_outbox_operations: 2,
            interrupted_bootstrap_generation: Some(9),
            durable_cursor: Some(41),
            applied_through: Some(41),
            scheduler_backoff_until_ms: Some(8_000),
            device_binding_generation: 7,
        },
    );
    assert_eq!(
        actions,
        Ok(vec![
            RecoveryAction::RunMigration { from: 3, to: 4 },
            RecoveryAction::RecoverInFlight,
            RecoveryAction::ResumeBootstrap { generation: 9 },
            RecoveryAction::ResumeCoordinator,
        ])
    );
}

#[test]
fn cursor_ahead_of_durable_apply_and_app_downgrade_fail_closed() {
    let corrupt = RecoverySnapshot {
        durable_cursor: Some(12),
        applied_through: Some(11),
        device_binding_generation: 1,
        ..RecoverySnapshot::default()
    };
    assert_eq!(
        classify_startup(
            StoreVersions {
                readable_min: 1,
                writable_current: 2,
                on_disk: 2,
            },
            corrupt,
        ),
        Err(RecoveryError::CursorAheadOfDurability)
    );
    assert_eq!(
        classify_startup(
            StoreVersions {
                readable_min: 1,
                writable_current: 2,
                on_disk: 3,
            },
            RecoverySnapshot {
                device_binding_generation: 1,
                ..RecoverySnapshot::default()
            },
        ),
        Err(RecoveryError::DowngradeRefused)
    );
}

#[test]
fn resource_policy_defers_large_work_but_not_small_required_metadata() {
    let mut context = ready_context();
    context.network.cost = NetworkCost::Expensive;
    let policy = MobilePolicy::default();
    assert_eq!(
        policy.decide(context, TransferKind::Snapshot, None),
        PolicyDecision::DeferForResources
    );
    assert!(matches!(
        policy.decide(context, TransferKind::SecurityMetadata, None),
        PolicyDecision::Run(_)
    ));
}

#[test]
fn storage_pressure_blocks_work_and_pending_intent_pins_scope() {
    let mut context = ready_context();
    context.storage = StoragePressure::Critical;
    assert_eq!(
        MobilePolicy::default().decide(context, TransferKind::StructuredSync, None),
        PolicyDecision::StorageUnavailable
    );
    assert!(!MobilePolicy::may_evict_scope(
        aequora_mobile_runtime::ScopeCachePolicy::OnDemand,
        true,
        false,
    ));
}

struct PanickingHost;

#[async_trait]
impl MobileBindingHost for PanickingHost {
    async fn execute(&self, _command: MobileCommand) -> Result<MobileOutcome, BindingError> {
        panic!("binding panic must be contained")
    }
}

#[tokio::test]
async fn binding_contains_panics_and_exposes_stable_internal_error() {
    let binding = MobileBinding::new(
        PanickingHost,
        MobileBuildIdentity {
            abi_version: aequora_mobile_runtime::AEQUORA_MOBILE_ABI_VERSION,
            core_version: "0.1.0".to_owned(),
            build_id: "test-build".to_owned(),
            registry_generation: 1,
            protocol_versions: vec![1],
        },
    );
    assert!(binding.is_ok());
    if let Ok(binding) = binding {
        assert_eq!(
            binding.call(MobileCommand::QueryStatus).await,
            Err(BindingError::new(BindingErrorCode::Internal))
        );
    }
}

#[test]
fn mobile_full_conformance_selects_all_normative_mobile_tests() {
    let definitions = definitions_for(
        ConformanceProfile::MobileClientFull,
        CertificationTier::FullSync,
    );
    let mobile = definitions
        .iter()
        .filter(|definition| definition.domain == ConformanceDomain::MobileRuntime)
        .collect::<Vec<_>>();
    assert_eq!(mobile.len(), 9);
    for suffix in 1..=9 {
        let id = format!("AEQ-INV-MOBILE{suffix:03}");
        assert!(
            mobile
                .iter()
                .any(|definition| definition.invariant_ids.contains(&id.as_str()))
        );
        assert!(id.parse::<InvariantId>().is_ok());
    }
}
