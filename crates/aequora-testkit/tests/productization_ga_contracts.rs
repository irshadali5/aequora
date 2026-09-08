use aequora_conformance::{CertificationTier, ConformanceProfile, definitions_for};
use aequora_invariants::InvariantId;
use aequora_release::productization::{
    DecisionCheck, GaGate, GaReadinessManifest, Maturity, Milestone, V1Component, V1Scope,
};

#[test]
fn frozen_v1_scope_is_small_and_keeps_stoolap_conditional() {
    let scope = V1Scope::from_ron(include_str!("../../../release/v1-scope.ron"))
        .unwrap_or_else(|error| panic!("v1 scope must validate: {error}"));
    assert_eq!(scope.authority, V1Component::PostgresAuthority);
    assert_eq!(scope.transport, V1Component::PostcardHttps);
    assert_eq!(
        scope.official_local_adapters,
        [V1Component::SQLiteLocal].into_iter().collect()
    );
    assert!(
        scope
            .conditional_local_adapters
            .contains(&V1Component::StoolapLocal)
    );
    assert!(scope.single_logical_writer);
}

#[test]
fn development_readiness_snapshot_is_explicitly_not_ga() {
    let manifest = GaReadinessManifest::from_ron(include_str!("../../../release/v1-readiness.ron"))
        .unwrap_or_else(|error| panic!("readiness snapshot must validate: {error}"));
    let decision = manifest
        .evaluate()
        .unwrap_or_else(|error| panic!("readiness snapshot must evaluate: {error}"));
    assert!(!decision.is_eligible());
    assert_eq!(decision.exact_candidate_artifact, DecisionCheck::Blocking);
    assert_eq!(decision.upgrade_evidence, DecisionCheck::Blocking);
    assert_eq!(decision.readiness_reviews, DecisionCheck::Blocking);
    assert_eq!(decision.blocking_milestones.len(), Milestone::ALL.len());
    assert!(decision.blocking_gates.contains(&GaGate::ReleaseProcess));
    assert!(manifest.support_claims.iter().any(|claim| {
        claim.surface == "Stoolap local adapter" && claim.maturity == Maturity::Experimental
    }));
}

#[test]
fn ga_invariants_and_conformance_ids_are_complete_and_stable() {
    let expected = (1..=10)
        .map(|number| format!("AEQ-INV-GA{number:03}"))
        .collect::<Vec<_>>();
    let registered = InvariantId::ALL
        .iter()
        .map(|id| id.as_str())
        .filter(|id| id.starts_with("AEQ-INV-GA"))
        .collect::<Vec<_>>();
    assert_eq!(
        registered,
        expected.iter().map(String::as_str).collect::<Vec<_>>()
    );

    let ids = definitions_for(
        ConformanceProfile::GeneralAvailabilityFull,
        CertificationTier::FullSync,
    )
    .into_iter()
    .map(|definition| definition.id.0)
    .collect::<Vec<_>>();
    assert_eq!(ids, (184..=193).collect::<Vec<_>>());
}
