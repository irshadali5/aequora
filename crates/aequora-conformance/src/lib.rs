//! Deterministic, database-neutral conformance and certification contracts.
//!
//! This crate certifies observable semantics. It deliberately has no database, network, runtime,
//! filesystem, or process dependency; adapters provide observations and applications decide how
//! to persist or publish the resulting evidence bundle.

use aequora_registry_types::{CertificationTierId, ConformanceProfileId, ConformanceTestId};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    fmt::Write as _,
};
use thiserror::Error;

pub const ARTIFACT_FORMAT_VERSION: u32 = 1;
pub const CURRENT_SUITE_VERSION: u32 = 1;
pub const MAX_IDENTITY_TEXT_BYTES: usize = 512;
pub const MAX_EVIDENCE_REFERENCE_BYTES: usize = 1_024;
pub const MAX_OBSERVATION_MESSAGE_BYTES: usize = 2_048;
pub const MAX_METADATA_ENTRIES: usize = 256;
pub const MAX_EVIDENCE_REFERENCES_PER_TEST: usize = 64;
pub const MAX_LIMITATIONS: usize = 128;
pub const MAX_PERFORMANCE_CHARACTERIZATIONS: usize = 128;
pub const MAX_EVIDENCE_FILES: usize = 4_096;
pub const MAX_SIGNATURE_BYTES: usize = 16 * 1_024;
pub const MAX_CATALOG_RECORDS: usize = 65_536;

/// A surface with independently testable Aequora semantics.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
pub enum ConformanceDomain {
    StorageAdapter,
    ClientRuntime,
    ServerRuntime,
    ProtocolImplementation,
    SnapshotProvider,
    CryptoProvider,
    JobSideEffectProvider,
    ChangeFeedConsumer,
    LegacyBridge,
    ExtensionApplication,
    MobileRuntime,
    DesktopRuntime,
    DeveloperTooling,
}

/// Strength of a certification claim. Higher tiers include lower-tier requirements.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
pub enum CertificationTier {
    Experimental,
    CoreTransactional,
    FullSync,
    Enterprise,
}

impl CertificationTier {
    #[must_use]
    pub const fn id(self) -> CertificationTierId {
        CertificationTierId(match self {
            Self::Experimental => 1,
            Self::CoreTransactional => 10,
            Self::FullSync => 20,
            Self::Enterprise => 30,
        })
    }
}

/// Published profile names bind test selection to an intended role.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
pub enum ConformanceProfile {
    StorageCore,
    StorageFullSync,
    ClientCore,
    ClientFullSync,
    ServerCore,
    ServerEnterprise,
    ProtocolCore,
    Provider,
    Integration,
    MobileClientFull,
    DesktopClientFull,
    DesktopAgentFull,
    MobileLocalStoreFull,
    DesktopLocalStoreFull,
    LocalAdapter,
    AuthoritativeAdapter,
    SnapshotAdapter,
    FencingAdapter,
    PostgresAuthorityFull,
    NeonOperationalProfile,
    StoolapLocalCore,
    StoolapDesktopLocalFull,
    StoolapMobileLocalFull,
    AxumServerFull,
    DioxusClientFull,
    CliToolchainFull,
    SQLiteLocalCore,
    SQLiteDesktopLocalFull,
    SQLiteMobileLocalFull,
    ConfigurationRuntimeFull,
}

impl ConformanceProfile {
    #[must_use]
    pub const fn id(self) -> ConformanceProfileId {
        ConformanceProfileId(match self {
            Self::StorageCore => 1,
            Self::StorageFullSync => 2,
            Self::ClientCore => 10,
            Self::ClientFullSync => 11,
            Self::ServerCore => 20,
            Self::ServerEnterprise => 21,
            Self::ProtocolCore => 30,
            Self::Provider => 40,
            Self::Integration => 50,
            Self::MobileClientFull => 60,
            Self::DesktopClientFull => 61,
            Self::DesktopAgentFull => 62,
            Self::MobileLocalStoreFull => 63,
            Self::DesktopLocalStoreFull => 64,
            Self::LocalAdapter => 65,
            Self::AuthoritativeAdapter => 66,
            Self::SnapshotAdapter => 67,
            Self::FencingAdapter => 68,
            Self::PostgresAuthorityFull => 69,
            Self::NeonOperationalProfile => 70,
            Self::StoolapLocalCore => 71,
            Self::StoolapDesktopLocalFull => 72,
            Self::StoolapMobileLocalFull => 73,
            Self::AxumServerFull => 74,
            Self::DioxusClientFull => 75,
            Self::CliToolchainFull => 76,
            Self::SQLiteLocalCore => 77,
            Self::SQLiteDesktopLocalFull => 78,
            Self::SQLiteMobileLocalFull => 79,
            Self::ConfigurationRuntimeFull => 80,
        })
    }

    #[must_use]
    pub const fn minimum_tier(self) -> CertificationTier {
        match self {
            Self::StorageCore
            | Self::ClientCore
            | Self::ServerCore
            | Self::ProtocolCore
            | Self::Integration
            | Self::StoolapLocalCore
            | Self::SQLiteLocalCore => CertificationTier::CoreTransactional,
            Self::StorageFullSync
            | Self::ClientFullSync
            | Self::Provider
            | Self::MobileClientFull
            | Self::DesktopClientFull
            | Self::DesktopAgentFull
            | Self::MobileLocalStoreFull
            | Self::DesktopLocalStoreFull
            | Self::LocalAdapter
            | Self::AuthoritativeAdapter
            | Self::SnapshotAdapter
            | Self::FencingAdapter
            | Self::PostgresAuthorityFull
            | Self::NeonOperationalProfile
            | Self::StoolapDesktopLocalFull
            | Self::StoolapMobileLocalFull
            | Self::AxumServerFull
            | Self::DioxusClientFull
            | Self::CliToolchainFull
            | Self::SQLiteDesktopLocalFull
            | Self::SQLiteMobileLocalFull
            | Self::ConfigurationRuntimeFull => CertificationTier::FullSync,
            Self::ServerEnterprise => CertificationTier::Enterprise,
        }
    }
}

/// Stable outcome for one test. Skipped and unsupported never count as passing.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum TestStatus {
    Passed,
    Failed,
    Skipped,
    Unsupported,
}

/// Overall certification result.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum CertificationResult {
    Passed,
    PassedWithLimitations,
    Failed,
    Unsupported,
}

/// Evidence method for one normative invariant.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
pub enum VerificationMethod {
    Automated,
    ModelChecked,
    ManualReview,
    DeploymentValidation,
}

/// Trust is about who verified an artifact, not what technical tests mean.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
pub enum TrustLevel {
    Experimental,
    CommunityVerified,
    MaintainerVerified,
    Official,
}

