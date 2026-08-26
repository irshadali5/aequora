use aequora_governance::{
    GovernanceError, GovernanceJobState, PurgeId, PurgeVerificationReport, StorageSurfaceId,
    StoreVerification, SurfaceOutcome, TenantLifecycle,
};
use std::collections::BTreeSet;

#[test]
fn offboarding_revokes_writes_and_partial_surface_failure_cannot_complete()
-> Result<(), GovernanceError> {
    let state = TenantLifecycle::Active.transition(TenantLifecycle::ReadOnly)?;
    assert!(!state.allows_authoritative_writes());
    let state = state
        .transition(TenantLifecycle::DeletionScheduled)?
        .transition(TenantLifecycle::Purging)?;
    assert!(!state.allows_authoritative_writes());

    let primary = StorageSurfaceId::new(1)?;
    let objects = StorageSurfaceId::new(2)?;
    let required = BTreeSet::from([primary, objects]);
    let report = PurgeVerificationReport {
        purge_id: PurgeId::new(),
        completed_at_unix_ms: 10,
        stores: vec![
            StoreVerification {
                surface: primary,
                outcome: SurfaceOutcome::Verified,
                remaining_items: 0,
                digest: [1; 32],
            },
            StoreVerification {
                surface: objects,
                outcome: SurfaceOutcome::Unreachable,
                remaining_items: 1,
                digest: [0; 32],
            },
        ],
        retained_exceptions: 0,
    };
    assert_eq!(
        report.state(&required)?,
        GovernanceJobState::PartiallyCompleted
    );
    Ok(())
}
