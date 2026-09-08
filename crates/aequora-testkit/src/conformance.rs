//! Reusable deterministic harness for third-party and built-in certification subjects.

use aequora_conformance::{
    CertificationArtifact, CertificationTier, ConformanceError, ConformanceProfile,
    ExecutionEnvironment, SubjectIdentity, TestDefinition, TestObservation, TestStatus, certify,
    definitions_for,
};
use async_trait::async_trait;
use std::collections::BTreeSet;
use thiserror::Error;

/// Stable fault points used by crash/retry/fencing conformance probes.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum FaultPoint {
    BeforeTransaction,
    AfterTransactionBegin,
    AfterDomainMutation,
    AfterOutboxWrite,
    BeforeCommit,
    AfterCommit,
    BeforeResponse,
    AfterResponseDecode,
    BeforeCursorWrite,
    AfterCursorWrite,
    DuringMigration,
    DuringSnapshotInstall,
    AfterStateBeforeIntent,
    AfterIntentBeforeCommit,
    AfterCommitBeforeResponse,
    BeforeCursorCommit,
    AfterCursorCommit,
    BeforeSnapshotPublish,
    AfterSnapshotPublish,
    StaleAuthorityEpoch,
    DuplicateDelivery,
}

/// A certifiable implementation executes tests through public, observable behavior.
#[async_trait]
pub trait ConformanceSubject: Send + Sync {
    fn identity(&self) -> SubjectIdentity;
    fn environment(&self) -> ExecutionEnvironment;
    fn claimed_capabilities(&self) -> BTreeSet<String>;

    /// Runs one stable test in an isolated namespace using the supplied reproducible seed.
    async fn run_test(
        &self,
        definition: &'static TestDefinition,
        seed: u64,
    ) -> Result<TestObservation, HarnessError>;
}

/// Optional fault injection contract. Implementations must reset all injected state between tests.
#[async_trait]
pub trait FaultInjectable: Send + Sync {
    async fn enable_fault(&self, point: FaultPoint) -> Result<(), HarnessError>;
    async fn clear_faults(&self) -> Result<(), HarnessError>;
}

#[derive(Clone, Debug)]
pub struct HarnessConfig {
    pub profile: ConformanceProfile,
    pub tier: CertificationTier,
    pub seed: u64,
    pub issued_at_unix_ms: u64,
    pub limitations: Vec<String>,
}

/// Runs every required test sequentially, preserving each failure as evidence rather than aborting.
///
/// Setup failures stop the run; semantic failures must be returned as `TestStatus::Failed`.
///
/// # Errors
///
/// Returns setup, identity, observation-integrity, or artifact validation failures.
pub async fn run_conformance<S: ConformanceSubject>(
    subject: &S,
    config: HarnessConfig,
) -> Result<CertificationArtifact, HarnessError> {
    let mut observations = Vec::new();
    for (index, definition) in definitions_for(config.profile, config.tier)
        .into_iter()
        .enumerate()
    {
        let offset = u64::try_from(index).map_err(|_| HarnessError::SeedOverflow)?;
        let seed = config
            .seed
            .checked_add(offset)
            .ok_or(HarnessError::SeedOverflow)?;
        let observation = subject.run_test(definition, seed).await?;
        if observation.test_id != definition.id || observation.seed != Some(seed) {
            return Err(HarnessError::ObservationBinding);
        }
        observations.push(observation);
    }
    certify(
        subject.identity(),
        subject.environment(),
        config.profile,
        config.tier,
        subject.claimed_capabilities(),
        observations,
        config.limitations,
        config.issued_at_unix_ms,
    )
    .map_err(HarnessError::Conformance)
}

/// Creates a payload-free passing observation for a completed semantic assertion.
#[must_use]
pub fn passed(definition: &TestDefinition, seed: u64, evidence_digest: String) -> TestObservation {
    TestObservation {
        test_id: definition.id,
        status: TestStatus::Passed,
        seed: Some(seed),
        evidence: vec![evidence_digest],
        message: definition.name.to_owned(),
    }
}

/// Creates a payload-free failed observation while retaining the reproducible seed.
#[must_use]
pub fn failed(
    definition: &TestDefinition,
    seed: u64,
    reason: impl Into<String>,
) -> TestObservation {
    TestObservation {
        test_id: definition.id,
        status: TestStatus::Failed,
        seed: Some(seed),
        evidence: Vec::new(),
        message: reason.into(),
    }
}

#[derive(Debug, Error)]
pub enum HarnessError {
    #[error("conformance subject setup failed: {0}")]
    Setup(String),
    #[error("test observation did not bind the requested test ID and seed")]
    ObservationBinding,
    #[error("deterministic seed overflow")]
    SeedOverflow,
    #[error(transparent)]
    Conformance(#[from] ConformanceError),
}

/// Generates a complete downstream conformance test from a subject expression.
#[macro_export]
macro_rules! aequora_conformance_compliance {
    ($test_name:ident, $subject:expr, $config:expr) => {
        #[tokio::test]
        async fn $test_name() -> Result<(), Box<dyn std::error::Error>> {
            let artifact = $crate::conformance::run_conformance(&$subject, $config).await?;
            artifact.verify_identity()?;
            if !matches!(
                artifact.result,
                aequora_conformance::CertificationResult::Passed
                    | aequora_conformance::CertificationResult::PassedWithLimitations
            ) {
                return Err(format!("conformance result was {:?}", artifact.result).into());
            }
            Ok(())
        }
    };
}