/// Certification lifecycle. Identity is never reused across transitions.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum CertificationStatus {
    Active,
    Superseded,
    Suspended,
    Revoked,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct SubjectIdentity {
    pub name: String,
    pub version: String,
    pub source_commit: String,
    pub binary_digest: String,
    pub enabled_features: BTreeSet<String>,
    pub build_configuration: BTreeMap<String, String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ExecutionEnvironment {
    pub target: String,
    pub operating_system: String,
    pub architecture: String,
    pub rust_version: String,
    pub database_engine: Option<String>,
    pub database_version: Option<String>,
    pub provider_versions: BTreeMap<String, String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TestDefinition {
    pub id: ConformanceTestId,
    pub name: &'static str,
    pub domain: ConformanceDomain,
    pub invariant_ids: &'static [&'static str],
    pub minimum_tier: CertificationTier,
    pub capability: Option<&'static str>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct TestObservation {
    pub test_id: ConformanceTestId,
    pub status: TestStatus,
    pub seed: Option<u64>,
    pub evidence: Vec<String>,
    pub message: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct CapabilityManifest {
    pub claimed: BTreeSet<String>,
    pub verified: BTreeSet<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct InvariantCoverage {
    pub invariant_id: String,
    pub test_ids: BTreeSet<ConformanceTestId>,
    pub methods: BTreeSet<VerificationMethod>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct PerformanceCharacterization {
    pub workload: String,
    pub measurement: String,
    pub environment_note: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct CertificationArtifact {
    pub artifact_format_version: u32,
    pub certification_id: String,
    pub subject: SubjectIdentity,
    pub suite_version: u32,
    pub profile: ConformanceProfile,
    pub tier: CertificationTier,
    pub environment: ExecutionEnvironment,
    pub capability_manifest: CapabilityManifest,
    pub observations: Vec<TestObservation>,
    pub coverage: Vec<InvariantCoverage>,
    pub limitations: Vec<String>,
    pub performance: Vec<PerformanceCharacterization>,
    pub evidence_digest: String,
    pub result: CertificationResult,
    pub issued_at_unix_ms: u64,
}

/// Serializable input consumed by the CLI after a harness collects observations.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct CertificationRequest {
    pub subject: SubjectIdentity,
    pub environment: ExecutionEnvironment,
    pub profile: ConformanceProfile,
    pub tier: CertificationTier,
    pub claimed_capabilities: BTreeSet<String>,
    pub observations: Vec<TestObservation>,
    pub limitations: Vec<String>,
    pub issued_at_unix_ms: u64,
}

impl CertificationRequest {
    /// Evaluates observations and creates an immutable, content-addressed artifact.
    ///
    /// # Errors
    ///
    /// Returns a conformance validation or encoding failure.
    pub fn execute(self) -> Result<CertificationArtifact, ConformanceError> {
        certify(
            self.subject,
            self.environment,
            self.profile,
            self.tier,
            self.claimed_capabilities,
            self.observations,
            self.limitations,
            self.issued_at_unix_ms,
        )
    }
}

impl CertificationArtifact {
    /// Validates exact suite/profile/tier binding and the no-skipped-required-tests rule.
    ///
    /// # Errors
    ///
    /// Returns a stable semantic validation failure.
    pub fn validate(&self) -> Result<(), ConformanceError> {
        validate_identity(&self.subject)?;
        validate_environment(&self.environment)?;
        validate_observations(&self.observations)?;
        validate_capability_manifest(&self.capability_manifest)?;
        validate_bounded_texts(&self.limitations, MAX_LIMITATIONS, "limitations")?;
        validate_performance(&self.performance)?;
        if self.artifact_format_version != ARTIFACT_FORMAT_VERSION
            || self.suite_version != CURRENT_SUITE_VERSION
        {
            return Err(ConformanceError::UnsupportedFormat);
        }
        if (!self.certification_id.is_empty() && !is_digest(&self.certification_id))
            || !is_digest(&self.evidence_digest)
            || self.profile.id().0 == 0
            || self.tier.id().0 == 0
            || (self.tier != CertificationTier::Experimental
                && self.tier < self.profile.minimum_tier())
            || self.evidence_digest != evidence_digest(&self.observations)?
        {
            return Err(ConformanceError::InvalidBinding);
        }
        let definitions = definitions_for(self.profile, self.tier);
        let observations = observation_map(&self.observations)?;
        let mut required_failure = false;
        let mut required_unsupported = false;
        for definition in definitions {
            let observation = observations
                .get(&definition.id)
                .ok_or(ConformanceError::MissingRequiredTest(definition.id))?;
            if observation.status == TestStatus::Passed
                && (observation.evidence.is_empty()
                    || observation
                        .evidence
                        .iter()
                        .any(|evidence| evidence.trim().is_empty()))
            {
                return Err(ConformanceError::InvalidBinding);
            }
            match observation.status {
                TestStatus::Passed => {}
                TestStatus::Unsupported => required_unsupported = true,
                TestStatus::Failed | TestStatus::Skipped => required_failure = true,
            }
        }
        let mut unverified_capability = false;
        if !self
            .capability_manifest
            .verified
            .is_subset(&self.capability_manifest.claimed)
        {
            return Err(ConformanceError::InvalidBinding);
        }
        for capability in &self.capability_manifest.claimed {
            let capability_tests = REFERENCE_TESTS
                .iter()
                .filter(|definition| definition.capability == Some(capability.as_str()))
                .collect::<Vec<_>>();
            if capability_tests.is_empty()
                || capability_tests.iter().any(|definition| {
                    observations.get(&definition.id).map(|value| value.status)
                        != Some(TestStatus::Passed)
                })
                || !self.capability_manifest.verified.contains(capability)
            {
                unverified_capability = true;
            }
        }
        let expected_coverage = coverage_from_observations(&self.observations);
        if self.coverage != expected_coverage {
            return Err(ConformanceError::InvalidBinding);
        }
        let expected = if required_unsupported {
            CertificationResult::Unsupported
        } else if required_failure || unverified_capability {
            CertificationResult::Failed
        } else if self.limitations.is_empty() {
            CertificationResult::Passed
        } else {
            CertificationResult::PassedWithLimitations
        };
        if self.result != expected {
            return Err(ConformanceError::ResultMismatch);
        }
        Ok(())
    }

    /// Computes a deterministic artifact digest with the certification identity blanked.
    ///
    /// # Errors
    ///
    /// Returns an encoding failure.
    pub fn canonical_digest(&self) -> Result<String, ConformanceError> {
        let mut value = self.clone();
        value.certification_id.clear();
        let bytes = postcard::to_stdvec(&value).map_err(|_| ConformanceError::Encoding)?;
        Ok(blake3::hash(&bytes).to_hex().to_string())
    }

    /// Assigns the content-derived certification identity.
    ///
    /// # Errors
    ///
    /// Returns an encoding failure.
    pub fn seal(mut self) -> Result<Self, ConformanceError> {
        self.certification_id.clear();
        self.validate()?;
        self.certification_id = self.canonical_digest()?;
        Ok(self)
    }

    /// Verifies that the immutable identity still matches the artifact contents.
    ///
    /// # Errors
    ///
    /// Returns an identity or semantic validation failure.
    pub fn verify_identity(&self) -> Result<(), ConformanceError> {
        self.validate()?;
        if self.certification_id != self.canonical_digest()? {
            return Err(ConformanceError::ArtifactTampered);
        }
        Ok(())
    }
}

fn validate_identity(subject: &SubjectIdentity) -> Result<(), ConformanceError> {
    if !is_bounded_text(&subject.name, MAX_IDENTITY_TEXT_BYTES, false)
        || !is_bounded_text(&subject.version, MAX_IDENTITY_TEXT_BYTES, false)
        || !is_bounded_text(&subject.source_commit, MAX_IDENTITY_TEXT_BYTES, false)
        || !is_digest(&subject.binary_digest)
        || !valid_set(&subject.enabled_features)
        || !valid_map(&subject.build_configuration)
    {
        return Err(ConformanceError::InvalidSubject);
    }
    Ok(())
}

fn validate_environment(environment: &ExecutionEnvironment) -> Result<(), ConformanceError> {
    if !is_bounded_text(&environment.target, MAX_IDENTITY_TEXT_BYTES, false)
        || !is_bounded_text(
            &environment.operating_system,
            MAX_IDENTITY_TEXT_BYTES,
            false,
        )
        || !is_bounded_text(&environment.architecture, MAX_IDENTITY_TEXT_BYTES, false)
        || !is_bounded_text(&environment.rust_version, MAX_IDENTITY_TEXT_BYTES, false)
        || (environment.database_version.is_some() && environment.database_engine.is_none())
        || environment
            .database_engine
            .as_deref()
            .is_some_and(|value| !is_bounded_text(value, MAX_IDENTITY_TEXT_BYTES, false))
        || environment
            .database_version
            .as_deref()
            .is_some_and(|value| !is_bounded_text(value, MAX_IDENTITY_TEXT_BYTES, false))
        || !valid_map(&environment.provider_versions)
    {
        return Err(ConformanceError::InvalidBinding);
    }
    Ok(())
}

fn validate_observations(observations: &[TestObservation]) -> Result<(), ConformanceError> {
    if observations.len() > REFERENCE_TESTS.len() {
        return Err(ConformanceError::LimitExceeded("observations"));
    }
    for observation in observations {
        if !REFERENCE_TESTS
            .iter()
            .any(|definition| definition.id == observation.test_id)
        {
            return Err(ConformanceError::UnknownTest(observation.test_id));
        }
        if observation.evidence.len() > MAX_EVIDENCE_REFERENCES_PER_TEST {
            return Err(ConformanceError::LimitExceeded("test evidence"));
        }
        if !is_bounded_text(&observation.message, MAX_OBSERVATION_MESSAGE_BYTES, false)
            || observation
                .evidence
                .iter()
                .any(|value| !is_bounded_text(value, MAX_EVIDENCE_REFERENCE_BYTES, false))
        {
            return Err(ConformanceError::InvalidBinding);
        }
    }
    Ok(())
}

fn validate_capability_manifest(manifest: &CapabilityManifest) -> Result<(), ConformanceError> {
    if !valid_set(&manifest.claimed) || !valid_set(&manifest.verified) {
        return Err(ConformanceError::InvalidBinding);
    }
    Ok(())
}

fn validate_performance(
    performance: &[PerformanceCharacterization],
) -> Result<(), ConformanceError> {
    if performance.len() > MAX_PERFORMANCE_CHARACTERIZATIONS {
        return Err(ConformanceError::LimitExceeded(
            "performance characterizations",
        ));
    }
    if performance.iter().any(|value| {
        !is_bounded_text(&value.workload, MAX_IDENTITY_TEXT_BYTES, false)
            || !is_bounded_text(&value.measurement, MAX_IDENTITY_TEXT_BYTES, false)
            || !is_bounded_text(&value.environment_note, MAX_IDENTITY_TEXT_BYTES, false)
    }) {
        return Err(ConformanceError::InvalidBinding);
    }
    Ok(())
}

fn validate_bounded_texts(
    values: &[String],
    maximum_entries: usize,
    field: &'static str,
) -> Result<(), ConformanceError> {
    if values.len() > maximum_entries {
        return Err(ConformanceError::LimitExceeded(field));
    }
    if values
        .iter()
        .any(|value| !is_bounded_text(value, MAX_EVIDENCE_REFERENCE_BYTES, false))
    {
        return Err(ConformanceError::InvalidBinding);
    }
    Ok(())
}

fn valid_set(values: &BTreeSet<String>) -> bool {
    values.len() <= MAX_METADATA_ENTRIES
        && values
            .iter()
            .all(|value| is_bounded_text(value, MAX_IDENTITY_TEXT_BYTES, false))
}

fn valid_map(values: &BTreeMap<String, String>) -> bool {
    values.len() <= MAX_METADATA_ENTRIES
        && values.iter().all(|(key, value)| {
            is_bounded_text(key, MAX_IDENTITY_TEXT_BYTES, false)
                && is_bounded_text(value, MAX_IDENTITY_TEXT_BYTES, false)
        })
}

fn is_bounded_text(value: &str, maximum: usize, allow_empty: bool) -> bool {
    value.len() <= maximum
        && (allow_empty || !value.trim().is_empty())
        && value.chars().all(|character| !character.is_control())
}

fn is_digest(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn observation_map(
    observations: &[TestObservation],
) -> Result<BTreeMap<ConformanceTestId, &TestObservation>, ConformanceError> {
    let mut output = BTreeMap::new();
    for observation in observations {
        if output.insert(observation.test_id, observation).is_some() {
            return Err(ConformanceError::DuplicateTest(observation.test_id));
        }
    }
    Ok(output)
}

/// Produces a certification artifact from externally measured observations.
///
/// # Errors
///
/// Returns an error when required observations or capability evidence are invalid.
#[allow(clippy::too_many_arguments)]
pub fn certify(
    subject: SubjectIdentity,
    environment: ExecutionEnvironment,
    profile: ConformanceProfile,
    tier: CertificationTier,
    claimed_capabilities: BTreeSet<String>,
    observations: Vec<TestObservation>,
    limitations: Vec<String>,
    issued_at_unix_ms: u64,
) -> Result<CertificationArtifact, ConformanceError> {
    let observation_by_id = observation_map(&observations)?;
    let verified = claimed_capabilities
        .iter()
        .filter(|capability| {
            let tests = REFERENCE_TESTS
                .iter()
                .filter(|definition| definition.capability == Some(capability.as_str()))
                .collect::<Vec<_>>();
            !tests.is_empty()
                && tests.iter().all(|definition| {
                    observation_by_id
                        .get(&definition.id)
                        .map(|value| value.status)
                        == Some(TestStatus::Passed)
                })
        })
        .cloned()
        .collect();
    let coverage = coverage_from_observations(&observations);
    let result = result_for(profile, tier, &observations, &limitations);
    let result = if verified == claimed_capabilities {
        result
    } else {
        CertificationResult::Failed
    };
    let evidence_digest = evidence_digest(&observations)?;
    let artifact = CertificationArtifact {
        artifact_format_version: ARTIFACT_FORMAT_VERSION,
        certification_id: String::new(),
        subject,
        suite_version: CURRENT_SUITE_VERSION,
        profile,
        tier,
        environment,
        capability_manifest: CapabilityManifest {
            claimed: claimed_capabilities,
            verified,
        },
        observations,
        coverage,
        limitations,
        performance: Vec::new(),
        evidence_digest,
        result,
        issued_at_unix_ms,
    };
    artifact.validate()?;
    artifact.seal()
}

fn result_for(
    profile: ConformanceProfile,
    tier: CertificationTier,
    observations: &[TestObservation],
    limitations: &[String],
) -> CertificationResult {
    let map = observations
        .iter()
        .map(|value| (value.test_id, value.status))
        .collect::<BTreeMap<_, _>>();
    let statuses = definitions_for(profile, tier)
        .into_iter()
        .map(|definition| map.get(&definition.id).copied());
    let mut unsupported = false;
    let mut failed = false;
    for status in statuses {
        match status {
            Some(TestStatus::Passed) => {}
            Some(TestStatus::Unsupported) => unsupported = true,
            Some(TestStatus::Failed | TestStatus::Skipped) | None => failed = true,
        }
    }
    if unsupported {
        CertificationResult::Unsupported
    } else if failed {
        CertificationResult::Failed
    } else if limitations.is_empty() {
        CertificationResult::Passed
    } else {
        CertificationResult::PassedWithLimitations
    }
}

fn evidence_digest(observations: &[TestObservation]) -> Result<String, ConformanceError> {
    let bytes = postcard::to_stdvec(observations).map_err(|_| ConformanceError::Encoding)?;
    Ok(blake3::hash(&bytes).to_hex().to_string())
}

fn coverage_from_observations(observations: &[TestObservation]) -> Vec<InvariantCoverage> {
    let present = observations
        .iter()
        .map(|value| value.test_id)
        .collect::<BTreeSet<_>>();
    let mut coverage = BTreeMap::<String, BTreeSet<ConformanceTestId>>::new();
    for definition in REFERENCE_TESTS {
        if present.contains(&definition.id) {
            for invariant in definition.invariant_ids {
                coverage
                    .entry((*invariant).to_owned())
                    .or_default()
                    .insert(definition.id);
            }
        }
    }
    coverage
        .into_iter()
        .map(|(invariant_id, test_ids)| InvariantCoverage {
            invariant_id,
            test_ids,
            methods: BTreeSet::from([VerificationMethod::Automated]),
        })
        .collect()
}

#[must_use]
pub fn definitions_for(
    profile: ConformanceProfile,
    tier: CertificationTier,
) -> Vec<&'static TestDefinition> {
    REFERENCE_TESTS
        .iter()
        .filter(|definition| definition.minimum_tier <= tier)
        .filter(|definition| definition_in_profile(definition, profile))
        .collect()
}

const fn definition_in_profile(definition: &TestDefinition, profile: ConformanceProfile) -> bool {
    match profile {
        ConformanceProfile::LocalAdapter => matches!(
            definition.id.0,
            1 | 4 | 39..=50 | 52..=58
        ),
        ConformanceProfile::AuthoritativeAdapter => {
            matches!(definition.id.0, 2 | 3 | 5 | 7 | 49 | 51..=58)
        }
        ConformanceProfile::SnapshotAdapter => {
            matches!(definition.id.0, 6 | 49 | 52 | 55 | 56 | 58)
        }
        ConformanceProfile::FencingAdapter => {
            matches!(definition.id.0, 5 | 49 | 52 | 55 | 56 | 58)
        }
        ConformanceProfile::PostgresAuthorityFull => {
            matches!(definition.id.0, 2 | 3 | 5 | 7 | 49 | 51..=68)
        }
        ConformanceProfile::NeonOperationalProfile => {
            matches!(definition.id.0, 2 | 3 | 5 | 7 | 49 | 51..=68)
        }
        ConformanceProfile::StoolapLocalCore => {
            matches!(definition.id.0, 1 | 4 | 39..=58 | 69..=73 | 77)
        }
        ConformanceProfile::StoolapDesktopLocalFull
        | ConformanceProfile::StoolapMobileLocalFull => {
            matches!(definition.id.0, 1 | 4 | 39..=58 | 69..=78)
        }
        ConformanceProfile::AxumServerFull => matches!(definition.id.0, 79..=88),
        ConformanceProfile::DioxusClientFull => matches!(definition.id.0, 89..=98),
        ConformanceProfile::CliToolchainFull => matches!(definition.id.0, 99..=108),
        ConformanceProfile::SQLiteLocalCore => matches!(definition.id.0, 109..=112),
        ConformanceProfile::SQLiteDesktopLocalFull | ConformanceProfile::SQLiteMobileLocalFull => {
            matches!(definition.id.0, 109..=113)
        }
        ConformanceProfile::ConfigurationRuntimeFull => matches!(definition.id.0, 114..=123),
        _ => definition.id.0 < 49 && domain_in_profile(definition.domain, profile),
    }
}

const fn domain_in_profile(domain: ConformanceDomain, profile: ConformanceProfile) -> bool {
    match profile {
        ConformanceProfile::StorageCore | ConformanceProfile::StorageFullSync => {
            matches!(
                domain,
                ConformanceDomain::StorageAdapter | ConformanceDomain::SnapshotProvider
            )
        }
        ConformanceProfile::ClientCore | ConformanceProfile::ClientFullSync => {
            matches!(
                domain,
                ConformanceDomain::ClientRuntime | ConformanceDomain::ProtocolImplementation
            )
        }
        ConformanceProfile::ServerCore | ConformanceProfile::ServerEnterprise => matches!(
            domain,
            ConformanceDomain::ServerRuntime
                | ConformanceDomain::StorageAdapter
                | ConformanceDomain::SnapshotProvider
                | ConformanceDomain::CryptoProvider
                | ConformanceDomain::JobSideEffectProvider
                | ConformanceDomain::ChangeFeedConsumer
        ),
        ConformanceProfile::ProtocolCore => {
            matches!(domain, ConformanceDomain::ProtocolImplementation)
        }
        ConformanceProfile::Provider => matches!(
            domain,
            ConformanceDomain::SnapshotProvider
                | ConformanceDomain::CryptoProvider
                | ConformanceDomain::JobSideEffectProvider
                | ConformanceDomain::ChangeFeedConsumer
        ),
        ConformanceProfile::Integration => matches!(
            domain,
            ConformanceDomain::LegacyBridge | ConformanceDomain::ExtensionApplication
        ),
        ConformanceProfile::MobileClientFull => matches!(
            domain,
            ConformanceDomain::MobileRuntime
                | ConformanceDomain::ClientRuntime
                | ConformanceDomain::ProtocolImplementation
        ),
        ConformanceProfile::DesktopClientFull => matches!(
            domain,
            ConformanceDomain::DesktopRuntime
                | ConformanceDomain::ClientRuntime
                | ConformanceDomain::ProtocolImplementation
        ),
        ConformanceProfile::DesktopAgentFull => {
            matches!(
                domain,
                ConformanceDomain::DesktopRuntime | ConformanceDomain::ProtocolImplementation
            )
        }
        ConformanceProfile::MobileLocalStoreFull | ConformanceProfile::DesktopLocalStoreFull => {
            matches!(domain, ConformanceDomain::StorageAdapter)
        }
        ConformanceProfile::LocalAdapter
        | ConformanceProfile::AuthoritativeAdapter
        | ConformanceProfile::SnapshotAdapter
        | ConformanceProfile::FencingAdapter
        | ConformanceProfile::PostgresAuthorityFull
        | ConformanceProfile::NeonOperationalProfile
        | ConformanceProfile::StoolapLocalCore
        | ConformanceProfile::StoolapDesktopLocalFull
        | ConformanceProfile::StoolapMobileLocalFull
        | ConformanceProfile::AxumServerFull
        | ConformanceProfile::DioxusClientFull
        | ConformanceProfile::CliToolchainFull
        | ConformanceProfile::SQLiteLocalCore
        | ConformanceProfile::SQLiteDesktopLocalFull
        | ConformanceProfile::SQLiteMobileLocalFull
        | ConformanceProfile::ConfigurationRuntimeFull => false,
    }
}

macro_rules! test_definition {
    ($id:literal, $name:literal, $domain:ident, $invariant:literal, $tier:ident, $capability:expr) => {
        TestDefinition {
            id: ConformanceTestId($id),
            name: $name,
            domain: ConformanceDomain::$domain,
            invariant_ids: &[$invariant],
            minimum_tier: CertificationTier::$tier,
            capability: $capability,
        }
    };
}

/// Stable reference suite. Test IDs are mirrored in the Part 29 registry.
pub static REFERENCE_TESTS: &[TestDefinition] = &[
    test_definition!(
        1,
        "local_intent_atomicity",
        StorageAdapter,
        "AEQ-INV-002",
        CoreTransactional,
        None
    ),
    test_definition!(
        2,
        "authoritative_publication_atomicity",
        StorageAdapter,
        "AEQ-INV-003",
        CoreTransactional,
        None
    ),
    test_definition!(
        3,
        "idempotent_authority",
        StorageAdapter,
        "AEQ-INV-001",
        CoreTransactional,
        None
    ),
    test_definition!(
        4,
        "cursor_after_durable_apply",
        StorageAdapter,
        "AEQ-INV-004",
        FullSync,
        Some("durable-cursors")
    ),
    test_definition!(
        5,
        "authority_fencing",
        StorageAdapter,
        "AEQ-INV-031",
        FullSync,
        Some("authority-fencing")
    ),
    test_definition!(
        6,
        "snapshot_round_trip",
        SnapshotProvider,
        "AEQ-INV-067",
        FullSync,
        Some("snapshots")
    ),
    test_definition!(
        7,
        "tombstone_and_anti_entropy",
        StorageAdapter,
        "AEQ-INV-009",
        FullSync,
        Some("anti-entropy")
    ),
    test_definition!(
        8,
        "governance_restore",
        SnapshotProvider,
        "AEQ-INV-145",
        Enterprise,
        Some("governance-restore")
    ),
    test_definition!(
        9,
        "protocol_unknown_id_fail_closed",
        ProtocolImplementation,
        "AEQ-INV-CERT001",
        CoreTransactional,
        None
    ),
    test_definition!(
        10,
        "protocol_differential_trace",
        ProtocolImplementation,
        "AEQ-INV-CERT001",
        FullSync,
        Some("differential-protocol")
    ),
    test_definition!(
        11,
        "client_offline_replay",
        ClientRuntime,
        "AEQ-INV-007",
        CoreTransactional,
        None
    ),
    test_definition!(
        12,
        "client_resource_bounds",
        ClientRuntime,
        "AEQ-INV-RESOURCE001",
        FullSync,
        Some("bounded-client")
    ),
    test_definition!(
        13,
        "server_failover_fencing",
        ServerRuntime,
        "AEQ-INV-AUTH001",
        FullSync,
        Some("authority-fencing")
    ),
    test_definition!(
        14,
        "server_security_fail_closed",
        ServerRuntime,
        "AEQ-INV-SEC001",
        Enterprise,
        Some("enterprise-security")
    ),
    test_definition!(
        15,
        "crypto_known_answer_and_misuse",
        CryptoProvider,
        "AEQ-INV-CRYPTO001",
        Enterprise,
        Some("managed-crypto")
    ),
    test_definition!(
        16,
        "job_exactly_once_effect",
        JobSideEffectProvider,
        "AEQ-INV-JOB001",
        Enterprise,
        Some("durable-effects")
    ),
    test_definition!(
        17,
        "feed_cursor_and_duplicate_safety",
        ChangeFeedConsumer,
        "AEQ-INV-FEED001",
        Enterprise,
        Some("change-feed")
    ),
    test_definition!(
        18,
        "legacy_shadow_and_cutover",
        LegacyBridge,
        "AEQ-INV-LEGACY001",
        Enterprise,
        Some("legacy-bridge")
    ),
    test_definition!(
        19,
        "extension_namespace_isolation",
        ExtensionApplication,
        "AEQ-INV-REG006",
        CoreTransactional,
        Some("extension-registry")
    ),
    test_definition!(
        20,
        "application_capability_truthfulness",
        ExtensionApplication,
        "AEQ-INV-CERT004",
        CoreTransactional,
        None
    ),
    test_definition!(
        21,
        "mobile_process_death_recovery",
        MobileRuntime,
        "AEQ-INV-MOBILE001",
        FullSync,
        Some("mobile-process-recovery")
    ),
    test_definition!(
        22,
        "mobile_atomic_local_outbox",
        MobileRuntime,
        "AEQ-INV-MOBILE002",
        FullSync,
        Some("mobile-atomic-outbox")
    ),
    test_definition!(
        23,
        "mobile_push_loss_and_duplication",
        MobileRuntime,
        "AEQ-INV-MOBILE003",
        FullSync,
        Some("mobile-push-hints")
    ),
    test_definition!(
        24,
        "mobile_background_cursor_checkpoint",
        MobileRuntime,
        "AEQ-INV-MOBILE004",
        FullSync,
        Some("mobile-background-checkpoint")
    ),
    test_definition!(
        25,
        "mobile_secure_store_integration",
        MobileRuntime,
        "AEQ-INV-MOBILE005",
        FullSync,
        Some("mobile-secure-store")
    ),
    test_definition!(
        26,
        "mobile_resource_semantic_parity",
        MobileRuntime,
        "AEQ-INV-MOBILE006",
        FullSync,
        Some("mobile-resource-policy")
    ),
    test_definition!(
        27,
        "mobile_upgrade_intent_preservation",
        MobileRuntime,
        "AEQ-INV-MOBILE007",
        FullSync,
        Some("mobile-upgrade")
    ),
    test_definition!(
        28,
        "mobile_storage_failure_truthfulness",
        MobileRuntime,
        "AEQ-INV-MOBILE008",
        FullSync,
        Some("mobile-storage-pressure")
    ),
    test_definition!(
        29,
        "mobile_binding_boundary",
        MobileRuntime,
        "AEQ-INV-MOBILE009",
        FullSync,
        Some("mobile-bindings")
    ),
    test_definition!(
        30,
        "desktop_single_coordinator",
        DesktopRuntime,
        "AEQ-INV-DESKTOP001",
        FullSync,
        Some("desktop-coordination")
    ),
    test_definition!(
        31,
        "desktop_stale_fence_rejection",
        DesktopRuntime,
        "AEQ-INV-DESKTOP002",
        FullSync,
        Some("desktop-fencing")
    ),
    test_definition!(
        32,
        "desktop_sleep_resume_idempotency",
        DesktopRuntime,
        "AEQ-INV-DESKTOP003",
        FullSync,
        Some("desktop-resume")
    ),
    test_definition!(
        33,
        "desktop_ipc_domain_boundary",
        DesktopRuntime,
        "AEQ-INV-DESKTOP004",
        FullSync,
        Some("desktop-ipc")
    ),
    test_definition!(
        34,
        "desktop_clone_rebinding",
        DesktopRuntime,
        "AEQ-INV-DESKTOP005",
        FullSync,
        Some("desktop-device-binding")
    ),
    test_definition!(
        35,
        "desktop_upgrade_intent_preservation",
        DesktopRuntime,
        "AEQ-INV-DESKTOP006",
        FullSync,
        Some("desktop-update")
    ),
    test_definition!(
        36,
        "desktop_mode_semantic_parity",
        DesktopRuntime,
        "AEQ-INV-DESKTOP007",
        FullSync,
        Some("desktop-mode-parity")
    ),
    test_definition!(
        37,
        "desktop_persistence_failure_truthfulness",
        DesktopRuntime,
        "AEQ-INV-DESKTOP008",
        FullSync,
        Some("desktop-persistence")
    ),
    test_definition!(
        38,
        "desktop_derived_state_non_authority",
        DesktopRuntime,
        "AEQ-INV-DESKTOP009",
        FullSync,
        Some("desktop-derived-state")
    ),
    test_definition!(
        39,
        "storage_atomic_local_intent",
        StorageAdapter,
        "AEQ-INV-STORAGE001",
        FullSync,
        Some("storage-atomicity")
    ),
    test_definition!(
        40,
        "storage_critical_intent_retention",
        StorageAdapter,
        "AEQ-INV-STORAGE002",
        FullSync,
        Some("storage-pressure")
    ),
    test_definition!(
        41,
        "storage_cache_purge_isolation",
        StorageAdapter,
        "AEQ-INV-STORAGE003",
        FullSync,
        Some("storage-cache")
    ),
    test_definition!(
        42,
        "storage_clone_binding",
        StorageAdapter,
        "AEQ-INV-STORAGE004",
        FullSync,
        Some("storage-restore")
    ),
    test_definition!(
        43,
        "storage_format_downgrade",
        StorageAdapter,
        "AEQ-INV-STORAGE005",
        FullSync,
        Some("storage-migration")
    ),
    test_definition!(
        44,
        "storage_platform_certification",
        StorageAdapter,
        "AEQ-INV-STORAGE006",
        FullSync,
        Some("storage-platform")
    ),
    test_definition!(
        45,
        "storage_verified_publication",
        StorageAdapter,
        "AEQ-INV-STORAGE007",
        FullSync,
        Some("storage-publication")
    ),
    test_definition!(
        46,
        "storage_low_disk_truthfulness",
        StorageAdapter,
        "AEQ-INV-STORAGE008",
        FullSync,
        Some("storage-admission")
    ),
    test_definition!(
        47,
        "storage_secret_isolation",
        StorageAdapter,
        "AEQ-INV-STORAGE009",
        FullSync,
        Some("storage-encryption")
    ),
    test_definition!(
        48,
        "storage_filesystem_placement",
        StorageAdapter,
        "AEQ-INV-STORAGE010",
        FullSync,
        Some("storage-layout")
    ),
    test_definition!(
        49,
        "adapter_capability_conformance",
        StorageAdapter,
        "AEQ-INV-ADAPTER001",
        FullSync,
        Some("adapter-capability-evidence")
    ),
    test_definition!(
        50,
        "adapter_atomic_local_outbox",
        StorageAdapter,
        "AEQ-INV-ADAPTER002",
        FullSync,
        Some("atomic-local-outbox")
    ),
    test_definition!(
        51,
        "adapter_atomic_authority_commit",
        StorageAdapter,
        "AEQ-INV-ADAPTER003",
        FullSync,
        Some("atomic-authority-commit")
    ),
    test_definition!(
        52,
        "adapter_neutral_error_boundary",
        StorageAdapter,
        "AEQ-INV-ADAPTER004",
        FullSync,
        Some("neutral-adapter-api")
    ),
    test_definition!(
        53,
        "adapter_payload_reuse_rejection",
        StorageAdapter,
        "AEQ-INV-ADAPTER005",
        FullSync,
        Some("operation-payload-binding")
    ),
    test_definition!(
        54,
        "adapter_migration_preservation",
        StorageAdapter,
        "AEQ-INV-ADAPTER006",
        FullSync,
        Some("adapter-migrations")
    ),
    test_definition!(
        55,
        "adapter_startup_fail_closed",
        StorageAdapter,
        "AEQ-INV-ADAPTER007",
        FullSync,
        Some("adapter-startup-validation")
    ),
    test_definition!(
        56,
        "adapter_environment_binding",
        StorageAdapter,
        "AEQ-INV-ADAPTER008",
        FullSync,
        Some("adapter-environment-binding")
    ),
    test_definition!(
        57,
        "adapter_critical_durability",
        StorageAdapter,
        "AEQ-INV-ADAPTER009",
        FullSync,
        Some("critical-intent-durability")
    ),
    test_definition!(
        58,
        "adapter_manifest_and_limitations",
        StorageAdapter,
        "AEQ-INV-ADAPTER010",
        FullSync,
        Some("adapter-support-matrix")
    ),
    test_definition!(
        59,
        "postgres_tx_b_atomicity",
        StorageAdapter,
        "AEQ-INV-PG001",
        FullSync,
        Some("postgres-authority-full")
    ),
    test_definition!(
        60,
        "postgres_idempotent_replay",
        StorageAdapter,
        "AEQ-INV-PG002",
        FullSync,
        Some("postgres-operation-ledger")
    ),
    test_definition!(
        61,
        "postgres_payload_binding",
        StorageAdapter,
        "AEQ-INV-PG003",
        FullSync,
        Some("postgres-operation-ledger")
    ),
    test_definition!(
        62,
        "postgres_committed_timeline",
        StorageAdapter,
        "AEQ-INV-PG004",
        FullSync,
        Some("postgres-transactional-timeline")
    ),
    test_definition!(
        63,
        "postgres_restore_epoch",
        StorageAdapter,
        "AEQ-INV-PG005",
        FullSync,
        Some("postgres-restore-epoch")
    ),
    test_definition!(
        64,
        "postgres_type_isolation",
        StorageAdapter,
        "AEQ-INV-PG006",
        FullSync,
        Some("postgres-adapter-boundary")
    ),
    test_definition!(
        65,
        "postgres_readiness_settings",
        StorageAdapter,
        "AEQ-INV-PG007",
        FullSync,
        Some("postgres-readiness")
    ),
    test_definition!(
        66,
        "postgres_durable_side_effect_intent",
        StorageAdapter,
        "AEQ-INV-PG008",
        FullSync,
        Some("postgres-side-effect-outbox")
    ),
    test_definition!(
        67,
        "postgres_retention_floor",
        StorageAdapter,
        "AEQ-INV-PG009",
        FullSync,
        Some("postgres-safe-retention")
    ),
    test_definition!(
        68,
        "neon_semantic_parity",
        StorageAdapter,
        "AEQ-INV-PG010",
        FullSync,
        Some("neon-operational-profile")
    ),
    test_definition!(
        69,
        "stoolap_tx_a_atomicity",
        StorageAdapter,
        "AEQ-INV-STOOLAP001",
        CoreTransactional,
        Some("stoolap-local-core")
    ),
    test_definition!(
        70,
        "stoolap_tx_c_atomicity",
        StorageAdapter,
        "AEQ-INV-STOOLAP002",
        CoreTransactional,
        Some("stoolap-local-core")
    ),
    test_definition!(
        71,
        "stoolap_retry_identity",
        StorageAdapter,
        "AEQ-INV-STOOLAP003",
        CoreTransactional,
        Some("stoolap-digest-bound-outbox")
    ),
    test_definition!(
        72,
        "stoolap_pending_intent_preservation",
        StorageAdapter,
        "AEQ-INV-STOOLAP004",
        CoreTransactional,
        Some("stoolap-pending-intent")
    ),
    test_definition!(
        73,
        "stoolap_cursor_safety",
        StorageAdapter,
        "AEQ-INV-STOOLAP005",
        CoreTransactional,
        Some("stoolap-local-core")
    ),
    test_definition!(
        74,
        "stoolap_fenced_coordinator",
        StorageAdapter,
        "AEQ-INV-STOOLAP006",
        FullSync,
        Some("stoolap-fencing")
    ),
    test_definition!(
        75,
        "stoolap_clone_rebinding",
        StorageAdapter,
        "AEQ-INV-STOOLAP007",
        FullSync,
        Some("stoolap-device-binding")
    ),
    test_definition!(
        76,
        "stoolap_storage_pressure",
        StorageAdapter,
        "AEQ-INV-STOOLAP008",
        FullSync,
        Some("stoolap-storage-preflight")
    ),
    test_definition!(
        77,
        "stoolap_type_isolation",
        StorageAdapter,
        "AEQ-INV-STOOLAP009",
        CoreTransactional,
        Some("stoolap-adapter-boundary")
    ),
    test_definition!(
        78,
        "stoolap_platform_certification",
        StorageAdapter,
        "AEQ-INV-STOOLAP010",
        FullSync,
        Some("stoolap-platform-profile")
    ),
    test_definition!(
        79,
        "axum_transport_isolation",
        ServerRuntime,
        "AEQ-INV-AXUM001",
        FullSync,
        Some("axum-server-full")
    ),
    test_definition!(
        80,
        "axum_credential_isolation",
        ServerRuntime,
        "AEQ-INV-AXUM002",
        FullSync,
        Some("axum-server-full")
    ),
    test_definition!(
        81,
        "axum_resource_bounds",
        ServerRuntime,
        "AEQ-INV-AXUM003",
        FullSync,
        Some("axum-server-full")
    ),
    test_definition!(
        82,
        "axum_commit_delivery_independence",
        ServerRuntime,
        "AEQ-INV-AXUM004",
        FullSync,
        Some("axum-server-full")
    ),
    test_definition!(
        83,
        "axum_overload_bounds",
        ServerRuntime,
        "AEQ-INV-AXUM005",
        FullSync,
        Some("axum-server-full")
    ),
    test_definition!(
        84,
        "axum_identity_binding",
        ServerRuntime,
        "AEQ-INV-AXUM006",
        FullSync,
        Some("axum-server-full")
    ),
    test_definition!(
        85,
        "axum_stable_semantics",
        ServerRuntime,
        "AEQ-INV-AXUM007",
        FullSync,
        Some("axum-server-full")
    ),
    test_definition!(
        86,
        "axum_live_hint_durability",
        ServerRuntime,
        "AEQ-INV-AXUM008",
        FullSync,
        Some("axum-server-full")
    ),
    test_definition!(
        87,
        "axum_node_epoch_independence",
        ServerRuntime,
        "AEQ-INV-AXUM009",
        FullSync,
        Some("axum-server-full")
    ),
    test_definition!(
        88,
        "axum_error_sanitization",
        ServerRuntime,
        "AEQ-INV-AXUM010",
        FullSync,
        Some("axum-server-full")
    ),
    test_definition!(
        89,
        "dioxus_durable_state_ownership",
        ClientRuntime,
        "AEQ-INV-DIOXUS001",
        FullSync,
        Some("dioxus-client-full")
    ),
    test_definition!(
        90,
        "dioxus_local_commit_truth",
        ClientRuntime,
        "AEQ-INV-DIOXUS002",
        FullSync,
        Some("dioxus-client-full")
    ),
    test_definition!(
        91,
        "dioxus_authority_distinction",
        ClientRuntime,
        "AEQ-INV-DIOXUS003",
        FullSync,
        Some("dioxus-client-full")
    ),
    test_definition!(
        92,
        "dioxus_event_loss_safety",
        ClientRuntime,
        "AEQ-INV-DIOXUS004",
        FullSync,
        Some("dioxus-client-full")
    ),
    test_definition!(
        93,
        "dioxus_unmount_safety",
        ClientRuntime,
        "AEQ-INV-DIOXUS005",
        FullSync,
        Some("dioxus-client-full")
    ),
    test_definition!(
        94,
        "dioxus_store_isolation",
        ClientRuntime,
        "AEQ-INV-DIOXUS006",
        FullSync,
        Some("dioxus-client-full")
    ),
    test_definition!(
        95,
        "dioxus_conflict_semantics",
        ClientRuntime,
        "AEQ-INV-DIOXUS007",
        FullSync,
        Some("dioxus-client-full")
    ),
    test_definition!(
        96,
        "dioxus_offline_capability",
        ClientRuntime,
        "AEQ-INV-DIOXUS008",
        FullSync,
        Some("dioxus-client-full")
    ),
    test_definition!(
        97,
        "dioxus_background_ownership",
        ClientRuntime,
        "AEQ-INV-DIOXUS009",
        FullSync,
        Some("dioxus-client-full")
    ),
    test_definition!(
        98,
        "dioxus_event_bounds",
        ClientRuntime,
        "AEQ-INV-DIOXUS010",
        FullSync,
        Some("dioxus-client-full")
    ),
    test_definition!(
        99,
        "cli_no_invariant_bypass",
        DeveloperTooling,
        "AEQ-INV-CLI001",
        FullSync,
        Some("cli-toolchain-full")
    ),
    test_definition!(
        100,
        "cli_machine_output_versioning",
        DeveloperTooling,
        "AEQ-INV-CLI002",
        FullSync,
        Some("cli-toolchain-full")
    ),
    test_definition!(
        101,
        "cli_sensitive_redaction",
        DeveloperTooling,
        "AEQ-INV-CLI003",
        FullSync,
        Some("cli-toolchain-full")
    ),
    test_definition!(
        102,
        "cli_plan_apply_safety",
        DeveloperTooling,
        "AEQ-INV-CLI004",
        FullSync,
        Some("cli-toolchain-full")
    ),
    test_definition!(
        103,
        "cli_submission_ambiguity",
        DeveloperTooling,
        "AEQ-INV-CLI005",
        FullSync,
        Some("cli-toolchain-full")
    ),
    test_definition!(
        104,
        "cli_store_ownership",
        DeveloperTooling,
        "AEQ-INV-CLI006",
        FullSync,
        Some("cli-toolchain-full")
    ),
    test_definition!(
        105,
        "cli_migration_safety",
        DeveloperTooling,
        "AEQ-INV-CLI007",
        FullSync,
        Some("cli-toolchain-full")
    ),
    test_definition!(
        106,
        "cli_semantic_mutation",
        DeveloperTooling,
        "AEQ-INV-CLI008",
        FullSync,
        Some("cli-toolchain-full")
    ),
    test_definition!(
        107,
        "cli_production_guard",
        DeveloperTooling,
        "AEQ-INV-CLI009",
        FullSync,
        Some("cli-toolchain-full")
    ),
    test_definition!(
        108,
        "cli_canonical_semantics",
        DeveloperTooling,
        "AEQ-INV-CLI010",
        FullSync,
        Some("cli-toolchain-full")
    ),
    test_definition!(
        109,
        "sqlite_wal_durability",
        StorageAdapter,
        "AEQ-INV-SQLITE001",
        CoreTransactional,
        Some("sqlite-local-core")
    ),
    test_definition!(
        110,
        "sqlite_single_writer",
        StorageAdapter,
        "AEQ-INV-SQLITE002",
        CoreTransactional,
        Some("sqlite-local-core")
    ),
    test_definition!(
        111,
        "sqlite_cursor_atomicity",
        StorageAdapter,
        "AEQ-INV-SQLITE003",
        CoreTransactional,
        Some("sqlite-local-core")
    ),
    test_definition!(
        112,
        "sqlite_outbox_preservation",
        StorageAdapter,
        "AEQ-INV-SQLITE004",
        CoreTransactional,
        Some("sqlite-local-core")
    ),
    test_definition!(
        113,
        "sqlite_adapter_parity",
        StorageAdapter,
        "AEQ-INV-SQLITE005",
        FullSync,
        Some("sqlite-local-full")
    ),
    test_definition!(
        114,
        "config_correctness_preservation",
        ServerRuntime,
        "AEQ-INV-CONFIG001",
        FullSync,
        Some("configuration-runtime-full")
    ),
    test_definition!(
        115,
        "config_secret_redaction",
        ServerRuntime,
        "AEQ-INV-CONFIG002",
        FullSync,
        Some("configuration-runtime-full")
    ),
    test_definition!(
        116,
        "config_validated_publication",
        ServerRuntime,
        "AEQ-INV-CONFIG003",
        FullSync,
        Some("configuration-runtime-full")
    ),
    test_definition!(
        117,
        "config_atomic_reload",
        ServerRuntime,
        "AEQ-INV-CONFIG004",
        FullSync,
        Some("configuration-runtime-full")
    ),
    test_definition!(
        118,
        "config_durable_identity_isolation",
        ServerRuntime,
        "AEQ-INV-CONFIG005",
        FullSync,
        Some("configuration-runtime-full")
    ),
    test_definition!(
        119,
        "config_feature_semantic_safety",
        ServerRuntime,
        "AEQ-INV-CONFIG006",
        FullSync,
        Some("configuration-runtime-full")
    ),
    test_definition!(
        120,
        "config_production_safety",
        ServerRuntime,
        "AEQ-INV-CONFIG007",
        FullSync,
        Some("configuration-runtime-full")
    ),
    test_definition!(
        121,
        "config_adapter_capability_safety",
        StorageAdapter,
        "AEQ-INV-CONFIG008",
        FullSync,
        Some("configuration-runtime-full")
    ),
    test_definition!(
        122,
        "config_failed_reload_preservation",
        ServerRuntime,
        "AEQ-INV-CONFIG009",
        FullSync,
        Some("configuration-runtime-full")
    ),
    test_definition!(
        123,
        "config_authoritative_policy",
        ServerRuntime,
        "AEQ-INV-CONFIG010",
        FullSync,
        Some("configuration-runtime-full")
    ),
];

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct EvidenceFile {
    pub path: String,
    pub digest: String,
    pub bytes: u64,
}

impl EvidenceFile {
    /// Creates a manifest entry from payload bytes without retaining those bytes in memory.
    #[must_use]
    pub fn from_bytes(path: impl Into<String>, bytes: &[u8]) -> Self {
        Self {
            path: path.into(),
            digest: blake3::hash(bytes).to_hex().to_string(),
            bytes: u64::try_from(bytes.len()).unwrap_or(u64::MAX),
        }
    }

    /// Verifies bytes loaded by a host against this bounded manifest entry.
    #[must_use]
    pub fn verifies(&self, bytes: &[u8]) -> bool {
        self.bytes == u64::try_from(bytes.len()).unwrap_or(u64::MAX)
            && self.digest == blake3::hash(bytes).to_hex().to_string()
    }

    fn is_valid(&self) -> bool {
        is_safe_evidence_path(&self.path) && is_digest(&self.digest)
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct CertificationBundle {
    pub artifact: CertificationArtifact,
    pub evidence_files: Vec<EvidenceFile>,
    pub bundle_digest: String,
    pub signature: Option<ArtifactSignature>,
}

impl CertificationBundle {
    /// Creates an unsigned evidence bundle whose manifest binds every file hash.
    ///
    /// # Errors
    ///
    /// Returns an encoding error.
    pub fn unsigned(
        artifact: CertificationArtifact,
        evidence_files: Vec<EvidenceFile>,
    ) -> Result<Self, ConformanceError> {
        artifact.verify_identity()?;
        validate_evidence_files(&evidence_files)?;
        let bundle_digest = bundle_digest(&artifact, &evidence_files)?;
        Ok(Self {
            artifact,
            evidence_files,
            bundle_digest,
            signature: None,
        })
    }

    /// Verifies the immutable artifact, evidence manifest, and optional signature.
    ///
    /// # Errors
    ///
    /// Returns tamper, identity, or signature failures.
    pub fn verify(&self, verifier: Option<&dyn SignatureVerifier>) -> Result<(), ConformanceError> {
        self.artifact.verify_identity()?;
        validate_evidence_files(&self.evidence_files)?;
        if !is_digest(&self.bundle_digest)
            || self.bundle_digest != bundle_digest(&self.artifact, &self.evidence_files)?
        {
            return Err(ConformanceError::BundleTampered);
        }
        match (&self.signature, verifier) {
            (Some(signature), Some(verifier)) => {
                if !signature.is_valid()
                    || signature.signed_digest != self.bundle_digest
                    || !verifier.verify(signature, self.bundle_digest.as_bytes())
                {
                    return Err(ConformanceError::InvalidSignature);
                }
            }
            (Some(_), None) => return Err(ConformanceError::SignatureVerifierRequired),
            (None, _) => {}
        }
        Ok(())
    }
}

fn validate_evidence_files(evidence_files: &[EvidenceFile]) -> Result<(), ConformanceError> {
    if evidence_files.len() > MAX_EVIDENCE_FILES {
        return Err(ConformanceError::LimitExceeded("evidence files"));
    }
    let mut paths = BTreeSet::new();
    if evidence_files
        .iter()
        .any(|file| !file.is_valid() || !paths.insert(file.path.as_str()))
    {
        return Err(ConformanceError::InvalidEvidenceManifest);
    }
    Ok(())
}

fn is_safe_evidence_path(path: &str) -> bool {
    is_bounded_text(path, MAX_EVIDENCE_REFERENCE_BYTES, false)
        && !path.starts_with('/')
        && !path.contains('\\')
        && path
            .split('/')
            .all(|component| !component.is_empty() && !matches!(component, "." | ".."))
}

fn bundle_digest(
    artifact: &CertificationArtifact,
    evidence_files: &[EvidenceFile],
) -> Result<String, ConformanceError> {
    let bytes =
        postcard::to_stdvec(&(artifact, evidence_files)).map_err(|_| ConformanceError::Encoding)?;
    Ok(blake3::hash(&bytes).to_hex().to_string())
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ArtifactSignature {
    pub issuer: String,
    pub key_id: String,
    pub algorithm: String,
    pub signed_digest: String,
    pub bytes: Vec<u8>,
}

impl ArtifactSignature {
    fn is_valid(&self) -> bool {
        is_bounded_text(&self.issuer, MAX_IDENTITY_TEXT_BYTES, false)
            && is_bounded_text(&self.key_id, MAX_IDENTITY_TEXT_BYTES, false)
            && is_bounded_text(&self.algorithm, MAX_IDENTITY_TEXT_BYTES, false)
            && is_digest(&self.signed_digest)
            && !self.bytes.is_empty()
            && self.bytes.len() <= MAX_SIGNATURE_BYTES
    }
}

pub trait SignatureVerifier {
    fn verify(&self, signature: &ArtifactSignature, message: &[u8]) -> bool;
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct CatalogRecord {
    pub certification_id: String,
    pub subject: SubjectIdentity,
    pub suite_version: u32,
    pub tier: CertificationTier,
    pub trust: TrustLevel,
    pub status: CertificationStatus,
    pub status_reason: String,
    pub artifact_digest: String,
    pub sbom_digest: Option<String>,
    pub provenance_digest: Option<String>,
    pub license: String,
    pub security_contact: String,
}

impl CatalogRecord {
    /// Validates catalog identity, digest, ownership, and bounded metadata.
    ///
    /// # Errors
    ///
    /// Returns an invalid-catalog error for malformed or unbounded metadata.
    pub fn validate(&self) -> Result<(), ConformanceError> {
        validate_identity(&self.subject)?;
        if !is_digest(&self.certification_id)
            || !is_digest(&self.artifact_digest)
            || self.suite_version == 0
            || !is_bounded_text(&self.status_reason, MAX_EVIDENCE_REFERENCE_BYTES, false)
            || self
                .sbom_digest
                .as_deref()
                .is_some_and(|value| !is_digest(value))
            || self
                .provenance_digest
                .as_deref()
                .is_some_and(|value| !is_digest(value))
            || !is_bounded_text(&self.license, MAX_IDENTITY_TEXT_BYTES, false)
            || !is_bounded_text(&self.security_contact, MAX_IDENTITY_TEXT_BYTES, false)
        {
            return Err(ConformanceError::InvalidCatalog);
        }
        Ok(())
    }

    /// Applies an allowed monotonic lifecycle transition without changing identity.
    ///
    /// # Errors
    ///
    /// Returns an invalid transition error.
    pub fn transition(
        &self,
        status: CertificationStatus,
        reason: impl Into<String>,
    ) -> Result<Self, ConformanceError> {
        self.validate()?;
        let allowed = matches!(
            (self.status, status),
            (
                CertificationStatus::Active,
                CertificationStatus::Superseded
                    | CertificationStatus::Suspended
                    | CertificationStatus::Revoked
            ) | (
                CertificationStatus::Suspended,
                CertificationStatus::Active | CertificationStatus::Revoked
            )
        );
        let reason = reason.into();
        if !allowed || !is_bounded_text(&reason, MAX_EVIDENCE_REFERENCE_BYTES, false) {
            return Err(ConformanceError::InvalidLifecycleTransition);
        }
        let mut next = self.clone();
        next.status = status;
        next.status_reason = reason;
        Ok(next)
    }
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct CertificationCatalog {
    pub records: Vec<CatalogRecord>,
}

impl CertificationCatalog {
    /// Inserts a new immutable certification identity.
    ///
    /// # Errors
    ///
    /// Returns an error if the identity was already used.
    pub fn insert(&mut self, record: CatalogRecord) -> Result<(), ConformanceError> {
        record.validate()?;
        if self.records.len() >= MAX_CATALOG_RECORDS {
            return Err(ConformanceError::LimitExceeded("catalog records"));
        }
        if self
            .records
            .iter()
            .any(|existing| existing.certification_id == record.certification_id)
        {
            return Err(ConformanceError::CertificationIdentityReused);
        }
        self.records.push(record);
        self.records
            .sort_by(|left, right| left.certification_id.cmp(&right.certification_id));
        Ok(())
    }

    #[must_use]
    pub fn get(&self, id: &str) -> Option<&CatalogRecord> {
        self.records
            .iter()
            .find(|record| record.certification_id == id)
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum EnforcementMode {
    DevelopmentWarn,
    ProductionReject,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct DeploymentCertificationPolicy {
    pub mode: EnforcementMode,
    pub minimum_suite_version: u32,
    pub minimum_tier: CertificationTier,
    pub minimum_trust: TrustLevel,
    pub allowed_subjects: BTreeSet<String>,
    pub required_features: BTreeSet<String>,
    pub exact_build_configuration: BTreeMap<String, String>,
    pub required_database_engine: Option<String>,
}

impl Default for DeploymentCertificationPolicy {
    fn default() -> Self {
        Self {
            mode: EnforcementMode::DevelopmentWarn,
            minimum_suite_version: CURRENT_SUITE_VERSION,
            minimum_tier: CertificationTier::Experimental,
            minimum_trust: TrustLevel::Experimental,
            allowed_subjects: BTreeSet::new(),
            required_features: BTreeSet::new(),
            exact_build_configuration: BTreeMap::new(),
            required_database_engine: None,
        }
    }
}

impl DeploymentCertificationPolicy {
    #[must_use]
    pub fn is_valid(&self) -> bool {
        self.minimum_suite_version > 0
            && valid_set(&self.allowed_subjects)
            && valid_set(&self.required_features)
            && valid_map(&self.exact_build_configuration)
            && self
                .required_database_engine
                .as_deref()
                .is_none_or(|value| is_bounded_text(value, MAX_IDENTITY_TEXT_BYTES, false))
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DeploymentDecision {
    Ready,
    Warning(Vec<String>),
}

/// Evaluates exact subject, build, environment, suite, trust, and lifecycle binding.
///
/// # Errors
///
/// Production policy returns a startup-blocking error for every mismatch or inactive record.
pub fn evaluate_deployment(
    artifact: &CertificationArtifact,
    record: &CatalogRecord,
    policy: &DeploymentCertificationPolicy,
) -> Result<DeploymentDecision, ConformanceError> {
    artifact.verify_identity()?;
    record.validate()?;
    if !policy.is_valid() {
        return Err(ConformanceError::InvalidPolicy);
    }
    let mut reasons = Vec::new();
    if artifact.certification_id != record.certification_id
        || artifact.subject != record.subject
        || artifact.suite_version != record.suite_version
        || artifact.tier != record.tier
        || artifact.canonical_digest()? != record.artifact_digest
    {
        reasons.push("artifact and catalog identity differ".to_owned());
    }
    if record.status != CertificationStatus::Active {
        reasons.push(format!("certification status is {:?}", record.status));
    }
    if artifact.result == CertificationResult::Failed
        || artifact.result == CertificationResult::Unsupported
        || artifact.suite_version < policy.minimum_suite_version
        || artifact.tier < policy.minimum_tier
        || record.trust < policy.minimum_trust
    {
        reasons.push("suite, tier, trust, or result is below policy".to_owned());
    }
    if !policy.allowed_subjects.is_empty()
        && !policy.allowed_subjects.contains(&artifact.subject.name)
    {
        reasons.push("subject is not allow-listed".to_owned());
    }
    if !policy
        .required_features
        .is_subset(&artifact.subject.enabled_features)
        || artifact.subject.build_configuration != policy.exact_build_configuration
    {
        reasons.push("feature or build configuration mismatch".to_owned());
    }
    if let Some(required) = policy.required_database_engine.as_deref() {
        if artifact.environment.database_engine.as_deref() != Some(required) {
            reasons.push("database engine mismatch".to_owned());
        }
    }
    if reasons.is_empty() {
        Ok(DeploymentDecision::Ready)
    } else if policy.mode == EnforcementMode::DevelopmentWarn {
        Ok(DeploymentDecision::Warning(reasons))
    } else {
        Err(ConformanceError::DeploymentRejected(reasons))
    }
}

/// Compares canonical differential outputs from two implementations.
///
/// # Errors
///
/// Returns a mismatch without disclosing payload contents.
pub fn verify_differential(
    reference_digest: &str,
    subject_digest: &str,
) -> Result<(), ConformanceError> {
    if !is_digest(reference_digest)
        || !is_digest(subject_digest)
        || reference_digest != subject_digest
    {
        return Err(ConformanceError::DifferentialMismatch);
    }
    Ok(())
}

/// Produces a deterministic, human-readable report with no secret payloads.
#[must_use]
pub fn markdown_report(artifact: &CertificationArtifact) -> String {
    let mut output = format!(
        "# Aequora certification report\n\n- Certification: `{}`\n- Subject: `{}` `{}`\n- Suite: `{}`\n- Profile: `{:?}`\n- Tier: `{:?}`\n- Result: `{:?}`\n- Evidence digest: `{}`\n\n## Tests\n\n",
        markdown_inline(&artifact.certification_id),
        markdown_inline(&artifact.subject.name),
        markdown_inline(&artifact.subject.version),
        artifact.suite_version,
        artifact.profile,
        artifact.tier,
        artifact.result,
        markdown_inline(&artifact.evidence_digest)
    );
    for observation in &artifact.observations {
        let name = REFERENCE_TESTS
            .iter()
            .find(|definition| definition.id == observation.test_id)
            .map_or("unknown-test", |definition| definition.name);
        let _ = writeln!(
            output,
            "- `{}` `{}`: `{:?}`",
            observation.test_id.0, name, observation.status
        );
    }
    output
}

fn markdown_inline(value: &str) -> String {
    let mut output = String::with_capacity(value.len());
    for character in value.chars() {
        match character {
            '&' => output.push_str("&amp;"),
            '<' => output.push_str("&lt;"),
            '>' => output.push_str("&gt;"),
            '`' => output.push_str("&#96;"),
            '\n' | '\r' | '\t' => output.push(' '),
            value if value.is_control() => output.push('\u{fffd}'),
            value => output.push(value),
        }
    }
    output
}

#[derive(Debug, Error, Eq, PartialEq)]
pub enum ConformanceError {
    #[error("unsupported certification artifact or suite format")]
    UnsupportedFormat,
    #[error("subject identity is incomplete or has an invalid binary digest")]
    InvalidSubject,
    #[error("profile, tier, environment, or evidence binding is invalid")]
    InvalidBinding,
    #[error("required conformance test {0:?} is missing")]
    MissingRequiredTest(ConformanceTestId),
    #[error("duplicate conformance test {0:?}")]
    DuplicateTest(ConformanceTestId),
    #[error("unknown conformance test {0:?}")]
    UnknownTest(ConformanceTestId),
    #[error("claimed capability {0:?} has not passed all required tests")]
    UnverifiedCapability(String),
    #[error("declared result does not match required test outcomes")]
    ResultMismatch,
    #[error("certification encoding failed")]
    Encoding,
    #[error("certification artifact identity does not match its contents")]
    ArtifactTampered,
    #[error("evidence bundle digest does not match its manifest")]
    BundleTampered,
    #[error("evidence manifest contains an invalid, duplicate, or unsafe path")]
    InvalidEvidenceManifest,
    #[error("a signature exists but no verifier was provided")]
    SignatureVerifierRequired,
    #[error("certification signature is invalid")]
    InvalidSignature,
    #[error("certification lifecycle transition is invalid")]
    InvalidLifecycleTransition,
    #[error("certification identity was already used")]
    CertificationIdentityReused,
    #[error("certification catalog metadata is invalid")]
    InvalidCatalog,
    #[error("deployment certification policy is invalid")]
    InvalidPolicy,
    #[error("certification input exceeds the bounded {0} limit")]
    LimitExceeded(&'static str),
    #[error("deployment rejected certification: {0:?}")]
    DeploymentRejected(Vec<String>),
    #[error("differential conformance output mismatch")]
    DifferentialMismatch,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn subject() -> SubjectIdentity {
        SubjectIdentity {
            name: "reference-store".into(),
            version: "1.0.0".into(),
            source_commit: "0123456789abcdef".into(),
            binary_digest: "a".repeat(64),
            enabled_features: BTreeSet::new(),
            build_configuration: BTreeMap::new(),
        }
    }

    fn environment() -> ExecutionEnvironment {
        ExecutionEnvironment {
            target: "x86_64-unknown-linux-gnu".into(),
            operating_system: "linux".into(),
            architecture: "x86_64".into(),
            rust_version: "1.87".into(),
            database_engine: None,
            database_version: None,
            provider_versions: BTreeMap::new(),
        }
    }

    fn observations(profile: ConformanceProfile, tier: CertificationTier) -> Vec<TestObservation> {
        definitions_for(profile, tier)
            .into_iter()
            .map(|definition| TestObservation {
                test_id: definition.id,
                status: TestStatus::Passed,
                seed: Some(7),
                evidence: vec!["digest-only".into()],
                message: definition.name.into(),
            })
            .collect()
    }

    fn catalog_record(
        artifact: &CertificationArtifact,
        status: CertificationStatus,
    ) -> Result<CatalogRecord, ConformanceError> {
        Ok(CatalogRecord {
            certification_id: artifact.certification_id.clone(),
            subject: artifact.subject.clone(),
            suite_version: artifact.suite_version,
            tier: artifact.tier,
            trust: TrustLevel::Official,
            status,
            status_reason: "fixture".into(),
            artifact_digest: artifact.canonical_digest()?,
            sbom_digest: None,
            provenance_digest: None,
            license: "MIT".into(),
            security_contact: "security@example.invalid".into(),
        })
    }

    fn production_policy(minimum_suite_version: u32) -> DeploymentCertificationPolicy {
        DeploymentCertificationPolicy {
            mode: EnforcementMode::ProductionReject,
            minimum_suite_version,
            minimum_tier: CertificationTier::CoreTransactional,
            minimum_trust: TrustLevel::MaintainerVerified,
            allowed_subjects: BTreeSet::new(),
            required_features: BTreeSet::new(),
            exact_build_configuration: BTreeMap::new(),
            required_database_engine: None,
        }
    }

    #[test]
    fn required_skip_cannot_claim_a_tier() -> Result<(), ConformanceError> {
        let mut values = observations(
            ConformanceProfile::StorageCore,
            CertificationTier::CoreTransactional,
        );
        values[0].status = TestStatus::Skipped;
        let artifact = certify(
            subject(),
            environment(),
            ConformanceProfile::StorageCore,
            CertificationTier::CoreTransactional,
            BTreeSet::new(),
            values,
            Vec::new(),
            1,
        )?;
        assert_eq!(artifact.result, CertificationResult::Failed);
        Ok(())
    }

    #[test]
    fn false_capability_claim_produces_failed_artifact() -> Result<(), ConformanceError> {
        let mut claimed = BTreeSet::new();
        claimed.insert("authority-fencing".to_owned());
        let artifact = certify(
            subject(),
            environment(),
            ConformanceProfile::StorageCore,
            CertificationTier::CoreTransactional,
            claimed,
            observations(
                ConformanceProfile::StorageCore,
                CertificationTier::CoreTransactional,
            ),
            Vec::new(),
            1,
        )?;
        assert_eq!(artifact.result, CertificationResult::Failed);
        assert!(artifact.capability_manifest.verified.is_empty());
        Ok(())
    }

    #[test]
    fn artifact_tampering_is_detected() -> Result<(), ConformanceError> {
        let mut artifact = certify(
            subject(),
            environment(),
            ConformanceProfile::StorageCore,
            CertificationTier::CoreTransactional,
            BTreeSet::new(),
            observations(
                ConformanceProfile::StorageCore,
                CertificationTier::CoreTransactional,
            ),
            Vec::new(),
            1,
        )?;
        artifact.subject.version = "2.0.0".into();
        assert_eq!(
            artifact.verify_identity(),
            Err(ConformanceError::ArtifactTampered)
        );
        Ok(())
    }

    #[test]
    fn evidence_file_tampering_is_detected() {
        let evidence = EvidenceFile::from_bytes("results.ron", b"passed");
        assert!(evidence.verifies(b"passed"));
        assert!(!evidence.verifies(b"failed"));
    }

    #[test]
    fn optional_snapshot_skip_does_not_fail_core_tier() -> Result<(), ConformanceError> {
        let mut values = observations(
            ConformanceProfile::StorageCore,
            CertificationTier::CoreTransactional,
        );
        values.push(TestObservation {
            test_id: ConformanceTestId(6),
            status: TestStatus::Skipped,
            seed: Some(99),
            evidence: Vec::new(),
            message: "snapshot capability not claimed".into(),
        });
        let artifact = certify(
            subject(),
            environment(),
            ConformanceProfile::StorageCore,
            CertificationTier::CoreTransactional,
            BTreeSet::new(),
            values,
            Vec::new(),
            1,
        )?;
        assert_eq!(artifact.result, CertificationResult::Passed);
        Ok(())
    }

    #[test]
    fn correctness_failure_cannot_be_hidden_by_performance() -> Result<(), ConformanceError> {
        let mut values = observations(
            ConformanceProfile::StorageCore,
            CertificationTier::CoreTransactional,
        );
        values[0].status = TestStatus::Failed;
        let mut artifact = certify(
            subject(),
            environment(),
            ConformanceProfile::StorageCore,
            CertificationTier::CoreTransactional,
            BTreeSet::new(),
            values,
            Vec::new(),
            1,
        )?;
        artifact.performance.push(PerformanceCharacterization {
            workload: "fast".into(),
            measurement: "1ns".into(),
            environment_note: "fixture".into(),
        });
        let artifact = artifact.seal()?;
        assert_eq!(artifact.result, CertificationResult::Failed);
        artifact.verify_identity()?;
        Ok(())
    }

    #[test]
    fn early_cursor_advance_failure_blocks_full_sync_tier() -> Result<(), ConformanceError> {
        let mut values = observations(
            ConformanceProfile::StorageFullSync,
            CertificationTier::FullSync,
        );
        let cursor = values
            .iter_mut()
            .find(|observation| observation.test_id == ConformanceTestId(4))
            .ok_or(ConformanceError::MissingRequiredTest(ConformanceTestId(4)))?;
        cursor.status = TestStatus::Failed;
        let artifact = certify(
            subject(),
            environment(),
            ConformanceProfile::StorageFullSync,
            CertificationTier::FullSync,
            BTreeSet::new(),
            values,
            Vec::new(),
            1,
        )?;
        assert_eq!(artifact.result, CertificationResult::Failed);
        Ok(())
    }

    #[test]
    fn bundle_manifest_tamper_is_detected() -> Result<(), ConformanceError> {
        let artifact = certify(
            subject(),
            environment(),
            ConformanceProfile::StorageCore,
            CertificationTier::CoreTransactional,
            BTreeSet::new(),
            observations(
                ConformanceProfile::StorageCore,
                CertificationTier::CoreTransactional,
            ),
            Vec::new(),
            1,
        )?;
        let mut bundle = CertificationBundle::unsigned(
            artifact,
            vec![EvidenceFile::from_bytes("report.md", b"passed")],
        )?;
        bundle.evidence_files[0].digest = "0".repeat(64);
        assert_eq!(bundle.verify(None), Err(ConformanceError::BundleTampered));
        Ok(())
    }

    #[test]
    fn wrong_binary_cannot_reuse_artifact_identity() -> Result<(), ConformanceError> {
        let mut artifact = certify(
            subject(),
            environment(),
            ConformanceProfile::StorageCore,
            CertificationTier::CoreTransactional,
            BTreeSet::new(),
            observations(
                ConformanceProfile::StorageCore,
                CertificationTier::CoreTransactional,
            ),
            Vec::new(),
            1,
        )?;
        artifact.subject.binary_digest = "b".repeat(64);
        assert_eq!(
            artifact.verify_identity(),
            Err(ConformanceError::ArtifactTampered)
        );
        Ok(())
    }

    #[test]
    fn revoked_certification_blocks_production() -> Result<(), ConformanceError> {
        let artifact = certify(
            subject(),
            environment(),
            ConformanceProfile::StorageCore,
            CertificationTier::CoreTransactional,
            BTreeSet::new(),
            observations(
                ConformanceProfile::StorageCore,
                CertificationTier::CoreTransactional,
            ),
            Vec::new(),
            1,
        )?;
        let record = catalog_record(&artifact, CertificationStatus::Revoked)?;
        let policy = production_policy(1);
        assert!(matches!(
            evaluate_deployment(&artifact, &record, &policy),
            Err(ConformanceError::DeploymentRejected(_))
        ));
        Ok(())
    }

    #[test]
    fn superseded_suite_version_blocks_production() -> Result<(), ConformanceError> {
        let artifact = certify(
            subject(),
            environment(),
            ConformanceProfile::StorageCore,
            CertificationTier::CoreTransactional,
            BTreeSet::new(),
            observations(
                ConformanceProfile::StorageCore,
                CertificationTier::CoreTransactional,
            ),
            Vec::new(),
            1,
        )?;
        let record = catalog_record(&artifact, CertificationStatus::Active)?;
        assert!(matches!(
            evaluate_deployment(&artifact, &record, &production_policy(2)),
            Err(ConformanceError::DeploymentRejected(_))
        ));
        Ok(())
    }

    #[test]
    fn differential_mismatch_fails() {
        assert_eq!(
            verify_differential(&"a".repeat(64), &"b".repeat(64)),
            Err(ConformanceError::DifferentialMismatch)
        );
        assert_eq!(
            verify_differential(&"z".repeat(64), &"z".repeat(64)),
            Err(ConformanceError::DifferentialMismatch)
        );
    }

    #[test]
    fn digest_shapes_and_unknown_tests_fail_closed() {
        let mut invalid_subject = subject();
        invalid_subject.binary_digest = "z".repeat(64);
        assert_eq!(
            certify(
                invalid_subject,
                environment(),
                ConformanceProfile::StorageCore,
                CertificationTier::CoreTransactional,
                BTreeSet::new(),
                observations(
                    ConformanceProfile::StorageCore,
                    CertificationTier::CoreTransactional,
                ),
                Vec::new(),
                1,
            ),
            Err(ConformanceError::InvalidSubject)
        );

        let mut values = observations(
            ConformanceProfile::StorageCore,
            CertificationTier::CoreTransactional,
        );
        values.push(TestObservation {
            test_id: ConformanceTestId(9_999),
            status: TestStatus::Passed,
            seed: Some(1),
            evidence: vec!["digest-only".into()],
            message: "unknown".into(),
        });
        assert_eq!(
            certify(
                subject(),
                environment(),
                ConformanceProfile::StorageCore,
                CertificationTier::CoreTransactional,
                BTreeSet::new(),
                values,
                Vec::new(),
                1,
            ),
            Err(ConformanceError::UnknownTest(ConformanceTestId(9_999)))
        );
    }

    #[test]
    fn coverage_and_future_suite_claims_cannot_be_resealed() -> Result<(), ConformanceError> {
        let artifact = certify(
            subject(),
            environment(),
            ConformanceProfile::StorageCore,
            CertificationTier::CoreTransactional,
            BTreeSet::new(),
            observations(
                ConformanceProfile::StorageCore,
                CertificationTier::CoreTransactional,
            ),
            Vec::new(),
            1,
        )?;
        let mut forged_coverage = artifact.clone();
        forged_coverage.coverage[0]
            .methods
            .insert(VerificationMethod::ManualReview);
        assert_eq!(
            forged_coverage.seal(),
            Err(ConformanceError::InvalidBinding)
        );

        let mut future_suite = artifact;
        future_suite.suite_version = CURRENT_SUITE_VERSION + 1;
        assert_eq!(
            future_suite.seal(),
            Err(ConformanceError::UnsupportedFormat)
        );
        Ok(())
    }

    #[test]
    fn report_uses_stable_names_and_omits_untrusted_messages() -> Result<(), ConformanceError> {
        let mut value = subject();
        value.name = "<script>alert(1)</script>".into();
        let mut values = observations(
            ConformanceProfile::StorageCore,
            CertificationTier::CoreTransactional,
        );
        values[0].message = "token=do-not-render".into();
        let artifact = certify(
            value,
            environment(),
            ConformanceProfile::StorageCore,
            CertificationTier::CoreTransactional,
            BTreeSet::new(),
            values,
            Vec::new(),
            1,
        )?;
        let report = markdown_report(&artifact);
        assert!(report.contains("&lt;script&gt;alert(1)&lt;/script&gt;"));
        assert!(report.contains("local_intent_atomicity"));
        assert!(!report.contains("do-not-render"));
        Ok(())
    }

    #[test]
    fn evidence_paths_and_policy_metadata_are_bounded() -> Result<(), ConformanceError> {
        let artifact = certify(
            subject(),
            environment(),
            ConformanceProfile::StorageCore,
            CertificationTier::CoreTransactional,
            BTreeSet::new(),
            observations(
                ConformanceProfile::StorageCore,
                CertificationTier::CoreTransactional,
            ),
            Vec::new(),
            1,
        )?;
        assert_eq!(
            CertificationBundle::unsigned(
                artifact.clone(),
                vec![EvidenceFile::from_bytes("../secret", b"payload")],
            ),
            Err(ConformanceError::InvalidEvidenceManifest)
        );
        assert_eq!(
            CertificationBundle::unsigned(
                artifact.clone(),
                vec![
                    EvidenceFile::from_bytes("report.ron", b"one"),
                    EvidenceFile::from_bytes("report.ron", b"two"),
                ],
            ),
            Err(ConformanceError::InvalidEvidenceManifest)
        );

        let record = catalog_record(&artifact, CertificationStatus::Active)?;
        let mut policy = production_policy(1);
        policy.required_features.insert(String::new());
        assert_eq!(
            evaluate_deployment(&artifact, &record, &policy),
            Err(ConformanceError::InvalidPolicy)
        );
        Ok(())
    }

    #[test]
    fn adapter_profiles_select_role_specific_part_36_tests() {
        let local = definitions_for(
            ConformanceProfile::LocalAdapter,
            CertificationTier::FullSync,
        )
        .into_iter()
        .map(|definition| definition.id)
        .collect::<BTreeSet<_>>();
        let authority = definitions_for(
            ConformanceProfile::AuthoritativeAdapter,
            CertificationTier::FullSync,
        )
        .into_iter()
        .map(|definition| definition.id)
        .collect::<BTreeSet<_>>();

        assert!(local.contains(&ConformanceTestId(50)));
        assert!(!local.contains(&ConformanceTestId(51)));
        assert!(authority.contains(&ConformanceTestId(51)));
        assert!(!authority.contains(&ConformanceTestId(50)));
        assert!(local.contains(&ConformanceTestId(56)));
        assert!(authority.contains(&ConformanceTestId(56)));
    }

    #[test]
    fn stoolap_profiles_separate_core_from_target_bound_full_evidence() {
        let core = definitions_for(
            ConformanceProfile::StoolapLocalCore,
            CertificationTier::CoreTransactional,
        )
        .into_iter()
        .map(|definition| definition.id)
        .collect::<BTreeSet<_>>();
        let desktop = definitions_for(
            ConformanceProfile::StoolapDesktopLocalFull,
            CertificationTier::FullSync,
        )
        .into_iter()
        .map(|definition| definition.id)
        .collect::<BTreeSet<_>>();
        let mobile = definitions_for(
            ConformanceProfile::StoolapMobileLocalFull,
            CertificationTier::FullSync,
        )
        .into_iter()
        .map(|definition| definition.id)
        .collect::<BTreeSet<_>>();

        for id in [69, 70, 71, 72, 73, 77] {
            assert!(core.contains(&ConformanceTestId(id)));
        }
        for id in [74, 75, 76, 78] {
            assert!(!core.contains(&ConformanceTestId(id)));
            assert!(desktop.contains(&ConformanceTestId(id)));
            assert!(mobile.contains(&ConformanceTestId(id)));
        }
    }

    #[test]
    fn sqlite_profiles_separate_portable_core_from_target_bound_full_evidence() {
        let core = definitions_for(
            ConformanceProfile::SQLiteLocalCore,
            CertificationTier::CoreTransactional,
        )
        .into_iter()
        .map(|definition| definition.id)
        .collect::<BTreeSet<_>>();
        let desktop = definitions_for(
            ConformanceProfile::SQLiteDesktopLocalFull,
            CertificationTier::FullSync,
        )
        .into_iter()
        .map(|definition| definition.id)
        .collect::<BTreeSet<_>>();
        let mobile = definitions_for(
            ConformanceProfile::SQLiteMobileLocalFull,
            CertificationTier::FullSync,
        )
        .into_iter()
        .map(|definition| definition.id)
        .collect::<BTreeSet<_>>();

        for id in 109..=112 {
            assert!(core.contains(&ConformanceTestId(id)));
            assert!(desktop.contains(&ConformanceTestId(id)));
            assert!(mobile.contains(&ConformanceTestId(id)));
        }
        assert!(!core.contains(&ConformanceTestId(113)));
        assert!(desktop.contains(&ConformanceTestId(113)));
        assert!(mobile.contains(&ConformanceTestId(113)));
    }

    #[test]
    fn axum_profile_selects_only_part_39_transport_contracts() {
        let ids = definitions_for(
            ConformanceProfile::AxumServerFull,
            CertificationTier::FullSync,
        )
        .into_iter()
        .map(|definition| definition.id.0)
        .collect::<BTreeSet<_>>();
        assert_eq!(ids, (79..=88).collect());
    }

    #[test]
    fn dioxus_profile_selects_only_part_40_ui_contracts() {
        let ids = definitions_for(
            ConformanceProfile::DioxusClientFull,
            CertificationTier::FullSync,
        )
        .into_iter()
        .map(|definition| definition.id.0)
        .collect::<BTreeSet<_>>();
        assert_eq!(ids, (89..=98).collect());
    }

    #[test]
    fn cli_profile_selects_only_part_41_toolchain_contracts() {
        let ids = definitions_for(
            ConformanceProfile::CliToolchainFull,
            CertificationTier::FullSync,
        )
        .into_iter()
        .map(|definition| definition.id.0)
        .collect::<BTreeSet<_>>();
        assert_eq!(ids, (99..=108).collect());
    }
}
