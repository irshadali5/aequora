use aequora_supply_chain::{
    DependencyChangeEvidence, DependencyPolicy, ReleaseEvidence, ReproducibilityLevel,
    ReproducibilityReport, RiskTier, SbomDocument, SemanticImpact, SupplyChainError,
    ThirdPartyService, VerificationArea, validate_services,
};
use std::collections::BTreeSet;

fn policy() -> DependencyPolicy {
    DependencyPolicy::from_ron(
        include_str!("../../../supply-chain/policy.ron"),
        1_788_739_200,
    )
    .unwrap_or_else(|error| panic!("checked-in dependency policy must validate: {error}"))
}

#[test]
fn checked_in_policy_reviews_owned_replaceable_critical_dependencies() {
    let policy = policy();
    assert!(policy.dependencies.len() >= 16);
    assert!(policy.dependencies.iter().all(|dependency| {
        dependency.risk < RiskTier::Tier2
            || (dependency.owner != "unowned" && dependency.replacement != "none")
    }));
    assert!(policy.dependency("sqlx").is_some());
    assert!(policy.dependency("rusqlite").is_some());
    assert!(policy.dependency("stoolap").is_some());
    assert!(policy.dependency("chacha20poly1305").is_some());
}

#[test]
fn release_sbom_and_provenance_are_bound_to_identical_inputs() {
    let policy = policy();
    let sbom: SbomDocument = ron::from_str(include_str!(
        "../../../tests/fixtures/part49/server.spdx.ron"
    ))
    .unwrap_or_else(|error| panic!("SBOM fixture must parse: {error}"));
    let evidence: ReleaseEvidence = ron::from_str(include_str!(
        "../../../tests/fixtures/part49/server-release-evidence.ron"
    ))
    .unwrap_or_else(|error| panic!("release evidence fixture must parse: {error}"));
    assert_eq!(sbom.validate(&policy), Ok(()));
    assert_eq!(evidence.validate(), Ok(()));
    assert_eq!(evidence.verify_sbom(&sbom), Ok(()));
}

#[test]
fn authority_dependency_update_requires_model_integration_and_security_evidence() {
    let mut evidence = DependencyChangeEvidence {
        package: "authority-critical-package".to_owned(),
        impact: SemanticImpact::Authority,
        passed: BTreeSet::from([
            VerificationArea::Unit,
            VerificationArea::Property,
            VerificationArea::Security,
        ]),
    };
    assert!(matches!(
        evidence.validate(),
        Err(SupplyChainError::IncompleteUpdateEvidence(_))
    ));
    evidence
        .passed
        .extend([VerificationArea::Model, VerificationArea::Integration]);
    assert_eq!(evidence.validate(), Ok(()));
}

#[test]
fn byte_identical_claim_requires_identical_independent_builder_digests() {
    let report = ReproducibilityReport {
        artifact: "aequora-server".to_owned(),
        claimed_level: ReproducibilityLevel::ByteIdentical,
        builder_a_digest: "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
            .to_owned(),
        builder_b_digest: "sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"
            .to_owned(),
        package_contents_equal: true,
        functional_tests_equal: true,
        differences: vec!["archive metadata differs".to_owned()],
    };
    assert_eq!(
        report.validate(),
        Err(SupplyChainError::OverstatedReproducibility)
    );
}

#[test]
fn operational_provider_cannot_be_hidden_authority_or_air_gap_requirement() {
    let services: Vec<ThirdPartyService> = ron::from_str(include_str!(
        "../../../supply-chain/third-party-services.ron"
    ))
    .unwrap_or_else(|error| panic!("service inventory must parse: {error}"));
    assert_eq!(validate_services(&services), Ok(()));
    let mut invalid = services[0].clone();
    invalid.source_of_authority = true;
    assert!(matches!(
        validate_services(&[invalid]),
        Err(SupplyChainError::InvalidServiceBoundary(_))
    ));
}
