use aequora_client::ClientReadSession;
use aequora_invariants::InvariantId;
use aequora_region::{
    AssignmentTrust, AuthorityLocation, AuthorityShardId, DirectoryGeneration, EndpointReadPolicy,
    ReadConsistency, ReadFallbackPolicy, ReadTarget, RegionError, RegionHealth, RegionMode,
    RegionalCopyId, RegionalCopyKind, RegionalCopyRecord, RegionalGovernanceRegistry,
    RegionalPurgeState, RegionalReadRequest, RegionalRole, RegionalRouter, RegionalRouterConfig,
    ReplicaId, ReplicaObservation, ReplicaWatermark, ResidencyPolicy, SessionWatermark,
    StalenessBudget, TenantAuthorityAssignment, TenantAuthorityDirectory,
};
use aequora_server::ServerRegionalReadGuard;
use aequora_types::{AuthorityEpoch, AuthorityId, RegionId, Sequence, TenantId};
use std::{collections::BTreeSet, time::Duration};

fn location(region: u16, epoch: AuthorityEpoch, sequence: u64) -> AuthorityLocation {
    AuthorityLocation {
        region_id: RegionId::new(region),
        endpoint: "https://authority.internal".to_owned(),
        authority_id: AuthorityId::LOCAL_DEVELOPMENT,
        authority_epoch: epoch,
        shard_id: Some(AuthorityShardId(1)),
        latest_sequence: Sequence(sequence),
    }
}

fn replica(region: u16, epoch: AuthorityEpoch, sequence: u64) -> ReplicaObservation {
    ReplicaObservation {
        replica_id: ReplicaId::new(),
        region_id: RegionId::new(region),
        role: RegionalRole::ReadReplica,
        watermark: ReplicaWatermark {
            authority_id: AuthorityId::LOCAL_DEVELOPMENT,
            authority_epoch: epoch,
            sequence: Sequence(sequence),
        },
        projection: None,
        authorization_sequence: Some(Sequence(sequence)),
        observed_authority_sequence: Sequence(20),
        approximate_time_lag: Duration::from_millis(25),
        health: RegionHealth::Healthy,
        mode: RegionMode::Healthy,
        priority: 1,
        load_basis_points: 500,
        maintenance: false,
    }
}

fn request(consistency: ReadConsistency) -> RegionalReadRequest {
    RegionalReadRequest {
        authority_id: AuthorityId::LOCAL_DEVELOPMENT,
        authority_epoch: AuthorityEpoch::INITIAL,
        policy: EndpointReadPolicy {
            consistency,
            staleness_budget: StalenessBudget::None,
            fallback: ReadFallbackPolicy::NeverDowngrade,
            allow_authority_fallback: true,
            authorization_sensitive: false,
        },
        session: None,
        region_hint: Some(RegionId::new(2)),
        required_projection: None,
        minimum_authorization_sequence: None,
    }
}

fn router(authority_available: bool) -> Result<RegionalRouter, RegionError> {
    RegionalRouter::new(
        location(1, AuthorityEpoch::INITIAL, 20),
        authority_available,
        RegionalRouterConfig {
            local_region: RegionId::new(2),
            replica_wait: Duration::from_millis(200),
            allow_authority_fallback: true,
        },
    )
}

#[test]
fn read_your_writes_is_carried_from_client_to_replica_guard_and_router() -> Result<(), RegionError>
{
    let mut session = ClientReadSession::default();
    session.observe_commit(
        AuthorityId::LOCAL_DEVELOPMENT,
        AuthorityEpoch::INITIAL,
        Sequence(20),
    )?;
    let mut read = request(ReadConsistency::Session);
    read.session = session.watermark();

    let stale = replica(2, AuthorityEpoch::INITIAL, 18);
    let guard = ServerRegionalReadGuard::new(stale.clone());
    assert_eq!(
        guard.authorize(read),
        Err(RegionError::ReplicaTooStale {
            available: Sequence(18),
            required: Sequence(20),
        })
    );

    let mut router = router(true)?;
    router.observe(stale)?;
    let decision = router.route(read)?;
    assert_eq!(decision.target, ReadTarget::Authority);
    assert!(!decision.downgraded);
    assert!(decision.wait_before_fallback.is_some());
    Ok(())
}

