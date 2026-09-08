//! Evidence-bound v1 scope, milestone, support, and GA-readiness contracts.
//!
//! This module deliberately evaluates evidence supplied by release automation. It does not infer
//! production readiness from compiled code, architecture documents, or the existence of tests.

use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use thiserror::Error;

const MAX_ITEMS: usize = 256;
const MAX_TEXT_BYTES: usize = 1_024;

/// Stable v1 product components which may appear on the critical path.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
pub enum V1Component {
    RustSdk,
    PostcardHttps,
    Axum,
    PostgresAuthority,
    NeonPostgresProfile,
    SQLiteLocal,
    StoolapLocal,
    DioxusIntegration,
    CliDevtools,
    TestkitConformance,
}

/// Explicitly deferred capabilities which must not silently enter the v1 critical path.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
pub enum DeferredCapability {
    MultiPrimaryAuthority,
    GenericCrdtFramework,
    PeerToPeerAuthority,
    ConsensusImplementation,
    RuntimePlugins,
    CustomDatabaseEngine,
    CustomTransport,
    UniversalOrm,
    UniversalRowReplication,
    ArbitraryQueryReplication,
}

/// Machine-readable v1 scope freeze.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct V1Scope {
    pub schema_version: u16,
    pub authority: V1Component,
    pub transport: V1Component,
    pub official_local_adapters: BTreeSet<V1Component>,
    pub conditional_local_adapters: BTreeSet<V1Component>,
    pub product_components: BTreeSet<V1Component>,
    pub deferred: BTreeSet<DeferredCapability>,
    pub single_logical_writer: bool,
    pub maximum_official_local_adapters: u8,
}

impl V1Scope {
    /// Parses and validates a checked-in v1 scope manifest.
    ///
    /// # Errors
    ///
    /// Returns an error for malformed RON or a scope which expands or weakens v1.
    pub fn from_ron(input: &str) -> Result<Self, ProductizationError> {
        let scope = ron::from_str(input)
            .map_err(|error| ProductizationError::MalformedManifest(error.to_string()))?;
        Self::validate(&scope)?;
        Ok(scope)
    }

    /// Enforces the frozen PostgreSQL/Postcard/SQLite v1 production path and explicit non-goals.
    ///
    /// # Errors
    ///
    /// Returns an error when a required component is absent or breadth exceeds v1 policy.
    pub fn validate(&self) -> Result<(), ProductizationError> {
        let required_components = BTreeSet::from([
            V1Component::RustSdk,
            V1Component::PostcardHttps,
            V1Component::Axum,
            V1Component::PostgresAuthority,
            V1Component::SQLiteLocal,
            V1Component::CliDevtools,
            V1Component::TestkitConformance,
        ]);
        let local_adapters = BTreeSet::from([V1Component::SQLiteLocal, V1Component::StoolapLocal]);
        let required_deferred = BTreeSet::from([
            DeferredCapability::MultiPrimaryAuthority,
            DeferredCapability::GenericCrdtFramework,
            DeferredCapability::PeerToPeerAuthority,
            DeferredCapability::ConsensusImplementation,
            DeferredCapability::RuntimePlugins,
            DeferredCapability::CustomDatabaseEngine,
            DeferredCapability::CustomTransport,
        ]);
        if self.schema_version != 1
            || self.authority != V1Component::PostgresAuthority
            || self.transport != V1Component::PostcardHttps
            || !self.single_logical_writer
            || self.maximum_official_local_adapters > 2
            || !self
                .official_local_adapters
                .contains(&V1Component::SQLiteLocal)
            || self.official_local_adapters.len()
                > usize::from(self.maximum_official_local_adapters)
            || !self.official_local_adapters.is_subset(&local_adapters)
            || !self.conditional_local_adapters.is_subset(&local_adapters)
            || !required_components.is_subset(&self.product_components)
            || !required_deferred.is_subset(&self.deferred)
            || self
                .official_local_adapters
                .contains(&V1Component::StoolapLocal)
                && self
                    .conditional_local_adapters
                    .contains(&V1Component::StoolapLocal)
        {
            return Err(ProductizationError::InvalidV1Scope);
        }
        Ok(())
    }
}

