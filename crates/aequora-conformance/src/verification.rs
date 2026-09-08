//! Neutral release-verification manifests, evidence, waivers, and invariant coverage.

use aequora_invariants::InvariantId;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use thiserror::Error;

pub const QUALITY_GATE_SCHEMA_VERSION: u32 = 1;
pub const MAX_EVIDENCE_REFS: usize = 4_096;
pub const MAX_TEST_RESULTS: usize = 65_536;
pub const MAX_TEXT_BYTES: usize = 1_024;

/// A stable verification suite that may participate in a release profile.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
pub enum SuiteId {
    Unit,
    Property,
    Model,
    SQLite,
    Stoolap,
    PostgreSql,
    AxumHttp,
    ProtocolCompatibility,
    Migration,
    ProcessKill,
    FaultInjection,
    Bootstrap,
    Conflict,
    Scope,
    Security,
    ArtifactIntegrity,
    Performance,
    Observability,
    BackupRestore,
    AuthorityFailover,
    MobilePlatform,
    DesktopPlatform,
    AirGapped,
}

/// Product surface qualified by a quality-gate manifest.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
pub enum ReleaseProfile {
    CoreLibrary,
    Desktop,
    Mobile,
    ServerStandard,
    Enterprise,
    AirGapped,
}

impl ReleaseProfile {
    /// Minimum correctness suites for the declared support profile.
    #[must_use]
    pub fn required_suites(self) -> BTreeSet<SuiteId> {
        let mut suites = BTreeSet::from([
            SuiteId::Unit,
            SuiteId::Property,
            SuiteId::Model,
            SuiteId::ProtocolCompatibility,
            SuiteId::Security,
            SuiteId::ArtifactIntegrity,
        ]);
        match self {
            Self::CoreLibrary => {}
            Self::Desktop => {
                suites.extend([
                    SuiteId::SQLite,
                    SuiteId::Stoolap,
                    SuiteId::Migration,
                    SuiteId::ProcessKill,
                    SuiteId::DesktopPlatform,
                ]);
            }
            Self::Mobile => {
                suites.extend([
                    SuiteId::SQLite,
                    SuiteId::Stoolap,
                    SuiteId::Migration,
                    SuiteId::ProcessKill,
                    SuiteId::MobilePlatform,
                ]);
            }
            Self::ServerStandard => {
                suites.extend([
                    SuiteId::PostgreSql,
                    SuiteId::AxumHttp,
                    SuiteId::Migration,
                    SuiteId::FaultInjection,
                    SuiteId::BackupRestore,
                ]);
            }
            Self::Enterprise => {
                suites.extend([
                    SuiteId::SQLite,
                    SuiteId::Stoolap,
                    SuiteId::PostgreSql,
                    SuiteId::AxumHttp,
                    SuiteId::Migration,
                    SuiteId::ProcessKill,
                    SuiteId::FaultInjection,
                    SuiteId::Bootstrap,
                    SuiteId::Conflict,
                    SuiteId::Scope,
                    SuiteId::Performance,
                    SuiteId::Observability,
                    SuiteId::BackupRestore,
                    SuiteId::AuthorityFailover,
                ]);
            }
            Self::AirGapped => {
                suites.extend([
                    SuiteId::SQLite,
                    SuiteId::PostgreSql,
                    SuiteId::AxumHttp,
                    SuiteId::Migration,
                    SuiteId::FaultInjection,
                    SuiteId::BackupRestore,
                    SuiteId::AirGapped,
                ]);
            }
        }
        suites
    }
}

/// Required suite result. Required failures and non-applicable results block qualification.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum GateStatus {
    Pass,
    Fail,
    Waived,
    NotApplicable,
}

/// Payload-free reference to retained verification evidence.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct EvidenceRef {
    pub kind: String,
    pub location: String,
    pub digest: String,
}

impl EvidenceRef {
    fn validate(&self) -> Result<(), VerificationError> {
        validate_text("evidence kind", &self.kind)?;
        validate_text("evidence location", &self.location)?;
        validate_text("evidence digest", &self.digest)
    }
}

/// Explicit, owned, scoped, and expiring acceptance of a known gate risk.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct GateWaiver {
    pub reason: String,
    pub owner: String,
    pub risk: String,
    pub expires_at_unix_ms: u64,
    pub affected_profile: ReleaseProfile,
    pub affected_suite: SuiteId,
}