#[test]
fn region_partition_keeps_eventual_reads_but_not_strong_or_writes() -> Result<(), RegionError> {
    let mut stale = replica(2, AuthorityEpoch::INITIAL, 18);
    stale.mode = RegionMode::Disconnected;
    let replica_id = stale.replica_id;
    let mut router = router(false)?;
    router.observe(stale)?;

    assert_eq!(
        router.route(request(ReadConsistency::Eventual))?.target,
        ReadTarget::RegionalReplica(replica_id)
    );
    assert!(matches!(
        router.route(request(ReadConsistency::AtLeast(Sequence(20)))),
        Err(RegionError::ReplicaTooStale { .. })
    ));

    let tenant = TenantId::new();
    let mut directory = TenantAuthorityDirectory::default();
    directory.install(
        TenantAuthorityAssignment {
            tenant_id: tenant,
            shard_id: AuthorityShardId(1),
            authority_id: AuthorityId::LOCAL_DEVELOPMENT,
            authority_epoch: AuthorityEpoch::INITIAL,
            authority_region: RegionId::new(1),
            directory_generation: DirectoryGeneration(1),
            residency: ResidencyPolicy {
                allowed_regions: BTreeSet::from([RegionId::new(1), RegionId::new(2)]),
                authority_region: RegionId::new(1),
                replica_regions: BTreeSet::from([RegionId::new(2)]),
                key_regions: BTreeSet::from([RegionId::new(1)]),
            },
        },
        AssignmentTrust::Verified,
    )?;
    assert_eq!(
        directory.route_write(
            tenant,
            &location(2, AuthorityEpoch::INITIAL, 18),
            RegionalRole::ReadReplica,
        ),
        Err(RegionError::NotAuthorityWriter)
    );
    Ok(())
}

#[test]
fn failover_same_epoch_changes_writer_location_without_client_epoch_transition()
-> Result<(), RegionError> {
    let mut router = router(true)?;
    router.observe(replica(2, AuthorityEpoch::INITIAL, 20))?;
    router.update_authority(location(2, AuthorityEpoch::INITIAL, 20), true)?;

    let read = request(ReadConsistency::AtLeast(Sequence(20)));
    assert!(matches!(
        router.route(read)?.target,
        ReadTarget::RegionalReplica(_)
    ));
    let session = SessionWatermark::new(
        AuthorityId::LOCAL_DEVELOPMENT,
        AuthorityEpoch::INITIAL,
        Sequence(20),
    );
    assert_eq!(session.authority_epoch, AuthorityEpoch::INITIAL);
    Ok(())
}

#[test]
fn routing_is_deterministic_under_a_large_replica_observation_set() -> Result<(), RegionError> {
    let mut router = router(true)?;
    let mut preferred = None;
    for value in 2_u16..=513 {
        let mut observation = replica(value, AuthorityEpoch::INITIAL, 20);
        observation.priority = value % 17;
        observation.load_basis_points = value.saturating_mul(7).min(10_000);
        if value == 2 {
            preferred = Some(observation.replica_id);
        }
        router.observe(observation)?;
    }
    let request = request(ReadConsistency::AtLeast(Sequence(20)));
    let first = router.route(request)?.target;
    assert_eq!(
        first,
        ReadTarget::RegionalReplica(preferred.unwrap_or_default())
    );
    for _ in 0..128 {
        assert_eq!(router.route(request)?.target, first);
    }
    Ok(())
}

#[test]
fn governance_completion_waits_for_each_required_regional_copy() -> Result<(), RegionError> {
    let projection = RegionalCopyId::new();
    let cache = RegionalCopyId::new();
    let mut registry = RegionalGovernanceRegistry::default();
    for (copy_id, kind) in [
        (projection, RegionalCopyKind::Projection),
        (cache, RegionalCopyKind::Cache),
    ] {
        registry.register(RegionalCopyRecord {
            copy_id,
            region_id: RegionId::new(2),
            kind,
            required: true,
            state: RegionalPurgeState::Registered,
        });
    }
    registry.transition(projection, RegionalPurgeState::Purged)?;
    registry.transition(projection, RegionalPurgeState::Verified)?;
    assert!(!registry.report().complete);
    registry.transition(cache, RegionalPurgeState::Purged)?;
    registry.transition(cache, RegionalPurgeState::Verified)?;
    assert!(registry.report().complete);
    Ok(())
}

#[test]
fn all_nine_regional_invariants_are_registered() {
    let identifiers = [
        InvariantId::RegionSingleWriter,
        InvariantId::RegionAtLeastWatermark,
        InvariantId::RegionSessionMonotonicity,
        InvariantId::RegionEpochIsolation,
        InvariantId::RegionFailureAuthoritySafety,
        InvariantId::RegionFallbackSafety,
        InvariantId::RegionArtifactIntegrity,
        InvariantId::RegionResidencyCoverage,
        InvariantId::RegionGovernanceCoverage,
    ];
    for (index, invariant) in identifiers.into_iter().enumerate() {
        assert_eq!(invariant.as_str(), format!("AEQ-INV-REG{:03}", index + 1));
        assert!(!invariant.entry().diagnostic.is_empty());
    }
}