/// Evidence categories required before an exact release candidate can become GA.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
pub enum GaGate {
    Correctness,
    Storage,
    Protocol,
    Migration,
    Recovery,
    Security,
    Operations,
    Performance,
    ResourceSafety,
    Documentation,
    SupplyChain,
    ReleaseProcess,
    IncidentReadiness,
    Ownership,
}

impl GaGate {
    pub const ALL: [Self; 14] = [
        Self::Correctness,
        Self::Storage,
        Self::Protocol,
        Self::Migration,
        Self::Recovery,
        Self::Security,
        Self::Operations,
        Self::Performance,
        Self::ResourceSafety,
        Self::Documentation,
        Self::SupplyChain,
        Self::ReleaseProcess,
        Self::IncidentReadiness,
        Self::Ownership,
    ];
}

/// Result of one readiness requirement.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum EvidenceStatus {
    Passed,
    Limited,
    Missing,
    NotApplicable,
}

/// Evidence for one GA gate. References identify immutable reports or artifacts.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct GateEvidence {
    pub gate: GaGate,
    pub status: EvidenceStatus,
    pub owner: String,
    pub evidence: Vec<String>,
    pub limitation: Option<String>,
}

/// Support maturity is categorical; one synthetic numeric readiness score is forbidden.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
pub enum Maturity {
    Ready,
    ReadyWithLimitations,
    Experimental,
    Unsupported,
}

/// Evidence-bound claim for a feature, adapter, target, or deployment topology.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SupportClaim {
    pub surface: String,
    pub maturity: Maturity,
    pub target_or_profile: String,
    pub evidence: Vec<String>,
    pub limitations: Vec<String>,
    pub tested_on_claimed_target: bool,
}

impl SupportClaim {
    fn validate(&self) -> Result<(), ProductizationError> {
        validate_text(&self.surface)?;
        validate_text(&self.target_or_profile)?;
        validate_list(&self.evidence)?;
        validate_list(&self.limitations)?;
        match self.maturity {
            Maturity::Ready if self.evidence.is_empty() || !self.tested_on_claimed_target => {
                Err(ProductizationError::UnsupportedClaim(self.surface.clone()))
            }
            Maturity::ReadyWithLimitations
                if self.evidence.is_empty()
                    || self.limitations.is_empty()
                    || !self.tested_on_claimed_target =>
            {
                Err(ProductizationError::UnsupportedClaim(self.surface.clone()))
            }
            Maturity::Unsupported if !self.evidence.is_empty() => {
                Err(ProductizationError::UnsupportedClaim(self.surface.clone()))
            }
            _ => Ok(()),
        }
    }
}

/// Release-defect severity.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
pub enum Severity {
    S0Catastrophic,
    S1Critical,
    S2Major,
    S3Minor,
    S4Cosmetic,
}

/// A known defect considered by the release decision.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct KnownDefect {
    pub id: String,
    pub severity: Severity,
    pub summary: String,
    pub resolved: bool,
    pub correctness_or_security_critical: bool,
    pub workaround: Option<String>,
}

/// High-risk productization areas tracked independently of discovered defects.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
pub enum RiskArea {
    AuthorityTimeline,
    StoolapMaturity,
    MobileLifecycle,
    Migration,
    Snapshot,
    Cryptography,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum RiskStatus {
    Open,
    Mitigated,
    Accepted,
}

/// Owned risk and mitigation record.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RiskRecord {
    pub area: RiskArea,
    pub owner: String,
    pub status: RiskStatus,
    pub mitigation: String,
    pub evidence: Vec<String>,
}

impl KnownDefect {
    fn blocks_ga(&self) -> bool {
        !self.resolved
            && (self.severity <= Severity::S1Critical || self.correctness_or_security_critical)
    }
}