impl GateWaiver {
    fn validate(
        &self,
        issued_at_unix_ms: u64,
        profile: ReleaseProfile,
        suite: SuiteId,
    ) -> Result<(), VerificationError> {
        validate_text("waiver reason", &self.reason)?;
        validate_text("waiver owner", &self.owner)?;
        validate_text("waiver risk", &self.risk)?;
        if self.expires_at_unix_ms <= issued_at_unix_ms {
            return Err(VerificationError::ExpiredWaiver(suite));
        }
        if self.affected_profile != profile || self.affected_suite != suite {
            return Err(VerificationError::WaiverScopeMismatch(suite));
        }
        Ok(())
    }
}

/// Result and evidence for one suite.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct GateResult {
    pub status: GateStatus,
    pub evidence: Vec<EvidenceRef>,
    pub waiver: Option<GateWaiver>,
}

/// Versioned, machine-readable release qualification input.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct QualityGateManifest {
    pub schema_version: u32,
    pub release: String,
    pub profile: ReleaseProfile,
    pub issued_at_unix_ms: u64,
    pub required_suites: BTreeSet<SuiteId>,
    pub results: BTreeMap<SuiteId, GateResult>,
    pub evidence: Vec<EvidenceRef>,
}

/// Release decision retaining every blocker rather than stopping at the first failure.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Qualification {
    Passed,
    PassedWithWaivers { suites: BTreeSet<SuiteId> },
    Blocked { suites: BTreeSet<SuiteId> },
}

impl QualityGateManifest {
    /// Validates bounded structure and returns a deterministic release decision.
    ///
    /// # Errors
    ///
    /// Returns [`VerificationError`] for malformed, unbounded, mismatched, or expired evidence.
    pub fn evaluate(&self) -> Result<Qualification, VerificationError> {
        if self.schema_version != QUALITY_GATE_SCHEMA_VERSION {
            return Err(VerificationError::UnsupportedSchema(self.schema_version));
        }
        validate_text("release", &self.release)?;
        if self.evidence.len() > MAX_EVIDENCE_REFS {
            return Err(VerificationError::EvidenceLimit);
        }
        for evidence in &self.evidence {
            evidence.validate()?;
        }
        let policy_suites = self.profile.required_suites();
        let undeclared = policy_suites
            .difference(&self.required_suites)
            .copied()
            .collect::<BTreeSet<_>>();
        if !undeclared.is_empty() {
            return Err(VerificationError::ProfileRequirementsMissing(undeclared));
        }

        let mut blocked = BTreeSet::new();
        let mut waived = BTreeSet::new();
        for suite in &self.required_suites {
            let Some(result) = self.results.get(suite) else {
                blocked.insert(*suite);
                continue;
            };
            validate_gate_result(self, *suite, result)?;
            match result.status {
                GateStatus::Pass => {}
                GateStatus::Waived => {
                    waived.insert(*suite);
                }
                GateStatus::Fail | GateStatus::NotApplicable => {
                    blocked.insert(*suite);
                }
            }
        }
        for (suite, result) in &self.results {
            validate_gate_result(self, *suite, result)?;
        }
        if !blocked.is_empty() {
            Ok(Qualification::Blocked { suites: blocked })
        } else if !waived.is_empty() {
            Ok(Qualification::PassedWithWaivers { suites: waived })
        } else {
            Ok(Qualification::Passed)
        }
    }
}

fn validate_gate_result(
    manifest: &QualityGateManifest,
    suite: SuiteId,
    result: &GateResult,
) -> Result<(), VerificationError> {
    if result.evidence.len() > MAX_EVIDENCE_REFS {
        return Err(VerificationError::EvidenceLimit);
    }
    for evidence in &result.evidence {
        evidence.validate()?;
    }
    if matches!(result.status, GateStatus::Pass | GateStatus::Waived) && result.evidence.is_empty()
    {
        return Err(VerificationError::MissingEvidence(suite));
    }
    match (result.status, &result.waiver) {
        (GateStatus::Waived, Some(waiver)) => {
            waiver.validate(manifest.issued_at_unix_ms, manifest.profile, suite)
        }
        (GateStatus::Waived, None) => Err(VerificationError::MissingWaiver(suite)),
        (_, Some(_)) => Err(VerificationError::UnexpectedWaiver(suite)),
        _ => Ok(()),
    }
}

/// Operational class used to place a test in an appropriate CI tier.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
pub enum TestCategory {
    Unit,
    Property,
    Model,
    Integration,
    EndToEnd,
    Fault,
    Migration,
    Security,
    Performance,
    Soak,
    Platform,
}

/// Stable critical test identity.
#[derive(Clone, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct TestId(pub String);

