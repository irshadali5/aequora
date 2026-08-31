use aequora_agent::{AgentError, AgentSession, CommandRoute};
use aequora_conformance::{
    CertificationTier, ConformanceDomain, ConformanceProfile, definitions_for,
};
use aequora_coordination::{
    CoordinationSnapshot, FencingToken, LeaseGrant, LeaseKind, LocalStoreGeneration, LocalStoreId,
    ProcessInstanceId,
};
use aequora_desktop_runtime::{
    CoordinatorAction, DesktopCoordinator, DesktopError, DesktopLifecycle, DesktopPolicy,
    DesktopSession, NetworkAvailability, NetworkContext, PowerContext, ResumeAction,
    StoragePressure, WorkClass, WorkDecision,
};
use aequora_invariants::InvariantId;
use aequora_ipc_protocol::{CURRENT_IPC_PROTOCOL_VERSION, Command, Handshake, SessionToken};
use aequora_platform_linux::{LinuxError, LinuxSecureStore};
use aequora_update::{UpdateError, UpdateStage, UpdateState};
use uuid::Uuid;

fn lease(token: u64) -> LeaseGrant {
    LeaseGrant {
        store_id: LocalStoreId::from_uuid(Uuid::nil()),
        owner_id: ProcessInstanceId::from_uuid(Uuid::nil()),
        fencing_token: FencingToken(token),
        store_generation: LocalStoreGeneration::INITIAL,
        kind: LeaseKind::SyncCoordinator,
        expires_at_unix_ms: 100,
    }
}

#[test]
fn revived_stale_process_is_fenced_from_coordinator_metadata() {
    let old = DesktopCoordinator::new(lease(1));
    let snapshot = CoordinationSnapshot {
        store_id: lease(2).store_id,
        store_generation: LocalStoreGeneration::INITIAL,
        fencing_token: FencingToken(2),
        owner_id: Some(lease(2).owner_id),
        kind: LeaseKind::SyncCoordinator,
        expires_at_unix_ms: 100,
    };
    assert_eq!(
        old.authorize(snapshot, 50, CoordinatorAction::MigrateStore),
        Err(DesktopError::StaleCoordinator)
    );
}

#[test]
fn resume_forces_credential_authority_and_network_refresh() {
    let mut session = DesktopSession::new(DesktopPolicy::default());
    assert_eq!(
        session.transition(DesktopLifecycle::Sleeping),
        Ok(ResumeAction::None)
    );
    assert_eq!(
        session.transition(DesktopLifecycle::Active),
        Ok(ResumeAction::RefreshCredentialsAndAuthority)
    );
    assert_eq!(session.network_generation(), 1);
}

#[test]
fn low_disk_never_reports_interactive_persistence_success() {
    let decision = DesktopPolicy::default().decide(
        WorkClass::Interactive,
        NetworkContext {
            availability: NetworkAvailability::Available,
            ..NetworkContext::default()
        },
        PowerContext::default(),
        StoragePressure::Critical,
    );
    assert_eq!(decision, WorkDecision::RejectNotDurable);
}

#[test]
fn ipc_requires_current_user_token_and_routes_mutation_to_domain_service() {
    let token = SessionToken::new(vec![7; 32]).unwrap_or_else(|_| unreachable!());
    let session = AgentSession::new(token, 3);
    let handshake = Handshake {
        protocol_version: CURRENT_IPC_PROTOCOL_VERSION,
        build_id: "desktop-ui-1".into(),
        registry_generation: 3,
        session_token: SessionToken::new(vec![7; 32]).unwrap_or_else(|_| unreachable!()),
    };
    assert_eq!(
        session.authenticate(&handshake, false),
        Err(AgentError::Unauthenticated)
    );
    assert_eq!(
        session.authenticate(&handshake, true),
        Ok(CURRENT_IPC_PROTOCOL_VERSION)
    );
    assert_eq!(
        session.authorize_command(&Command::Mutate { operation: vec![1] }),
        Ok(CommandRoute::DomainService)
    );
}

#[test]
fn secure_store_and_update_fail_closed() {
    assert_eq!(
        LinuxSecureStore::Unavailable.require_secure(),
        Err(LinuxError::SecureStoreUnavailable)
    );
    let installed = UpdateState {
        stage: UpdateStage::Installed,
        pending_operations: 4,
        store_version: 2,
    };
    assert_eq!(
        installed.advance(UpdateStage::Migrated, 3),
        Err(UpdateError::PendingIntentLost)
    );
}

#[test]
fn desktop_profiles_cover_all_desktop_invariants() {
    for profile in [
        ConformanceProfile::DesktopClientFull,
        ConformanceProfile::DesktopAgentFull,
    ] {
        let tests = definitions_for(profile, CertificationTier::FullSync);
        for suffix in 1..=9 {
            let id = format!("AEQ-INV-DESKTOP{suffix:03}");
            assert!(tests.iter().any(|definition| definition.domain
                == ConformanceDomain::DesktopRuntime
                && definition.invariant_ids.contains(&id.as_str())));
        }
    }
    let registered = InvariantId::ALL
        .iter()
        .map(|id| id.as_str())
        .filter(|id| id.starts_with("AEQ-INV-DESKTOP"))
        .count();
    assert_eq!(registered, 9);
}
