use aequora_conformance::verification::{
    EnvironmentFingerprint, EvidenceRef, GateResult, GateStatus, GateWaiver, InvariantCoverageMap,
    QUALITY_GATE_SCHEMA_VERSION, Qualification, QualityGateManifest, ReleaseProfile, SuiteId,
    TestCategory, TestId, VerificationError, VerificationReport, VerificationTestResult,
};
use aequora_invariants::InvariantId;
use aequora_testkit::{
    conformance::FaultPoint,
    verification::{
        FaultAction, FaultErrorClass, FixtureVersion, GoldenFixture, TestConfiguration,
        TestContext, TestSeed,
    },
};
use std::{collections::BTreeSet, time::Duration};

fn evidence(name: &str) -> EvidenceRef {
    EvidenceRef {
        kind: "verification-report".to_owned(),
        location: format!("artifacts/{name}.ron"),
        digest: format!("blake3:{name}"),
    }
}

fn result(status: GateStatus) -> GateResult {
    GateResult {
        status,
        evidence: vec![evidence("suite")],
        waiver: None,
    }
}

fn manifest() -> QualityGateManifest {
    let profile = ReleaseProfile::ServerStandard;
    let required_suites = profile.required_suites();
    let results = required_suites
        .iter()
        .copied()
        .map(|suite| (suite, result(GateStatus::Pass)))
        .collect();
    QualityGateManifest {
        schema_version: QUALITY_GATE_SCHEMA_VERSION,
        release: "0.1.0-rc.1".to_owned(),
        profile,
        issued_at_unix_ms: 1_000,
        required_suites,
        results,
        evidence: vec![evidence("manifest")],
    }
}

#[test]
fn deterministic_context_replays_ids_clock_and_fault_schedule() {
    let first = TestContext::new(TestSeed(48), 10_000);
    let second = TestContext::new(TestSeed(48), 10_000);
    assert_eq!(first.ids.operation_id(), second.ids.operation_id());
    assert_eq!(first.ids.entity_id(), second.ids.entity_id());

    first.clock.freeze();
    first
        .clock
        .tick(Duration::from_secs(5))
        .unwrap_or_else(|error| panic!("clock tick failed: {error}"));
    assert_eq!(first.clock.monotonic_ms(), 0);
    first
        .clock
        .advance(Duration::from_millis(7))
        .unwrap_or_else(|error| panic!("clock advance failed: {error}"));
    first.clock.jump_wall(2);
    assert_eq!(first.clock.monotonic_ms(), 7);
    assert_eq!(first.clock.wall_unix_ms(), 2);

    first.faults.schedule(
        FaultPoint::AfterCommitBeforeResponse,
        FaultAction::ReturnError(FaultErrorClass::Transient),
    );
    assert_eq!(
        first.faults.hit(FaultPoint::AfterCommitBeforeResponse),
        FaultAction::ReturnError(FaultErrorClass::Transient)
    );
    assert_eq!(
        first.faults.hit(FaultPoint::AfterCommitBeforeResponse),
        FaultAction::Continue
    );
    assert!(
        first
            .reproduction_label("lost-response", "reference", "build-48")
            .contains("seed=48")
    );
}

#[test]
fn golden_fixture_is_version_bound_and_never_silently_accepts_drift() {
    let fixture = GoldenFixture::from_reviewed_bytes(
        "protocol-v1",
        FixtureVersion {
            schema_version: 3,
            protocol_version: 2,
            registry_generation: 9,
        },
        b"reviewed",
    );
    assert!(fixture.verify(b"reviewed"));
    assert!(!fixture.verify(b"regenerated-to-hide-a-failure"));
}

#[test]
fn required_failures_missing_results_and_not_applicable_block_release() {
    let mut gates = manifest();
    gates.results.remove(&SuiteId::PostgreSql);
    gates
        .results
        .insert(SuiteId::Security, result(GateStatus::Fail));
    gates
        .results
        .insert(SuiteId::AxumHttp, result(GateStatus::NotApplicable));
    let qualification = gates
        .evaluate()
        .unwrap_or_else(|error| panic!("manifest was malformed: {error}"));
    let Qualification::Blocked { suites } = qualification else {
        panic!("required failures must block release");
    };
    assert_eq!(
        suites,
        BTreeSet::from([SuiteId::PostgreSql, SuiteId::AxumHttp, SuiteId::Security])
    );
}