/// One payload-free test observation with reproduction metadata.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct VerificationTestResult {
    pub test_id: TestId,
    pub category: TestCategory,
    pub status: GateStatus,
    pub seed: Option<u64>,
    pub scenario: String,
    pub adapter: Option<String>,
    pub evidence: Vec<EvidenceRef>,
}

/// Fingerprint of the exact environment that produced a report.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct EnvironmentFingerprint {
    pub target: String,
    pub toolchain: String,
    pub configuration_digest: String,
    pub registry_digest: String,
}

/// Machine-readable report for one executed suite.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct VerificationReport {
    pub schema_version: u32,
    pub build: String,
    pub suite: SuiteId,
    pub environment: EnvironmentFingerprint,
    pub results: Vec<VerificationTestResult>,
    pub evidence: Vec<EvidenceRef>,
}

impl VerificationReport {
    /// Validates report bounds and deterministic reproduction metadata.
    ///
    /// # Errors
    ///
    /// Returns [`VerificationError`] when required identity, seed, or evidence is absent.
    pub fn validate(&self) -> Result<(), VerificationError> {
        if self.schema_version != QUALITY_GATE_SCHEMA_VERSION {
            return Err(VerificationError::UnsupportedSchema(self.schema_version));
        }
        validate_text("build", &self.build)?;
        validate_text("target", &self.environment.target)?;
        validate_text("toolchain", &self.environment.toolchain)?;
        validate_text(
            "configuration digest",
            &self.environment.configuration_digest,
        )?;
        validate_text("registry digest", &self.environment.registry_digest)?;
        if self.results.len() > MAX_TEST_RESULTS || self.evidence.len() > MAX_EVIDENCE_REFS {
            return Err(VerificationError::EvidenceLimit);
        }
        let mut ids = BTreeSet::new();
        for result in &self.results {
            validate_text("test id", &result.test_id.0)?;
            validate_text("scenario", &result.scenario)?;
            if !ids.insert(&result.test_id) {
                return Err(VerificationError::DuplicateTestId(result.test_id.clone()));
            }
            if matches!(
                result.category,
                TestCategory::Property | TestCategory::Model | TestCategory::Fault
            ) && result.seed.is_none()
            {
                return Err(VerificationError::MissingReproductionSeed(
                    result.test_id.clone(),
                ));
            }
            for evidence in &result.evidence {
                evidence.validate()?;
            }
        }
        for evidence in &self.evidence {
            evidence.validate()?;
        }
        Ok(())
    }
}

/// Executable mapping from stable invariants to the tests that cover them.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct InvariantCoverageMap {
    pub coverage: BTreeMap<InvariantId, BTreeSet<TestId>>,
}

impl InvariantCoverageMap {
    pub fn record(&mut self, invariant: InvariantId, test: TestId) {
        self.coverage.entry(invariant).or_default().insert(test);
    }

    /// Returns critical invariants that have no executable test binding.
    #[must_use]
    pub fn uncovered(&self, required: &[InvariantId]) -> BTreeSet<InvariantId> {
        required
            .iter()
            .copied()
            .filter(|invariant| self.coverage.get(invariant).is_none_or(BTreeSet::is_empty))
            .collect()
    }
}

fn validate_text(field: &'static str, value: &str) -> Result<(), VerificationError> {
    if value.is_empty() || value.len() > MAX_TEXT_BYTES {
        Err(VerificationError::InvalidText(field))
    } else {
        Ok(())
    }
}

#[derive(Clone, Debug, Error, Eq, PartialEq)]
pub enum VerificationError {
    #[error("unsupported verification schema {0}")]
    UnsupportedSchema(u32),
    #[error("{0} is empty or exceeds the verification text bound")]
    InvalidText(&'static str),
    #[error("verification evidence or result limit exceeded")]
    EvidenceLimit,
    #[error("waived suite {0:?} has no waiver")]
    MissingWaiver(SuiteId),
    #[error("suite {0:?} carries a waiver without Waived status")]
    UnexpectedWaiver(SuiteId),
    #[error("waiver for {0:?} is expired or non-expiring")]
    ExpiredWaiver(SuiteId),
    #[error("waiver scope does not match suite {0:?} and release profile")]
    WaiverScopeMismatch(SuiteId),
    #[error("duplicate stable test id {0:?}")]
    DuplicateTestId(TestId),
    #[error("randomized test {0:?} omitted its reproduction seed")]
    MissingReproductionSeed(TestId),
    #[error("declared release profile omitted required suites {0:?}")]
    ProfileRequirementsMissing(BTreeSet<SuiteId>),
    #[error("passing or waived suite {0:?} has no evidence")]
    MissingEvidence(SuiteId),
}
