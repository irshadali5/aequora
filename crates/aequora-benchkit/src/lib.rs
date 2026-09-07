//! Runtime-, transport-, database-, and vendor-neutral performance evidence contracts.

use aequora_invariants::InvariantId;
use aequora_workload::{ScenarioId, WorkloadSpec};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use thiserror::Error;

pub const BENCHMARK_FORMAT_VERSION: u32 = 1;
pub const MAX_SAMPLES: usize = 100_000;
pub const MAX_TEXT_BYTES: usize = 512;

pub const BENCHMARK_INVARIANTS: [InvariantId; 10] = [
    InvariantId::BenchmarkSemanticIntegrity,
    InvariantId::BenchmarkEvidenceBinding,
    InvariantId::BenchmarkCorrectnessUnderLoad,
    InvariantId::BenchmarkResourceBounds,
    InvariantId::BenchmarkCapacityHeadroom,
    InvariantId::BenchmarkCompatibleRegression,
    InvariantId::BenchmarkAdapterParity,
    InvariantId::BenchmarkRepresentativeWorkload,
    InvariantId::BenchmarkEvidenceClassification,
    InvariantId::BenchmarkMeasuredOptimization,
];

#[derive(Clone, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct BenchmarkId(String);

impl BenchmarkId {
    /// Creates an `AEQ-BENCH-*` stable identifier.
    ///
    /// # Errors
    ///
    /// Rejects malformed or oversized identifiers.
    pub fn new(value: impl Into<String>) -> Result<Self, BenchmarkError> {
        let value = value.into();
        if !value.starts_with("AEQ-BENCH-")
            || value.len() > 96
            || !value
                .bytes()
                .all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit() || byte == b'-')
        {
            return Err(BenchmarkError::InvalidIdentity);
        }
        Ok(Self(value))
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum BenchmarkLayer {
    Micro,
    Component,
    Adapter,
    Protocol,
    EndToEnd,
    Concurrency,
    Scalability,
    Soak,
    ResourceConstrained,
    FailureDegraded,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum BenchmarkMode {
    Throughput,
    Latency,
    Saturation,
    Scalability,
    Burst,
    Recovery,
    Soak,
    Resource,
    Failure,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
pub enum CorrectnessFeature {
    Durability,
    Idempotency,
    Authentication,
    Authorization,
    Validation,
    Audit,
    ConflictSemantics,
    Encryption,
    Integrity,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct CorrectnessProfile {
    pub required: BTreeSet<CorrectnessFeature>,
    pub enabled: BTreeSet<CorrectnessFeature>,
}

impl CorrectnessProfile {
    /// Ensures every production-required semantic feature is enabled.
    ///
    /// # Errors
    ///
    /// Returns the benchmark safety error on any weakened feature.
    pub fn validate(&self) -> Result<(), BenchmarkError> {
        if self.required.is_empty() || !self.required.is_subset(&self.enabled) {
            return Err(BenchmarkError::WeakenedCorrectness);
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct EnvironmentFingerprint {
    pub operating_system: String,
    pub kernel: String,
    pub architecture: String,
    pub cpu_model: String,
    pub cpu_count: u16,
    pub memory_bytes: u64,
    pub storage_class: String,
    pub filesystem: String,
    pub rust_compiler: String,
    pub database_version: Option<String>,
    pub adapter_version: String,
    pub network_topology: String,
    pub virtualization: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct BuildFingerprint {
    pub build_id: String,
    pub git_commit: String,
    pub release_build: bool,
    pub lto: bool,
    pub codegen_units: u16,
    pub target_cpu: String,
    pub features: BTreeSet<String>,
    pub allocator: String,
    pub panic_strategy: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct DatasetFingerprint {
    pub digest: [u8; 32],
    pub tenant_count: u32,
    pub entity_count: u64,
    pub journal_events: u64,
    pub ledger_records: u64,
    pub outbox_operations: u64,
    pub snapshot_bytes: u64,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct BenchmarkManifest {
    pub format_version: u32,
    pub benchmark_id: BenchmarkId,
    pub scenario: ScenarioId,
    pub scenario_version: u32,
    pub layer: BenchmarkLayer,
    pub mode: BenchmarkMode,
    pub build: BuildFingerprint,
    pub environment: EnvironmentFingerprint,
    pub dataset: DatasetFingerprint,
    pub workload_fingerprint: [u8; 32],
    pub correctness: CorrectnessProfile,
    pub started_at_unix_millis: u64,
}

impl BenchmarkManifest {
    /// Validates complete, reproducible and correctness-preserving run metadata.
    ///
    /// # Errors
    ///
    /// Rejects missing fingerprints, unsafe correctness, or incomplete environment data.
    pub fn validate(&self) -> Result<(), BenchmarkError> {
        self.correctness.validate()?;
        let required_text = [
            self.build.build_id.as_str(),
            self.build.git_commit.as_str(),
            self.build.target_cpu.as_str(),
            self.build.allocator.as_str(),
            self.build.panic_strategy.as_str(),
            self.environment.operating_system.as_str(),
            self.environment.kernel.as_str(),
            self.environment.architecture.as_str(),
            self.environment.cpu_model.as_str(),
            self.environment.storage_class.as_str(),
            self.environment.filesystem.as_str(),
            self.environment.rust_compiler.as_str(),
            self.environment.adapter_version.as_str(),
            self.environment.network_topology.as_str(),
            self.environment.virtualization.as_str(),
        ];
        if self.format_version != BENCHMARK_FORMAT_VERSION
            || self.scenario_version == 0
            || self.started_at_unix_millis == 0
            || self.environment.cpu_count == 0
            || self.environment.memory_bytes == 0
            || self.build.codegen_units == 0
            || self.dataset.digest == [0; 32]
            || self.workload_fingerprint == [0; 32]
            || required_text
                .iter()
                .any(|text| text.is_empty() || text.len() > MAX_TEXT_BYTES)
        {
            return Err(BenchmarkError::IncompleteManifest);
        }
        Ok(())
    }

    #[must_use]
    pub fn comparable_to(&self, other: &Self) -> bool {
        self.benchmark_id == other.benchmark_id
            && self.scenario == other.scenario
            && self.scenario_version == other.scenario_version
            && self.workload_fingerprint == other.workload_fingerprint
            && self.environment == other.environment
            && self.correctness == other.correctness
            && self.build.release_build == other.build.release_build
            && self.build.lto == other.build.lto
            && self.build.codegen_units == other.build.codegen_units
            && self.build.target_cpu == other.build.target_cpu
            && self.build.features == other.build.features
            && self.build.allocator == other.build.allocator
            && self.build.panic_strategy == other.build.panic_strategy
    }
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct LoadCounters {
    pub offered: u64,
    pub admitted: u64,
    pub accepted: u64,
    pub rejected: u64,
    pub completed: u64,
    pub authoritative: u64,
    pub goodput: u64,
}

impl LoadCounters {
    const fn valid(self) -> bool {
        self.admitted <= self.offered
            && self.accepted <= self.admitted
            && self.rejected <= self.offered
            && self.completed <= self.accepted
            && self.authoritative <= self.completed
            && self.goodput <= self.authoritative
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct LatencyDistribution {
    pub sample_count: u64,
    pub p50_micros: u64,
    pub p90_micros: u64,
    pub p95_micros: u64,
    pub p99_micros: u64,
    pub p999_micros: Option<u64>,
    pub maximum_micros: u64,
}

impl LatencyDistribution {
    const fn valid(&self) -> bool {
        self.sample_count > 0
            && self.p50_micros <= self.p90_micros
            && self.p90_micros <= self.p95_micros
            && self.p95_micros <= self.p99_micros
            && self.p99_micros <= self.maximum_micros
            && match self.p999_micros {
                Some(value) => value >= self.p99_micros && value <= self.maximum_micros,
                None => true,
            }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ResourceStats {
    pub peak_resident_bytes: u64,
    pub peak_heap_bytes: u64,
    pub allocations: u64,
    pub cpu_millis: u64,
    pub network_bytes: u64,
    pub storage_bytes: u64,
    pub maximum_queue_depth: u32,
    pub generator_saturated: bool,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct CorrectnessResult {
    pub duplicate_authoritative_effects: u64,
    pub cursor_skips: u64,
    pub lost_accepted_operations: u64,
    pub cross_tenant_leaks: u64,
    pub broken_versions: u64,
    pub invalid_journal_orderings: u64,
}

impl CorrectnessResult {
    #[must_use]
    pub const fn passed(&self) -> bool {
        self.duplicate_authoritative_effects == 0
            && self.cursor_skips == 0
            && self.lost_accepted_operations == 0
            && self.cross_tenant_leaks == 0
            && self.broken_versions == 0
            && self.invalid_journal_orderings == 0
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct BenchmarkResult {
    pub manifest: BenchmarkManifest,
    pub repetitions: u16,
    pub warmup_seconds: u32,
    pub counters: LoadCounters,
    pub latency: LatencyDistribution,
    pub resources: ResourceStats,
    pub correctness: CorrectnessResult,
    pub raw_samples_micros: Vec<u64>,
}

impl BenchmarkResult {
    /// Validates bounded samples, tail latency, load accounting, and correctness.
    ///
    /// # Errors
    ///
    /// Rejects incomplete, inconsistent, saturated-generator, or incorrect evidence.
    pub fn validate(&self) -> Result<(), BenchmarkError> {
        self.manifest.validate()?;
        if self.repetitions == 0
            || !self.counters.valid()
            || !self.latency.valid()
            || self.raw_samples_micros.is_empty()
            || self.raw_samples_micros.len() > MAX_SAMPLES
            || self.resources.generator_saturated
        {
            return Err(BenchmarkError::InvalidResult);
        }
        if !self.correctness.passed() {
            return Err(BenchmarkError::CorrectnessFailure);
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum MetricDirection {
    LowerIsBetter,
    HigherIsBetter,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum Significance {
    Meaningful,
    Noisy,
    InsufficientSamples,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum RegressionDecision {
    Pass,
    Warn,
    Fail,
    AcceptedWithJustification,
    Inconclusive,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct RegressionPolicy {
    pub direction: MetricDirection,
    pub warning_basis_points: u16,
    pub failure_basis_points: u16,
    pub minimum_samples: u64,
    pub maximum_noise_basis_points: u16,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct RegressionComparison {
    pub benchmark_id: BenchmarkId,
    pub baseline_value: u64,
    pub candidate_value: u64,
    pub delta_basis_points: i64,
    pub significance: Significance,
    pub decision: RegressionDecision,
    pub justification: Option<String>,
}

/// Compares compatible benchmark p99 or goodput evidence without false precision.
///
/// # Errors
///
/// Rejects malformed results or invalid policies. Environment drift yields `Inconclusive`.
pub fn compare(
    baseline: &BenchmarkResult,
    candidate: &BenchmarkResult,
    policy: &RegressionPolicy,
) -> Result<RegressionComparison, BenchmarkError> {
    baseline.validate()?;
    candidate.validate()?;
    if policy.warning_basis_points > policy.failure_basis_points
        || policy.failure_basis_points > 10_000
        || policy.maximum_noise_basis_points > policy.warning_basis_points
        || policy.minimum_samples == 0
    {
        return Err(BenchmarkError::InvalidRegressionPolicy);
    }
    let (baseline_value, candidate_value) = match policy.direction {
        MetricDirection::LowerIsBetter => {
            (baseline.latency.p99_micros, candidate.latency.p99_micros)
        }
        MetricDirection::HigherIsBetter => (baseline.counters.goodput, candidate.counters.goodput),
    };
    if !baseline.manifest.comparable_to(&candidate.manifest) {
        return Ok(comparison(
            baseline,
            baseline_value,
            candidate_value,
            0,
            Significance::Noisy,
            RegressionDecision::Inconclusive,
        ));
    }
    if baseline.latency.sample_count < policy.minimum_samples
        || candidate.latency.sample_count < policy.minimum_samples
        || baseline_value == 0
    {
        return Ok(comparison(
            baseline,
            baseline_value,
            candidate_value,
            0,
            Significance::InsufficientSamples,
            RegressionDecision::Inconclusive,
        ));
    }
    let signed = match policy.direction {
        MetricDirection::LowerIsBetter => i128::from(candidate_value) - i128::from(baseline_value),
        MetricDirection::HigherIsBetter => i128::from(baseline_value) - i128::from(candidate_value),
    };
    let delta = signed.saturating_mul(10_000) / i128::from(baseline_value);
    let delta = i64::try_from(delta).unwrap_or(if delta.is_negative() {
        i64::MIN
    } else {
        i64::MAX
    });
    let harmful = u16::try_from(delta.max(0)).unwrap_or(u16::MAX);
    let (significance, decision) = if harmful <= policy.maximum_noise_basis_points {
        (Significance::Noisy, RegressionDecision::Inconclusive)
    } else if harmful >= policy.failure_basis_points {
        (Significance::Meaningful, RegressionDecision::Fail)
    } else if harmful >= policy.warning_basis_points {
        (Significance::Meaningful, RegressionDecision::Warn)
    } else {
        (Significance::Meaningful, RegressionDecision::Pass)
    };
    Ok(comparison(
        baseline,
        baseline_value,
        candidate_value,
        delta,
        significance,
        decision,
    ))
}

fn comparison(
    baseline: &BenchmarkResult,
    baseline_value: u64,
    candidate_value: u64,
    delta_basis_points: i64,
    significance: Significance,
    decision: RegressionDecision,
) -> RegressionComparison {
    RegressionComparison {
        benchmark_id: baseline.manifest.benchmark_id.clone(),
        baseline_value,
        candidate_value,
        delta_basis_points,
        significance,
        decision,
        justification: None,
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum EvidenceClass {
    Measured,
    Interpolated,
    Extrapolated,
    Unknown,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct CapacityEvidence {
    pub classification: EvidenceClass,
    pub sustainable_goodput_per_second: Option<u64>,
    pub tested_peak_operations_per_second: Option<u64>,
    pub source_benchmark: Option<BenchmarkId>,
    pub evidence_bundle: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct CapacityInputs {
    pub active_tenants: u32,
    pub active_clients: u64,
    pub operations_per_client_hour: u32,
    pub peak_multiplier_basis_points: u32,
    pub batch_size: u32,
    pub safety_headroom_basis_points: u16,
    pub api_node_count: u16,
    pub survive_one_node_loss: bool,
    pub journal_bytes_per_operation: u64,
    pub ledger_bytes_per_operation: u64,
    pub audit_bytes_per_operation: u64,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct CapacityEstimate {
    pub classification: EvidenceClass,
    pub average_operations_per_second: u64,
    pub offered_peak_operations_per_second: u64,
    pub safe_capacity_per_node: Option<u64>,
    pub required_api_nodes: Option<u16>,
    pub daily_journal_bytes: u64,
    pub daily_ledger_bytes: u64,
    pub daily_audit_bytes: u64,
    pub certified: bool,
    pub risk_notes: Vec<String>,
}

/// Produces conservative capacity guidance with explicit headroom and provenance.
///
/// # Errors
///
/// Rejects zero dimensions, absent measured evidence, or unsafe headroom.
pub fn estimate_capacity(
    workload: &WorkloadSpec,
    inputs: &CapacityInputs,
    evidence: &CapacityEvidence,
) -> Result<CapacityEstimate, BenchmarkError> {
    workload
        .validate()
        .map_err(|_| BenchmarkError::InvalidWorkload)?;
    if inputs.active_tenants == 0
        || inputs.active_clients == 0
        || inputs.operations_per_client_hour == 0
        || inputs.peak_multiplier_basis_points < 10_000
        || inputs.batch_size == 0
        || inputs.api_node_count == 0
        || inputs.safety_headroom_basis_points == 0
        || inputs.safety_headroom_basis_points >= 10_000
    {
        return Err(BenchmarkError::InvalidCapacityInputs);
    }
    if matches!(evidence.classification, EvidenceClass::Measured)
        && (evidence.sustainable_goodput_per_second.is_none()
            || evidence.source_benchmark.is_none()
            || evidence
                .evidence_bundle
                .as_deref()
                .is_none_or(str::is_empty))
    {
        return Err(BenchmarkError::MissingCapacityEvidence);
    }
    let hourly = inputs
        .active_clients
        .saturating_mul(u64::from(inputs.operations_per_client_hour));
    let average = hourly.div_ceil(3_600);
    let peak = average
        .saturating_mul(u64::from(inputs.peak_multiplier_basis_points))
        .div_ceil(10_000);
    let daily_operations = hourly.saturating_mul(24);
    let sustainable = evidence.sustainable_goodput_per_second;
    let safe = sustainable
        .map(|value| value.saturating_mul(u64::from(inputs.safety_headroom_basis_points)) / 10_000);
    let required = safe.and_then(|per_node| {
        if per_node == 0 {
            None
        } else {
            let nodes = peak
                .div_ceil(per_node)
                .saturating_add(u64::from(inputs.survive_one_node_loss));
            u16::try_from(nodes).ok()
        }
    });
    let mut risks = Vec::new();
    if !matches!(evidence.classification, EvidenceClass::Measured) {
        risks.push("planning guidance is not capacity certification".to_owned());
    }
    if required.is_some_and(|nodes| nodes > inputs.api_node_count) {
        risks.push("configured API node count is below the modeled requirement".to_owned());
    }
    Ok(CapacityEstimate {
        classification: evidence.classification,
        average_operations_per_second: average,
        offered_peak_operations_per_second: peak,
        safe_capacity_per_node: safe,
        required_api_nodes: required,
        daily_journal_bytes: daily_operations.saturating_mul(inputs.journal_bytes_per_operation),
        daily_ledger_bytes: daily_operations.saturating_mul(inputs.ledger_bytes_per_operation),
        daily_audit_bytes: daily_operations.saturating_mul(inputs.audit_bytes_per_operation),
        certified: false,
        risk_notes: risks,
    })
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum BenchmarkTarget {
    NonProduction,
    Production,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
pub enum ProductionSafeguard {
    Allowlisted,
    OperatorConfirmed,
    TenantIsolated,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ProductionTargetGuard {
    pub target: BenchmarkTarget,
    pub safeguards: BTreeSet<ProductionSafeguard>,
    pub rate_cap_operations_per_second: Option<u32>,
}

impl ProductionTargetGuard {
    /// Blocks accidental production load and requires every explicit production safeguard.
    ///
    /// # Errors
    ///
    /// Returns an authorization error when production use is not fully guarded.
    pub fn authorize(&self) -> Result<(), BenchmarkError> {
        let required = BTreeSet::from([
            ProductionSafeguard::Allowlisted,
            ProductionSafeguard::OperatorConfirmed,
            ProductionSafeguard::TenantIsolated,
        ]);
        if self.target == BenchmarkTarget::Production
            && (!required.is_subset(&self.safeguards)
                || self
                    .rate_cap_operations_per_second
                    .is_none_or(|cap| cap == 0))
        {
            return Err(BenchmarkError::ProductionTargetDenied);
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Error, Eq, PartialEq)]
pub enum BenchmarkError {
    #[error("benchmark identity is invalid")]
    InvalidIdentity,
    #[error("benchmark weakened a required correctness feature")]
    WeakenedCorrectness,
    #[error("benchmark manifest is incomplete")]
    IncompleteManifest,
    #[error("benchmark result is inconsistent, unbounded, or generator-saturated")]
    InvalidResult,
    #[error("correctness failed under benchmark load")]
    CorrectnessFailure,
    #[error("regression policy is invalid")]
    InvalidRegressionPolicy,
    #[error("workload is invalid")]
    InvalidWorkload,
    #[error("capacity inputs or headroom are invalid")]
    InvalidCapacityInputs,
    #[error("measured capacity requires a benchmark and evidence bundle")]
    MissingCapacityEvidence,
    #[error("production load target is not explicitly authorized and bounded")]
    ProductionTargetDenied,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn correctness_features_cannot_be_removed_for_speed() {
        let profile = CorrectnessProfile {
            required: BTreeSet::from([CorrectnessFeature::Durability, CorrectnessFeature::Audit]),
            enabled: BTreeSet::from([CorrectnessFeature::Durability]),
        };
        assert_eq!(profile.validate(), Err(BenchmarkError::WeakenedCorrectness));
    }

    #[test]
    fn production_target_requires_every_guard() {
        let guard = ProductionTargetGuard {
            target: BenchmarkTarget::Production,
            safeguards: BTreeSet::from([
                ProductionSafeguard::Allowlisted,
                ProductionSafeguard::TenantIsolated,
            ]),
            rate_cap_operations_per_second: Some(100),
        };
        assert_eq!(
            guard.authorize(),
            Err(BenchmarkError::ProductionTargetDenied)
        );
    }

    #[test]
    fn invariant_catalog_is_complete() {
        assert_eq!(BENCHMARK_INVARIANTS.len(), 10);
        assert_eq!(BENCHMARK_INVARIANTS[0].as_str(), "AEQ-INV-BENCH001");
        assert_eq!(BENCHMARK_INVARIANTS[9].as_str(), "AEQ-INV-BENCH010");
    }

    #[test]
    fn unknown_capacity_never_becomes_certification() {
        let evidence = CapacityEvidence {
            classification: EvidenceClass::Unknown,
            sustainable_goodput_per_second: None,
            tested_peak_operations_per_second: None,
            source_benchmark: None,
            evidence_bundle: None,
        };
        assert_eq!(evidence.classification, EvidenceClass::Unknown);
        assert!(evidence.sustainable_goodput_per_second.is_none());
    }
}