/// Durable state which an advertised upgrade path must preserve.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
pub enum PreservedState {
    DurableIdentifiers,
    PendingOperations,
    CursorState,
    Conflicts,
    AuthorityMetadata,
    AuditGovernanceState,
}

/// Evidence for one supported source-to-target upgrade.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct UpgradeEvidence {
    pub from: String,
    pub to: String,
    pub preserved: BTreeSet<PreservedState>,
    pub evidence: Vec<String>,
    pub includes_pending_operations: bool,
}

impl UpgradeEvidence {
    fn validate(&self) -> Result<(), ProductizationError> {
        let required = BTreeSet::from([
            PreservedState::DurableIdentifiers,
            PreservedState::PendingOperations,
            PreservedState::CursorState,
            PreservedState::Conflicts,
            PreservedState::AuthorityMetadata,
            PreservedState::AuditGovernanceState,
        ]);
        validate_text(&self.from)?;
        validate_text(&self.to)?;
        validate_list(&self.evidence)?;
        if self.evidence.is_empty()
            || !self.includes_pending_operations
            || !required.is_subset(&self.preserved)
        {
            return Err(ProductizationError::IncompleteUpgradeEvidence);
        }
        Ok(())
    }
}

/// Evidence-based implementation milestone.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
pub enum Milestone {
    M0WorkspaceContracts,
    M1OfflineLocalKernel,
    M2AuthorityKernel,
    M3NetworkSync,
    M4ConflictRecovery,
    M5ProductSdk,
    M6Platform,
    M7Operations,
    M8Verification,
    M9Beta,
    M10ReleaseCandidate,
    M11GeneralAvailability,
}

impl Milestone {
    pub const ALL: [Self; 12] = [
        Self::M0WorkspaceContracts,
        Self::M1OfflineLocalKernel,
        Self::M2AuthorityKernel,
        Self::M3NetworkSync,
        Self::M4ConflictRecovery,
        Self::M5ProductSdk,
        Self::M6Platform,
        Self::M7Operations,
        Self::M8Verification,
        Self::M9Beta,
        Self::M10ReleaseCandidate,
        Self::M11GeneralAvailability,
    ];
}

/// Every milestone records all Definition-of-Done artifact classes, including explicit limits.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct MilestoneEvidence {
    pub milestone: Milestone,
    pub complete: bool,
    pub code: Vec<String>,
    pub tests: Vec<String>,
    pub documentation: Vec<String>,
    pub migration_state: Vec<String>,
    pub verification_reports: Vec<String>,
    pub known_limitations: Vec<String>,
}

impl MilestoneEvidence {
    fn validate(&self) -> Result<(), ProductizationError> {
        for list in [
            &self.code,
            &self.tests,
            &self.documentation,
            &self.migration_state,
            &self.verification_reports,
            &self.known_limitations,
        ] {
            validate_list(list)?;
        }
        if self.complete
            && (self.code.is_empty()
                || self.tests.is_empty()
                || self.documentation.is_empty()
                || self.migration_state.is_empty()
                || self.verification_reports.is_empty())
        {
            return Err(ProductizationError::UnevidencedMilestone(self.milestone));
        }
        Ok(())
    }
}

/// Standard subsystem production-readiness review required by Part 50.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ReadinessReview {
    pub subsystem: String,
    pub purpose: String,
    pub owner: String,
    pub dependencies: String,
    pub invariants: String,
    pub failure_modes: String,
    pub recovery: String,
    pub metrics: String,
    pub alerts: String,
    pub capacity: String,
    pub security: String,
    pub migration: String,
    pub backup: String,
    pub tests: String,
    pub known_limits: String,
}

impl ReadinessReview {
    fn validate(&self) -> Result<(), ProductizationError> {
        for field in [
            &self.subsystem,
            &self.purpose,
            &self.owner,
            &self.dependencies,
            &self.invariants,
            &self.failure_modes,
            &self.recovery,
            &self.metrics,
            &self.alerts,
            &self.capacity,
            &self.security,
            &self.migration,
            &self.backup,
            &self.tests,
            &self.known_limits,
        ] {
            validate_text(field)?;
        }
        Ok(())
    }
}