#[test]
fn waiver_must_be_owned_scoped_risk_documented_and_expiring() {
    let mut gates = manifest();
    gates.results.insert(
        SuiteId::Security,
        GateResult {
            status: GateStatus::Waived,
            evidence: vec![evidence("security")],
            waiver: Some(GateWaiver {
                reason: "provider sandbox outage".to_owned(),
                owner: "security-team".to_owned(),
                risk: "sandbox contract drift may be missed".to_owned(),
                expires_at_unix_ms: 2_000,
                affected_profile: ReleaseProfile::ServerStandard,
                affected_suite: SuiteId::Security,
            }),
        },
    );
    assert_eq!(
        gates.evaluate(),
        Ok(Qualification::PassedWithWaivers {
            suites: BTreeSet::from([SuiteId::Security])
        })
    );

    gates
        .results
        .get_mut(&SuiteId::Security)
        .and_then(|result| result.waiver.as_mut())
        .unwrap_or_else(|| panic!("waiver must exist"))
        .expires_at_unix_ms = 1_000;
    assert_eq!(
        gates.evaluate(),
        Err(VerificationError::ExpiredWaiver(SuiteId::Security))
    );
}

#[test]
fn randomized_reports_require_seed_and_stable_unique_test_ids() {
    let mut report = VerificationReport {
        schema_version: QUALITY_GATE_SCHEMA_VERSION,
        build: "build-48".to_owned(),
        suite: SuiteId::Property,
        environment: EnvironmentFingerprint {
            target: "x86_64-unknown-linux-gnu".to_owned(),
            toolchain: "rustc-1.87".to_owned(),
            configuration_digest: "config-digest".to_owned(),
            registry_digest: "registry-digest".to_owned(),
        },
        results: vec![VerificationTestResult {
            test_id: TestId("AEQ-TEST-IDEMP-001".to_owned()),
            category: TestCategory::Property,
            status: GateStatus::Pass,
            seed: Some(48),
            scenario: "ambiguous-commit".to_owned(),
            adapter: Some("reference".to_owned()),
            evidence: vec![evidence("idempotency")],
        }],
        evidence: vec![evidence("report")],
    };
    assert_eq!(report.validate(), Ok(()));
    report.results[0].seed = None;
    assert!(matches!(
        report.validate(),
        Err(VerificationError::MissingReproductionSeed(_))
    ));
}

#[test]
fn every_part_48_invariant_has_a_stable_test_binding() {
    let required = [
        InvariantId::VerificationInvariantEvidence,
        InvariantId::VerificationFailureBoundaryCoverage,
        InvariantId::VerificationAdapterParity,
        InvariantId::VerificationCursorAtomicity,
        InvariantId::VerificationAmbiguousCommit,
        InvariantId::VerificationUpgradePreservation,
        InvariantId::VerificationReproducibility,
        InvariantId::VerificationWaiverGovernance,
        InvariantId::VerificationIncidentRegression,
        InvariantId::VerificationProductionParity,
    ];
    let mut coverage = InvariantCoverageMap::default();
    for (offset, invariant) in required.into_iter().enumerate() {
        coverage.record(
            invariant,
            TestId(format!("AEQ-TEST-VERIFY-{:03}", offset + 1)),
        );
    }
    assert!(coverage.uncovered(&required).is_empty());
}

#[test]
fn all_committed_test_profiles_are_bounded_and_seeded() {
    for encoded in [
        include_str!("../../../config/test/unit.ron"),
        include_str!("../../../config/test/integration.ron"),
        include_str!("../../../config/test/e2e.ron"),
        include_str!("../../../config/test/fault.ron"),
    ] {
        let configuration: TestConfiguration = ron::from_str(encoded)
            .unwrap_or_else(|error| panic!("test configuration did not decode: {error}"));
        configuration
            .validate()
            .unwrap_or_else(|error| panic!("test configuration was invalid: {error}"));
    }
}
