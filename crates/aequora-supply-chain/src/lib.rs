//! Database-, runtime-, transport-, and vendor-neutral supply-chain governance contracts.
//!
//! Metadata scanners, advisory databases, SBOM encoders, signing services, CI runners, and
//! providers remain replaceable adapters around these fail-closed data models.

use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use thiserror::Error;

pub const POLICY_SCHEMA_VERSION: u16 = 1;
pub const MAX_RECORDS: usize = 16_384;
const MAX_TEXT_BYTES: usize = 2_048;

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
pub enum DependencyClass {
    CoreSemantic,
    SecurityCritical,
    Storage,
    Networking,
    Serialization,
    Platform,
    UiIntegration,
    BuildTime,
    DeveloperTooling,
    Testing,
    OptionalIntegration,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
pub enum RiskTier {
    Tier0,
    Tier1,
    Tier2,
    Tier3,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum UnsafeUse {
    None,
    Contained,
    Significant,
    Unknown,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum SourceKind {
    Workspace,
    ApprovedRegistry,
    ImmutableGitCommit,
    ReviewedFork,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct DependencyRecord {
    pub package: String,
    pub class: DependencyClass,
    pub risk: RiskTier,
    pub license_expression: String,
    pub source: String,
    pub source_kind: SourceKind,
    pub native_code: bool,
    pub build_script: bool,
    pub proc_macro: bool,
    pub unsafe_use: UnsafeUse,
    pub owner: String,
    pub capability: String,
    pub replacement: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PolicyException {
    pub package: String,
    pub policy: String,
    pub reason: String,
    pub owner: String,
    pub expires_at_unix_seconds: u64,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RiskAcceptance {
    pub identity: String,
    pub benefit: String,
    pub risk: String,
    pub mitigation: String,
    pub replacement: String,
    pub owner: String,
    pub review_at_unix_seconds: u64,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct DependencyPolicy {
    pub schema_version: u16,
    pub allowed_licenses: BTreeSet<String>,
    pub approved_sources: BTreeSet<String>,
    pub dependencies: Vec<DependencyRecord>,
    pub exceptions: Vec<PolicyException>,
    pub risk_acceptances: Vec<RiskAcceptance>,
}

impl DependencyPolicy {
    /// Parses and validates a checked-in policy at the supplied review time.
    ///
    /// # Errors
    ///
    /// Returns an error for malformed RON, invalid records, or expired exceptions and reviews.
    pub fn from_ron(input: &str, now_unix_seconds: u64) -> Result<Self, SupplyChainError> {
        let policy: Self = ron::from_str(input).map_err(|_| SupplyChainError::MalformedPolicy)?;
        policy.validate(now_unix_seconds)?;
        Ok(policy)
    }

    /// Validates license, source, ownership, replacement, and exception policy.
    ///
    /// # Errors
    ///
    /// Fails closed for unknown licenses or sources, duplicates, incomplete critical reviews,
    /// expired exceptions, and expired risk acceptances.
    pub fn validate(&self, now_unix_seconds: u64) -> Result<(), SupplyChainError> {
        if self.schema_version != POLICY_SCHEMA_VERSION {
            return Err(SupplyChainError::UnsupportedSchema(self.schema_version));
        }
        if self.allowed_licenses.is_empty()
            || self.approved_sources.is_empty()
            || self.dependencies.is_empty()
            || self.dependencies.len() > MAX_RECORDS
            || self.exceptions.len() > MAX_RECORDS
            || self.risk_acceptances.len() > MAX_RECORDS
        {
            return Err(SupplyChainError::InvalidPolicy(
                "missing or unbounded policy data",
            ));
        }
        validate_set(&self.allowed_licenses)?;
        validate_set(&self.approved_sources)?;
        let mut packages = BTreeSet::new();
        for dependency in &self.dependencies {
            dependency.validate(self)?;
            if !packages.insert(dependency.package.as_str()) {
                return Err(SupplyChainError::DuplicateDependency(
                    dependency.package.clone(),
                ));
            }
        }
        let mut exception_keys = BTreeSet::new();
        for exception in &self.exceptions {
            for value in [
                &exception.package,
                &exception.policy,
                &exception.reason,
                &exception.owner,
            ] {
                validate_text(value)?;
            }
            if exception.expires_at_unix_seconds <= now_unix_seconds {
                return Err(SupplyChainError::ExpiredException(
                    exception.package.clone(),
                ));
            }
            if !exception_keys.insert((&exception.package, &exception.policy)) {
                return Err(SupplyChainError::DuplicateException(
                    exception.package.clone(),
                ));
            }
        }
        let mut risk_ids = BTreeSet::new();
        for acceptance in &self.risk_acceptances {
            for value in [
                &acceptance.identity,
                &acceptance.benefit,
                &acceptance.risk,
                &acceptance.mitigation,
                &acceptance.replacement,
                &acceptance.owner,
            ] {
                validate_text(value)?;
            }
            if acceptance.review_at_unix_seconds <= now_unix_seconds {
                return Err(SupplyChainError::ExpiredRiskReview(
                    acceptance.identity.clone(),
                ));
            }
            if !risk_ids.insert(acceptance.identity.as_str()) {
                return Err(SupplyChainError::DuplicateRisk(acceptance.identity.clone()));
            }
        }
        Ok(())
    }

    /// Verifies that every externally resolved package was explicitly reviewed.
    ///
    /// # Errors
    ///
    /// Returns the first dependency absent from the reviewed policy.
    pub fn verify_inventory(&self, packages: &BTreeSet<String>) -> Result<(), SupplyChainError> {
        let reviewed = self
            .dependencies
            .iter()
            .map(|dependency| dependency.package.as_str())
            .collect::<BTreeSet<_>>();
        if let Some(package) = packages
            .iter()
            .find(|package| !reviewed.contains(package.as_str()))
        {
            return Err(SupplyChainError::UnreviewedDependency(package.clone()));
        }
        Ok(())
    }

    #[must_use]
    pub fn dependency(&self, package: &str) -> Option<&DependencyRecord> {
        self.dependencies
            .iter()
            .find(|dependency| dependency.package == package)
    }
}

impl DependencyRecord {
    fn validate(&self, policy: &DependencyPolicy) -> Result<(), SupplyChainError> {
        for value in [
            &self.package,
            &self.license_expression,
            &self.source,
            &self.owner,
            &self.capability,
            &self.replacement,
        ] {
            validate_text(value)?;
        }
        if self.unsafe_use == UnsafeUse::Unknown && self.risk >= RiskTier::Tier2 {
            return Err(SupplyChainError::UnknownCriticalUnsafe(
                self.package.clone(),
            ));
        }
        if self.risk >= RiskTier::Tier2 && (self.owner == "unowned" || self.replacement == "none") {
            return Err(SupplyChainError::IncompleteCriticalReview(
                self.package.clone(),
            ));
        }
        if self.source_kind != SourceKind::Workspace
            && !policy.approved_sources.contains(&self.source)
        {
            return Err(SupplyChainError::UnapprovedSource(self.package.clone()));
        }
        let licenses = license_ids(&self.license_expression).collect::<Vec<_>>();
        if licenses.is_empty() {
            return Err(SupplyChainError::ProhibitedLicense {
                package: self.package.clone(),
                license: "unknown".to_owned(),
            });
        }
        for license in licenses {
            if !policy.allowed_licenses.contains(license) {
                return Err(SupplyChainError::ProhibitedLicense {
                    package: self.package.clone(),
                    license: license.to_owned(),
                });
            }
        }
        Ok(())
    }
}

fn license_ids(expression: &str) -> impl Iterator<Item = &str> {
    expression
        .split(|character: char| character.is_whitespace() || matches!(character, '(' | ')'))
        .filter(|token| !token.is_empty() && *token != "AND" && *token != "OR" && *token != "WITH")
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
pub enum ArtifactProfile {
    CoreLibrary,
    Cli,
    Server,
    Desktop,
    Android,
    Ios,
    Agent,
    AirGapped,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SbomComponent {
    pub package: String,
    pub version: String,
    pub source: String,
    pub checksum: String,
    pub license_expression: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SbomDocument {
    pub schema_version: u16,
    pub format: String,
    pub profile: ArtifactProfile,
    pub artifact: String,
    pub artifact_digest: String,
    pub build_id: String,
    pub source_commit: String,
    pub lockfile_digest: String,
    pub components: Vec<SbomComponent>,
    pub relationships: BTreeMap<String, BTreeSet<String>>,
}

impl SbomDocument {
    /// Validates the SBOM and binds every external component to reviewed policy.
    ///
    /// # Errors
    ///
    /// Returns an error for malformed identity, duplicate or unreviewed components, invalid
    /// relationships, or license and source drift.
    pub fn validate(&self, policy: &DependencyPolicy) -> Result<(), SupplyChainError> {
        if self.schema_version != POLICY_SCHEMA_VERSION
            || !matches!(self.format.as_str(), "CycloneDX" | "SPDX")
            || self.components.is_empty()
            || self.components.len() > MAX_RECORDS
        {
            return Err(SupplyChainError::InvalidSbom);
        }
        for value in [
            &self.artifact,
            &self.build_id,
            &self.source_commit,
            &self.lockfile_digest,
        ] {
            validate_text(value)?;
        }
        validate_digest(&self.artifact_digest)?;
        validate_digest(&self.lockfile_digest)?;
        let mut packages = BTreeSet::new();
        for component in &self.components {
            for value in [
                &component.package,
                &component.version,
                &component.source,
                &component.checksum,
                &component.license_expression,
            ] {
                validate_text(value)?;
            }
            validate_digest(&component.checksum)?;
            let reviewed = policy
                .dependency(&component.package)
                .ok_or_else(|| SupplyChainError::UnreviewedDependency(component.package.clone()))?;
            if reviewed.source != component.source
                || reviewed.license_expression != component.license_expression
            {
                return Err(SupplyChainError::SbomPolicyDrift(component.package.clone()));
            }
            if !packages.insert(component.package.as_str()) {
                return Err(SupplyChainError::DuplicateDependency(
                    component.package.clone(),
                ));
            }
        }
        if self
            .relationships
            .keys()
            .any(|package| !packages.contains(package.as_str()))
            || self
                .relationships
                .values()
                .flatten()
                .any(|package| !packages.contains(package.as_str()))
        {
            return Err(SupplyChainError::InvalidSbomRelationship);
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
pub enum VerificationArea {
    Unit,
    Property,
    Model,
    AdapterConformance,
    Integration,
    Migration,
    Compatibility,
    Security,
    Performance,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum SemanticImpact {
    None,
    Storage,
    Protocol,
    Cryptography,
    Authority,
    Security,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct DependencyChangeEvidence {
    pub package: String,
    pub impact: SemanticImpact,
    pub passed: BTreeSet<VerificationArea>,
}

impl DependencyChangeEvidence {
    /// Rejects correctness and security sensitive updates without required verification.
    ///
    /// # Errors
    ///
    /// Returns an error when the declared semantic impact lacks required evidence.
    pub fn validate(&self) -> Result<(), SupplyChainError> {
        validate_text(&self.package)?;
        let required = match self.impact {
            SemanticImpact::None => BTreeSet::from([VerificationArea::Unit]),
            SemanticImpact::Storage => BTreeSet::from([
                VerificationArea::Unit,
                VerificationArea::Property,
                VerificationArea::AdapterConformance,
                VerificationArea::Migration,
            ]),
            SemanticImpact::Protocol => BTreeSet::from([
                VerificationArea::Unit,
                VerificationArea::Property,
                VerificationArea::Compatibility,
                VerificationArea::Security,
            ]),
            SemanticImpact::Cryptography | SemanticImpact::Security => BTreeSet::from([
                VerificationArea::Unit,
                VerificationArea::Compatibility,
                VerificationArea::Security,
            ]),
            SemanticImpact::Authority => BTreeSet::from([
                VerificationArea::Unit,
                VerificationArea::Property,
                VerificationArea::Model,
                VerificationArea::Integration,
                VerificationArea::Security,
            ]),
        };
        if required.is_subset(&self.passed) {
            Ok(())
        } else {
            Err(SupplyChainError::IncompleteUpdateEvidence(
                self.package.clone(),
            ))
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ReleaseEvidence {
    pub source_commit: String,
    pub lockfile_digest: String,
    pub build_configuration_digest: String,
    pub sbom_digest: String,
    pub provenance_digest: String,
    pub artifact_digest: String,
    pub builder_identity: String,
    pub rust_toolchain: String,
    pub target: String,
    pub network_access_after_staging: bool,
    pub signing_credentials_exposed_to_build: bool,
    pub dependency_changes: Vec<DependencyChangeEvidence>,
}

impl ReleaseEvidence {
    /// Validates high-assurance build binding and credential and network isolation.
    ///
    /// # Errors
    ///
    /// Rejects incomplete digests, uncontrolled network access, exposed signing credentials, or
    /// dependency changes lacking their semantic verification matrix.
    pub fn validate(&self) -> Result<(), SupplyChainError> {
        for value in [
            &self.source_commit,
            &self.builder_identity,
            &self.rust_toolchain,
            &self.target,
        ] {
            validate_text(value)?;
        }
        for digest in [
            &self.lockfile_digest,
            &self.build_configuration_digest,
            &self.sbom_digest,
            &self.provenance_digest,
            &self.artifact_digest,
        ] {
            validate_digest(digest)?;
        }
        if self.network_access_after_staging {
            return Err(SupplyChainError::BuildNetworkAccess);
        }
        if self.signing_credentials_exposed_to_build {
            return Err(SupplyChainError::SigningCredentialExposure);
        }
        for change in &self.dependency_changes {
            change.validate()?;
        }
        Ok(())
    }

    /// Verifies that release evidence and the SBOM describe the same artifact build.
    ///
    /// # Errors
    ///
    /// Returns an error when artifact, source, lockfile, or build identity differs.
    pub fn verify_sbom(&self, sbom: &SbomDocument) -> Result<(), SupplyChainError> {
        if self.artifact_digest != sbom.artifact_digest
            || self.source_commit != sbom.source_commit
            || self.lockfile_digest != sbom.lockfile_digest
            || self.builder_identity != sbom.build_id
        {
            return Err(SupplyChainError::ReleaseSbomMismatch);
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
pub enum ReproducibilityLevel {
    NoClaim,
    Functional,
    DeterministicPackageContents,
    ByteIdentical,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ReproducibilityReport {
    pub artifact: String,
    pub claimed_level: ReproducibilityLevel,
    pub builder_a_digest: String,
    pub builder_b_digest: String,
    pub package_contents_equal: bool,
    pub functional_tests_equal: bool,
    pub differences: Vec<String>,
}

impl ReproducibilityReport {
    /// Ensures the claimed reproducibility level does not exceed retained evidence.
    ///
    /// # Errors
    ///
    /// Returns an error for an unsupported claim or malformed evidence.
    pub fn validate(&self) -> Result<(), SupplyChainError> {
        validate_text(&self.artifact)?;
        validate_digest(&self.builder_a_digest)?;
        validate_digest(&self.builder_b_digest)?;
        if self.differences.len() > 1_024 {
            return Err(SupplyChainError::InvalidReproducibilityReport);
        }
        for difference in &self.differences {
            validate_text(difference)?;
        }
        let demonstrated = if self.builder_a_digest == self.builder_b_digest {
            ReproducibilityLevel::ByteIdentical
        } else if self.package_contents_equal {
            ReproducibilityLevel::DeterministicPackageContents
        } else if self.functional_tests_equal {
            ReproducibilityLevel::Functional
        } else {
            ReproducibilityLevel::NoClaim
        };
        if self.claimed_level > demonstrated {
            Err(SupplyChainError::OverstatedReproducibility)
        } else {
            Ok(())
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ThirdPartyService {
    pub provider: String,
    pub purpose: String,
    pub data_handled: BTreeSet<String>,
    pub failure_behavior: String,
    pub replacement: String,
    pub residency: String,
    pub owner: String,
    pub source_of_authority: bool,
    pub required_in_air_gapped_profile: bool,
}

/// Validates that operational dependencies are explicit, replaceable, and non-authoritative.
///
/// # Errors
///
/// Returns an error for duplicate or incomplete entries, hidden authority, or air-gap dependence.
pub fn validate_services(services: &[ThirdPartyService]) -> Result<(), SupplyChainError> {
    if services.len() > MAX_RECORDS {
        return Err(SupplyChainError::InvalidServiceInventory);
    }
    let mut providers = BTreeSet::new();
    for service in services {
        for value in [
            &service.provider,
            &service.purpose,
            &service.failure_behavior,
            &service.replacement,
            &service.residency,
            &service.owner,
        ] {
            validate_text(value)?;
        }
        validate_set(&service.data_handled)?;
        if service.source_of_authority || service.required_in_air_gapped_profile {
            return Err(SupplyChainError::InvalidServiceBoundary(
                service.provider.clone(),
            ));
        }
        if !providers.insert(service.provider.as_str()) {
            return Err(SupplyChainError::InvalidServiceInventory);
        }
    }
    Ok(())
}

fn validate_set(values: &BTreeSet<String>) -> Result<(), SupplyChainError> {
    for value in values {
        validate_text(value)?;
    }
    Ok(())
}

fn validate_text(value: &str) -> Result<(), SupplyChainError> {
    if value.is_empty() || value.len() > MAX_TEXT_BYTES || value.chars().any(char::is_control) {
        return Err(SupplyChainError::InvalidText);
    }
    Ok(())
}

fn validate_digest(value: &str) -> Result<(), SupplyChainError> {
    validate_text(value)?;
    let Some((algorithm, encoded)) = value.split_once(':') else {
        return Err(SupplyChainError::InvalidDigest);
    };
    if !matches!(algorithm, "sha256" | "blake3")
        || encoded.len() != 64
        || !encoded.bytes().all(|byte| byte.is_ascii_hexdigit())
    {
        return Err(SupplyChainError::InvalidDigest);
    }
    Ok(())
}

#[derive(Debug, Error, Eq, PartialEq)]
pub enum SupplyChainError {
    #[error("supply-chain policy RON is malformed")]
    MalformedPolicy,
    #[error("unsupported supply-chain schema version {0}")]
    UnsupportedSchema(u16),
    #[error("invalid dependency policy: {0}")]
    InvalidPolicy(&'static str),
    #[error("supply-chain text is empty, unbounded, or contains control characters")]
    InvalidText,
    #[error("dependency {0} is duplicated")]
    DuplicateDependency(String),
    #[error("policy exception for dependency {0} is duplicated")]
    DuplicateException(String),
    #[error("risk acceptance {0} is duplicated")]
    DuplicateRisk(String),
    #[error("dependency {0} has an expired policy exception")]
    ExpiredException(String),
    #[error("third-party risk {0} has an expired review")]
    ExpiredRiskReview(String),
    #[error("critical dependency {0} has unknown unsafe-code exposure")]
    UnknownCriticalUnsafe(String),
    #[error("critical dependency {0} lacks owner or replacement plan")]
    IncompleteCriticalReview(String),
    #[error("dependency {0} uses an unapproved source")]
    UnapprovedSource(String),
    #[error("dependency {package} has prohibited or unknown license {license}")]
    ProhibitedLicense { package: String, license: String },
    #[error("dependency {0} has not been reviewed")]
    UnreviewedDependency(String),
    #[error("SBOM is missing required identity or components")]
    InvalidSbom,
    #[error("SBOM relationship references an absent component")]
    InvalidSbomRelationship,
    #[error("SBOM component {0} differs from reviewed license or source policy")]
    SbomPolicyDrift(String),
    #[error("digest must be a 32-byte sha256 or blake3 hexadecimal digest")]
    InvalidDigest,
    #[error("dependency update {0} lacks required semantic verification")]
    IncompleteUpdateEvidence(String),
    #[error("high-assurance build accessed the network after inputs were staged")]
    BuildNetworkAccess,
    #[error("release signing credentials were exposed to an ordinary build")]
    SigningCredentialExposure,
    #[error("release evidence and SBOM do not identify the same build")]
    ReleaseSbomMismatch,
    #[error("reproducibility report is malformed or unbounded")]
    InvalidReproducibilityReport,
    #[error("reproducibility claim exceeds retained rebuild evidence")]
    OverstatedReproducibility,
    #[error("third-party service inventory is duplicated or unbounded")]
    InvalidServiceInventory,
    #[error("third-party service {0} became authoritative or mandatory when air-gapped")]
    InvalidServiceBoundary(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    fn digest(byte: char) -> String {
        format!("sha256:{}", byte.to_string().repeat(64))
    }

    fn policy() -> DependencyPolicy {
        DependencyPolicy {
            schema_version: POLICY_SCHEMA_VERSION,
            allowed_licenses: BTreeSet::from(["MIT".to_owned(), "Apache-2.0".to_owned()]),
            approved_sources: BTreeSet::from(["crates.io".to_owned()]),
            dependencies: vec![DependencyRecord {
                package: "serde".to_owned(),
                class: DependencyClass::Serialization,
                risk: RiskTier::Tier2,
                license_expression: "MIT OR Apache-2.0".to_owned(),
                source: "crates.io".to_owned(),
                source_kind: SourceKind::ApprovedRegistry,
                native_code: false,
                build_script: false,
                proc_macro: false,
                unsafe_use: UnsafeUse::Contained,
                owner: "protocol".to_owned(),
                capability: "stable serialization traits".to_owned(),
                replacement: "serde-compatible provider boundary".to_owned(),
            }],
            exceptions: Vec::new(),
            risk_acceptances: vec![RiskAcceptance {
                identity: "serde".to_owned(),
                benefit: "shared serialization ecosystem".to_owned(),
                risk: "durable representation drift".to_owned(),
                mitigation: "golden fixtures".to_owned(),
                replacement: "compatibility migration".to_owned(),
                owner: "protocol".to_owned(),
                review_at_unix_seconds: 2_000,
            }],
        }
    }

    fn sbom() -> SbomDocument {
        SbomDocument {
            schema_version: POLICY_SCHEMA_VERSION,
            format: "CycloneDX".to_owned(),
            profile: ArtifactProfile::Server,
            artifact: "aequora-server".to_owned(),
            artifact_digest: digest('a'),
            build_id: "controlled-runner-49".to_owned(),
            source_commit: "0123456789abcdef0123456789abcdef01234567".to_owned(),
            lockfile_digest: digest('b'),
            components: vec![SbomComponent {
                package: "serde".to_owned(),
                version: "1.0.219".to_owned(),
                source: "crates.io".to_owned(),
                checksum: digest('c'),
                license_expression: "MIT OR Apache-2.0".to_owned(),
            }],
            relationships: BTreeMap::from([("serde".to_owned(), BTreeSet::new())]),
        }
    }

    #[test]
    fn policy_fails_closed_for_unknown_license_source_and_unsafe() {
        assert_eq!(policy().validate(1_000), Ok(()));
        let mut invalid = policy();
        invalid.dependencies[0].license_expression = "LicenseRef-Unknown".to_owned();
        assert!(matches!(
            invalid.validate(1_000),
            Err(SupplyChainError::ProhibitedLicense { .. })
        ));
        invalid = policy();
        invalid.dependencies[0].source = "unknown-registry".to_owned();
        assert!(matches!(
            invalid.validate(1_000),
            Err(SupplyChainError::UnapprovedSource(_))
        ));
        invalid = policy();
        invalid.dependencies[0].unsafe_use = UnsafeUse::Unknown;
        assert!(matches!(
            invalid.validate(1_000),
            Err(SupplyChainError::UnknownCriticalUnsafe(_))
        ));
    }

    #[test]
    fn exceptions_and_risk_acceptances_expire() {
        let mut invalid = policy();
        invalid.exceptions.push(PolicyException {
            package: "serde".to_owned(),
            policy: "advisory-RUSTSEC-0000-0000".to_owned(),
            reason: "not reachable in reviewed profile".to_owned(),
            owner: "security".to_owned(),
            expires_at_unix_seconds: 1_000,
        });
        assert_eq!(
            invalid.validate(1_000),
            Err(SupplyChainError::ExpiredException("serde".to_owned()))
        );
        assert!(matches!(
            policy().validate(2_000),
            Err(SupplyChainError::ExpiredRiskReview(_))
        ));
    }

    #[test]
    fn sbom_is_artifact_bound_and_policy_checked() {
        let policy = policy();
        let mut sbom = sbom();
        assert_eq!(sbom.validate(&policy), Ok(()));
        sbom.components[0].source = "substituted-registry".to_owned();
        assert_eq!(
            sbom.validate(&policy),
            Err(SupplyChainError::SbomPolicyDrift("serde".to_owned()))
        );
    }

    #[test]
    fn critical_updates_require_domain_specific_evidence() {
        let mut change = DependencyChangeEvidence {
            package: "sqlx".to_owned(),
            impact: SemanticImpact::Storage,
            passed: BTreeSet::from([VerificationArea::Unit]),
        };
        assert!(matches!(
            change.validate(),
            Err(SupplyChainError::IncompleteUpdateEvidence(_))
        ));
        change.passed.extend([
            VerificationArea::Property,
            VerificationArea::AdapterConformance,
            VerificationArea::Migration,
        ]);
        assert_eq!(change.validate(), Ok(()));
    }

    #[test]
    fn release_evidence_is_offline_credential_isolated_and_sbom_bound() {
        let sbom = sbom();
        let mut evidence = ReleaseEvidence {
            source_commit: sbom.source_commit.clone(),
            lockfile_digest: sbom.lockfile_digest.clone(),
            build_configuration_digest: digest('d'),
            sbom_digest: digest('e'),
            provenance_digest: digest('f'),
            artifact_digest: sbom.artifact_digest.clone(),
            builder_identity: sbom.build_id.clone(),
            rust_toolchain: "rustc-1.87.0".to_owned(),
            target: "x86_64-unknown-linux-gnu".to_owned(),
            network_access_after_staging: false,
            signing_credentials_exposed_to_build: false,
            dependency_changes: Vec::new(),
        };
        assert_eq!(evidence.validate(), Ok(()));
        assert_eq!(evidence.verify_sbom(&sbom), Ok(()));
        evidence.signing_credentials_exposed_to_build = true;
        assert_eq!(
            evidence.validate(),
            Err(SupplyChainError::SigningCredentialExposure)
        );
    }

    #[test]
    fn reproducibility_claim_cannot_exceed_rebuild_evidence() {
        let report = ReproducibilityReport {
            artifact: "aequora-server".to_owned(),
            claimed_level: ReproducibilityLevel::ByteIdentical,
            builder_a_digest: digest('a'),
            builder_b_digest: digest('b'),
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
    fn services_are_replaceable_non_authoritative_and_air_gap_optional() {
        let mut service = ThirdPartyService {
            provider: "managed-postgresql".to_owned(),
            purpose: "authoritative storage hosting".to_owned(),
            data_handled: BTreeSet::from(["encrypted-tenant-data".to_owned()]),
            failure_behavior: "writes stop; no alternate authority is inferred".to_owned(),
            replacement: "compatible PostgreSQL deployment".to_owned(),
            residency: "deployment-selected".to_owned(),
            owner: "storage".to_owned(),
            source_of_authority: false,
            required_in_air_gapped_profile: false,
        };
        assert_eq!(validate_services(&[service.clone()]), Ok(()));
        service.source_of_authority = true;
        assert!(matches!(
            validate_services(&[service]),
            Err(SupplyChainError::InvalidServiceBoundary(_))
        ));
    }
}
