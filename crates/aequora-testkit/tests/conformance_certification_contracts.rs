use aequora_conformance::{
    CertificationResult, CertificationTier, ConformanceProfile, ExecutionEnvironment,
    SubjectIdentity, TestDefinition, TestObservation, TestStatus,
};
use aequora_testkit::conformance::{
    ConformanceSubject, HarnessConfig, HarnessError, passed, run_conformance,
};
use async_trait::async_trait;
use std::collections::{BTreeMap, BTreeSet};

struct DeterministicSubject {
    skip_test: Option<u32>,
}

#[async_trait]
impl ConformanceSubject for DeterministicSubject {
    fn identity(&self) -> SubjectIdentity {
        SubjectIdentity {
            name: "reference-conformance-subject".into(),
            version: "0.1.0".into(),
            source_commit: "fixture-commit".into(),
            binary_digest: "a".repeat(64),
            enabled_features: BTreeSet::new(),
            build_configuration: BTreeMap::new(),
        }
    }

    fn environment(&self) -> ExecutionEnvironment {
        ExecutionEnvironment {
            target: "test-target".into(),
            operating_system: "test-os".into(),
            architecture: "test-arch".into(),
            rust_version: "1.87".into(),
            database_engine: None,
            database_version: None,
            provider_versions: BTreeMap::new(),
        }
    }

    fn claimed_capabilities(&self) -> BTreeSet<String> {
        BTreeSet::new()
    }

    async fn run_test(
        &self,
        definition: &'static TestDefinition,
        seed: u64,
    ) -> Result<TestObservation, HarnessError> {
        if self.skip_test == Some(definition.id.0) {
            return Ok(TestObservation {
                test_id: definition.id,
                status: TestStatus::Skipped,
                seed: Some(seed),
                evidence: Vec::new(),
                message: "deliberately skipped".into(),
            });
        }
        Ok(passed(definition, seed, "b".repeat(64)))
    }
}

fn config() -> HarnessConfig {
    HarnessConfig {
        profile: ConformanceProfile::StorageCore,
        tier: CertificationTier::CoreTransactional,
        seed: 41,
        issued_at_unix_ms: 1,
        limitations: Vec::new(),
    }
}

#[tokio::test]
async fn deterministic_harness_binds_seeds_and_exact_subject() -> Result<(), HarnessError> {
    let artifact = run_conformance(&DeterministicSubject { skip_test: None }, config()).await?;
    artifact.verify_identity()?;
    assert_eq!(artifact.result, CertificationResult::Passed);
    assert_eq!(artifact.observations[0].seed, Some(41));
    assert_eq!(artifact.observations[1].seed, Some(42));
    Ok(())
}

#[tokio::test]
async fn skipped_required_test_creates_failure_not_a_tier_claim() -> Result<(), HarnessError> {
    let artifact = run_conformance(&DeterministicSubject { skip_test: Some(1) }, config()).await?;
    assert_eq!(artifact.result, CertificationResult::Failed);
    assert!(artifact.verify_identity().is_ok());
    Ok(())
}