/// Complete release-decision input. Passing validation does not imply GA eligibility.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct GaReadinessManifest {
    pub schema_version: u16,
    pub release_id: String,
    pub release_candidate_digest: [u8; 32],
    pub promoted_digest: [u8; 32],
    pub gates: Vec<GateEvidence>,
    pub support_claims: Vec<SupportClaim>,
    pub known_defects: Vec<KnownDefect>,
    pub risks: Vec<RiskRecord>,
    pub upgrades: Vec<UpgradeEvidence>,
    pub milestones: Vec<MilestoneEvidence>,
    pub reviews: Vec<ReadinessReview>,
}

/// Machine-readable result of evaluating a readiness manifest.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DecisionCheck {
    Satisfied,
    Blocking,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GaEligibility {
    Eligible,
    Blocked,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GaDecision {
    pub eligibility: GaEligibility,
    pub blocking_gates: BTreeSet<GaGate>,
    pub blocking_defects: BTreeSet<String>,
    pub blocking_risks: BTreeSet<RiskArea>,
    pub blocking_milestones: BTreeSet<Milestone>,
    pub upgrade_evidence: DecisionCheck,
    pub readiness_reviews: DecisionCheck,
    pub exact_candidate_artifact: DecisionCheck,
}

impl GaDecision {
    #[must_use]
    pub const fn is_eligible(&self) -> bool {
        matches!(self.eligibility, GaEligibility::Eligible)
    }
}

impl GaReadinessManifest {
    /// Parses a bounded caller-provided RON manifest and evaluates its structure.
    ///
    /// # Errors
    ///
    /// Returns an error for malformed RON or internally inconsistent evidence.
    pub fn from_ron(input: &str) -> Result<Self, ProductizationError> {
        let manifest = ron::from_str(input)
            .map_err(|error| ProductizationError::MalformedManifest(error.to_string()))?;
        Self::validate(&manifest)?;
        Ok(manifest)
    }

    /// Validates identities, unique coverage, support claims, milestones, upgrades, and reviews.
    ///
    /// Missing or failed GA gates are valid inputs and appear as blockers in [`Self::evaluate`].
    ///
    /// # Errors
    ///
    /// Returns an error for duplicate, malformed, contradictory, or unbounded input.
    pub fn validate(&self) -> Result<(), ProductizationError> {
        validate_text(&self.release_id)?;
        if self.schema_version != 1
            || self.gates.len() > MAX_ITEMS
            || self.support_claims.len() > MAX_ITEMS
            || self.known_defects.len() > MAX_ITEMS
            || self.risks.len() > MAX_ITEMS
            || self.upgrades.len() > MAX_ITEMS
            || self.milestones.len() > MAX_ITEMS
            || self.reviews.len() > MAX_ITEMS
            || self.release_candidate_digest == [0; 32]
            || self.promoted_digest == [0; 32]
        {
            return Err(ProductizationError::InvalidReadinessManifest);
        }
        let mut gates = BTreeSet::new();
        for gate in &self.gates {
            validate_text(&gate.owner)?;
            validate_list(&gate.evidence)?;
            if !gates.insert(gate.gate)
                || gate.status == EvidenceStatus::Passed && gate.evidence.is_empty()
                || gate.status == EvidenceStatus::Limited && gate.limitation.is_none()
            {
                return Err(ProductizationError::InvalidGateEvidence(gate.gate));
            }
            if let Some(limitation) = &gate.limitation {
                validate_text(limitation)?;
            }
        }
        let mut claims = BTreeSet::new();
        for claim in &self.support_claims {
            claim.validate()?;
            if !claims.insert(claim.surface.as_str()) {
                return Err(ProductizationError::DuplicateIdentity);
            }
        }
        let mut defects = BTreeSet::new();
        for defect in &self.known_defects {
            validate_text(&defect.id)?;
            validate_text(&defect.summary)?;
            if !defects.insert(defect.id.as_str())
                || !defect.resolved
                    && defect.severity == Severity::S2Major
                    && defect.workaround.is_none()
            {
                return Err(ProductizationError::InvalidKnownDefect);
            }
            if let Some(workaround) = &defect.workaround {
                validate_text(workaround)?;
            }
        }
        let mut risks = BTreeSet::new();
        for risk in &self.risks {
            validate_text(&risk.owner)?;
            validate_text(&risk.mitigation)?;
            validate_list(&risk.evidence)?;
            if !risks.insert(risk.area)
                || risk.status != RiskStatus::Open && risk.evidence.is_empty()
            {
                return Err(ProductizationError::InvalidRiskRecord);
            }
        }
        let mut upgrades = BTreeSet::new();
        for upgrade in &self.upgrades {
            upgrade.validate()?;
            if !upgrades.insert((upgrade.from.as_str(), upgrade.to.as_str())) {
                return Err(ProductizationError::DuplicateIdentity);
            }
        }
        let mut milestones = BTreeSet::new();
        for milestone in &self.milestones {
            milestone.validate()?;
            if !milestones.insert(milestone.milestone) {
                return Err(ProductizationError::DuplicateIdentity);
            }
        }
        let mut reviews = BTreeSet::new();
        for review in &self.reviews {
            review.validate()?;
            if !reviews.insert(review.subsystem.as_str()) {
                return Err(ProductizationError::DuplicateIdentity);
            }
        }
        Ok(())
    }

    /// Produces the fail-closed GA decision without mutating release state.
    ///
    /// # Errors
    ///
    /// Returns an error if the manifest itself is inconsistent.
    pub fn evaluate(&self) -> Result<GaDecision, ProductizationError> {
        self.validate()?;
        let passed = self
            .gates
            .iter()
            .filter(|evidence| evidence.status == EvidenceStatus::Passed)
            .map(|evidence| evidence.gate)
            .collect::<BTreeSet<_>>();
        let blocking_gates = GaGate::ALL
            .into_iter()
            .filter(|gate| !passed.contains(gate))
            .collect::<BTreeSet<_>>();
        let blocking_defects = self
            .known_defects
            .iter()
            .filter(|defect| defect.blocks_ga())
            .map(|defect| defect.id.clone())
            .collect::<BTreeSet<_>>();
        let blocking_risks = self
            .risks
            .iter()
            .filter(|risk| risk.status == RiskStatus::Open)
            .map(|risk| risk.area)
            .collect::<BTreeSet<_>>();
        let completed_milestones = self
            .milestones
            .iter()
            .filter(|milestone| milestone.complete)
            .map(|milestone| milestone.milestone)
            .collect::<BTreeSet<_>>();
        let blocking_milestones = Milestone::ALL
            .into_iter()
            .filter(|milestone| !completed_milestones.contains(milestone))
            .collect::<BTreeSet<_>>();
        let upgrade_evidence = if self.upgrades.is_empty() {
            DecisionCheck::Blocking
        } else {
            DecisionCheck::Satisfied
        };
        let readiness_reviews = if self.reviews.is_empty() {
            DecisionCheck::Blocking
        } else {
            DecisionCheck::Satisfied
        };
        let exact_artifact = self.release_candidate_digest == self.promoted_digest;
        let eligibility = if blocking_gates.is_empty()
            && blocking_defects.is_empty()
            && blocking_risks.is_empty()
            && blocking_milestones.is_empty()
            && upgrade_evidence == DecisionCheck::Satisfied
            && readiness_reviews == DecisionCheck::Satisfied
            && exact_artifact
        {
            GaEligibility::Eligible
        } else {
            GaEligibility::Blocked
        };
        Ok(GaDecision {
            eligibility,
            blocking_gates,
            blocking_defects,
            blocking_risks,
            blocking_milestones,
            upgrade_evidence,
            readiness_reviews,
            exact_candidate_artifact: if exact_artifact {
                DecisionCheck::Satisfied
            } else {
                DecisionCheck::Blocking
            },
        })
    }
}

fn validate_list(values: &[String]) -> Result<(), ProductizationError> {
    if values.len() > MAX_ITEMS {
        return Err(ProductizationError::InputLimit);
    }
    for value in values {
        validate_text(value)?;
    }
    Ok(())
}

fn validate_text(value: &str) -> Result<(), ProductizationError> {
    if value.is_empty() || value.len() > MAX_TEXT_BYTES || value.chars().any(char::is_control) {
        return Err(ProductizationError::InvalidText);
    }
    Ok(())
}

#[derive(Clone, Debug, Eq, Error, PartialEq)]
pub enum ProductizationError {
    #[error("productization manifest is malformed: {0}")]
    MalformedManifest(String),
    #[error("v1 scope violates the frozen product boundary")]
    InvalidV1Scope,
    #[error("GA-readiness manifest is invalid")]
    InvalidReadinessManifest,
    #[error("GA gate evidence is invalid: {0:?}")]
    InvalidGateEvidence(GaGate),
    #[error("support claim exceeds its evidence: {0}")]
    UnsupportedClaim(String),
    #[error("supported upgrade does not preserve all durable state")]
    IncompleteUpgradeEvidence,
    #[error("completed milestone lacks Definition-of-Done evidence: {0:?}")]
    UnevidencedMilestone(Milestone),
    #[error("known defect record is invalid")]
    InvalidKnownDefect,
    #[error("risk record is invalid")]
    InvalidRiskRecord,
    #[error("manifest identities must be unique")]
    DuplicateIdentity,
    #[error("manifest text is empty, unsafe, or too large")]
    InvalidText,
    #[error("manifest collection exceeds its bound")]
    InputLimit,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn gate(gate: GaGate) -> GateEvidence {
        GateEvidence {
            gate,
            status: EvidenceStatus::Passed,
            owner: "release-owner".to_owned(),
            evidence: vec!["immutable-report-digest".to_owned()],
            limitation: None,
        }
    }

    fn manifest() -> GaReadinessManifest {
        GaReadinessManifest {
            schema_version: 1,
            release_id: "aequora-v1.0.0-rc.1".to_owned(),
            release_candidate_digest: [7; 32],
            promoted_digest: [7; 32],
            gates: GaGate::ALL.into_iter().map(gate).collect(),
            support_claims: vec![SupportClaim {
                surface: "sqlite-linux".to_owned(),
                maturity: Maturity::Ready,
                target_or_profile: "x86_64-unknown-linux-gnu".to_owned(),
                evidence: vec!["conformance-report".to_owned()],
                limitations: vec![],
                tested_on_claimed_target: true,
            }],
            known_defects: vec![],
            risks: vec![],
            upgrades: vec![UpgradeEvidence {
                from: "v1.0.0-beta.1".to_owned(),
                to: "v1.0.0-rc.1".to_owned(),
                preserved: BTreeSet::from([
                    PreservedState::DurableIdentifiers,
                    PreservedState::PendingOperations,
                    PreservedState::CursorState,
                    PreservedState::Conflicts,
                    PreservedState::AuthorityMetadata,
                    PreservedState::AuditGovernanceState,
                ]),
                evidence: vec!["upgrade-report".to_owned()],
                includes_pending_operations: true,
            }],
            milestones: Milestone::ALL
                .into_iter()
                .map(|milestone| MilestoneEvidence {
                    milestone,
                    complete: true,
                    code: vec!["source-digest".to_owned()],
                    tests: vec!["test-report".to_owned()],
                    documentation: vec!["documentation-report".to_owned()],
                    migration_state: vec!["migration-report-or-explicit-na".to_owned()],
                    verification_reports: vec!["verification-report".to_owned()],
                    known_limitations: vec!["none-known".to_owned()],
                })
                .collect(),
            reviews: vec![ReadinessReview {
                subsystem: "release".to_owned(),
                purpose: "release-decision".to_owned(),
                owner: "release-owner".to_owned(),
                dependencies: "declared".to_owned(),
                invariants: "registered".to_owned(),
                failure_modes: "documented".to_owned(),
                recovery: "documented".to_owned(),
                metrics: "documented".to_owned(),
                alerts: "documented".to_owned(),
                capacity: "measured".to_owned(),
                security: "reviewed".to_owned(),
                migration: "tested".to_owned(),
                backup: "tested".to_owned(),
                tests: "passed".to_owned(),
                known_limits: "none-known".to_owned(),
            }],
        }
    }

    #[test]
    fn every_gate_and_identical_bytes_allow_ga() {
        let decision = manifest()
            .evaluate()
            .unwrap_or_else(|error| panic!("manifest must evaluate: {error}"));
        assert!(decision.is_eligible());
        assert!(decision.blocking_gates.is_empty());
    }

    #[test]
    fn missing_gate_critical_defect_and_rebuild_block_ga() {
        let mut manifest = manifest();
        manifest.gates.retain(|gate| gate.gate != GaGate::Security);
        manifest.promoted_digest = [8; 32];
        manifest.known_defects.push(KnownDefect {
            id: "AEQ-1".to_owned(),
            severity: Severity::S1Critical,
            summary: "lost pending operation".to_owned(),
            resolved: false,
            correctness_or_security_critical: true,
            workaround: None,
        });
        let decision = manifest
            .evaluate()
            .unwrap_or_else(|error| panic!("manifest must evaluate: {error}"));
        assert!(!decision.is_eligible());
        assert!(decision.blocking_gates.contains(&GaGate::Security));
        assert!(decision.blocking_defects.contains("AEQ-1"));
    }

    #[test]
    fn claims_cannot_exceed_target_evidence() {
        let mut manifest = manifest();
        manifest.support_claims[0].tested_on_claimed_target = false;
        assert!(matches!(
            manifest.validate(),
            Err(ProductizationError::UnsupportedClaim(_))
        ));
    }

    #[test]
    fn completed_milestone_requires_every_artifact_class() {
        let mut milestone = MilestoneEvidence {
            milestone: Milestone::M1OfflineLocalKernel,
            complete: true,
            code: vec!["local-store".to_owned()],
            tests: vec!["tx-a-fault".to_owned()],
            documentation: vec!["local-store-guide".to_owned()],
            migration_state: vec!["migration-0001".to_owned()],
            verification_reports: vec![],
            known_limitations: vec!["none-known".to_owned()],
        };
        assert!(milestone.validate().is_err());
        milestone
            .verification_reports
            .push("report-digest".to_owned());
        assert_eq!(milestone.validate(), Ok(()));
    }

    #[test]
    fn scope_requires_postgres_postcard_sqlite_and_explicit_non_goals() {
        let scope = V1Scope::from_ron(include_str!("../../../release/v1-scope.ron"))
            .unwrap_or_else(|error| panic!("checked-in v1 scope must validate: {error}"));
        assert_eq!(scope.authority, V1Component::PostgresAuthority);
        assert!(
            scope
                .official_local_adapters
                .contains(&V1Component::SQLiteLocal)
        );
    }

    #[test]
    fn checked_in_development_snapshot_is_valid_but_does_not_claim_ga() {
        let readiness =
            GaReadinessManifest::from_ron(include_str!("../../../release/v1-readiness.ron"))
                .unwrap_or_else(|error| {
                    panic!("checked-in readiness snapshot must validate: {error}")
                });
        let decision = readiness
            .evaluate()
            .unwrap_or_else(|error| panic!("snapshot must evaluate: {error}"));
        assert!(!decision.is_eligible());
        assert_eq!(decision.exact_candidate_artifact, DecisionCheck::Blocking);
        assert!(!decision.blocking_gates.is_empty());
        assert_eq!(decision.blocking_milestones.len(), Milestone::ALL.len());
    }
}
