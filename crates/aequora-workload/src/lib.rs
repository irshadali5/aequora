//! Deterministic, bounded, and domain-representative workload definitions.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use thiserror::Error;

pub const WORKLOAD_FORMAT_VERSION: u32 = 1;
pub const MAX_OPERATION_KINDS: usize = 64;
pub const MAX_SCENARIO_TEXT_BYTES: usize = 96;

#[derive(Clone, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct ScenarioId(String);

impl ScenarioId {
    /// Creates a stable scenario identity such as `school_standard/v1`.
    ///
    /// # Errors
    ///
    /// Rejects empty, oversized, or non-portable identities.
    pub fn new(value: impl Into<String>) -> Result<Self, WorkloadError> {
        let value = value.into();
        if value.is_empty()
            || value.len() > MAX_SCENARIO_TEXT_BYTES
            || !value.bytes().all(|byte| {
                byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'/' | b'.')
            })
        {
            return Err(WorkloadError::InvalidScenarioId);
        }
        Ok(Self(value))
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum DomainProfile {
    GenericCrud,
    SchoolErp,
    FinanceErp,
    Inventory,
    FieldService,
    DocumentMetadata,
    MessagingMetadata,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum ArrivalModel {
    Constant,
    PoissonLike,
    Burst,
    Scheduled,
    ReconnectStorm,
    Ramp,
    Step,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum LoadModel {
    Closed,
    Open,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum NetworkProfile {
    Lan,
    Broadband,
    Cellular,
    HighLatencyWan,
    LossyMobile,
    MeteredSlow,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum CacheState {
    Cold,
    Warm,
    Mixed,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum DatasetReset {
    Fresh,
    Preloaded,
    RetainedJournal,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct PercentileBytes {
    pub p50: u32,
    pub p95: u32,
    pub p99: u32,
}

impl PercentileBytes {
    const fn valid(self) -> bool {
        self.p50 > 0 && self.p50 <= self.p95 && self.p95 <= self.p99
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct NetworkModel {
    pub profile: NetworkProfile,
    pub latency_millis: u32,
    pub jitter_millis: u32,
    pub loss_basis_points: u16,
    pub bandwidth_bytes_per_second: u64,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ConflictModel {
    pub conflicting_operations_basis_points: u16,
    pub hot_entity_basis_points: u16,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct DependencyModel {
    pub dependent_operations_basis_points: u16,
    pub maximum_depth: u16,
    pub maximum_edges_per_operation: u16,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct WorkloadSpec {
    pub format_version: u32,
    pub scenario: ScenarioId,
    pub domain: DomainProfile,
    pub dataset_seed: u64,
    pub tenants: u32,
    pub clients: u32,
    pub active_clients: u32,
    pub duration_seconds: u64,
    pub arrival: ArrivalModel,
    pub load_model: LoadModel,
    pub offered_operations_per_second: u32,
    pub maximum_in_flight: u32,
    pub maximum_queue_depth: u32,
    pub operation_mix_basis_points: BTreeMap<String, u16>,
    pub payload_bytes: PercentileBytes,
    pub offline_clients_basis_points: u16,
    pub reconnect_window_seconds: u32,
    pub network: NetworkModel,
    pub conflicts: ConflictModel,
    pub dependencies: DependencyModel,
    pub cache_state: CacheState,
    pub dataset_reset: DatasetReset,
    pub synthetic_or_sanitized: bool,
}

impl WorkloadSpec {
    /// Validates finite load, bounded distributions, privacy, and realistic client state.
    ///
    /// # Errors
    ///
    /// Returns a stable error when the workload is unsafe or incomplete.
    pub fn validate(&self) -> Result<(), WorkloadError> {
        if self.format_version != WORKLOAD_FORMAT_VERSION {
            return Err(WorkloadError::UnsupportedVersion);
        }
        if self.dataset_seed == 0
            || self.tenants == 0
            || self.clients == 0
            || self.active_clients == 0
            || self.active_clients > self.clients
            || self.duration_seconds == 0
            || self.offered_operations_per_second == 0
            || self.maximum_in_flight == 0
            || self.maximum_queue_depth == 0
            || !self.payload_bytes.valid()
            || self.network.bandwidth_bytes_per_second == 0
            || self.network.loss_basis_points > 10_000
            || self.offline_clients_basis_points > 10_000
            || self.conflicts.conflicting_operations_basis_points > 10_000
            || self.conflicts.hot_entity_basis_points > 10_000
            || self.dependencies.dependent_operations_basis_points > 10_000
        {
            return Err(WorkloadError::InvalidBounds);
        }
        if self.operation_mix_basis_points.is_empty()
            || self.operation_mix_basis_points.len() > MAX_OPERATION_KINDS
            || self.operation_mix_basis_points.keys().any(|name| {
                name.is_empty()
                    || name.len() > MAX_SCENARIO_TEXT_BYTES
                    || !name.bytes().all(|byte| {
                        byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.')
                    })
            })
            || self
                .operation_mix_basis_points
                .values()
                .map(|value| u32::from(*value))
                .sum::<u32>()
                != 10_000
        {
            return Err(WorkloadError::InvalidOperationMix);
        }
        if !self.synthetic_or_sanitized {
            return Err(WorkloadError::SensitiveDataset);
        }
        if matches!(self.arrival, ArrivalModel::ReconnectStorm)
            && (self.offline_clients_basis_points == 0 || self.reconnect_window_seconds == 0)
        {
            return Err(WorkloadError::InvalidReconnectModel);
        }
        Ok(())
    }

    /// Returns a canonical content digest for result and baseline binding.
    ///
    /// # Errors
    ///
    /// Returns an error when validation or canonical encoding fails.
    pub fn fingerprint(&self) -> Result<[u8; 32], WorkloadError> {
        self.validate()?;
        let bytes = postcard::to_stdvec(self).map_err(|_| WorkloadError::Encoding)?;
        Ok(*blake3::hash(&bytes).as_bytes())
    }

    #[must_use]
    pub fn total_offered_operations(&self) -> u64 {
        u64::from(self.offered_operations_per_second).saturating_mul(self.duration_seconds)
    }
}

/// A tiny deterministic generator used for repeatable workload choices, not cryptography.
#[derive(Clone, Copy, Debug)]
pub struct DatasetGenerator {
    state: u64,
}

impl DatasetGenerator {
    /// Starts a generator from the workload seed.
    ///
    /// # Errors
    ///
    /// Rejects the reserved zero seed.
    pub const fn new(seed: u64) -> Result<Self, WorkloadError> {
        if seed == 0 {
            Err(WorkloadError::InvalidBounds)
        } else {
            Ok(Self { state: seed })
        }
    }

    #[must_use]
    pub const fn next_u64(&mut self) -> u64 {
        let mut value = self.state;
        value ^= value << 13;
        value ^= value >> 7;
        value ^= value << 17;
        self.state = value;
        value
    }

    #[must_use]
    pub fn choose_basis_points(&mut self) -> u16 {
        u16::try_from(self.next_u64() % 10_000).unwrap_or_default()
    }
}

#[derive(Clone, Copy, Debug, Error, Eq, PartialEq)]
pub enum WorkloadError {
    #[error("scenario identity is invalid")]
    InvalidScenarioId,
    #[error("workload format version is unsupported")]
    UnsupportedVersion,
    #[error("workload contains an empty or unbounded dimension")]
    InvalidBounds,
    #[error("operation mix must contain bounded names totaling 10,000 basis points")]
    InvalidOperationMix,
    #[error("reconnect storm requires offline clients and a reconnect window")]
    InvalidReconnectModel,
    #[error("benchmark data must be synthetic or sanitized")]
    SensitiveDataset,
    #[error("workload canonical encoding failed")]
    Encoding,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn workload() -> Result<WorkloadSpec, Box<dyn std::error::Error>> {
        Ok(WorkloadSpec {
            format_version: 1,
            scenario: ScenarioId::new("school_standard/v1")?,
            domain: DomainProfile::SchoolErp,
            dataset_seed: 47,
            tenants: 100,
            clients: 10_000,
            active_clients: 2_000,
            duration_seconds: 60,
            arrival: ArrivalModel::Burst,
            load_model: LoadModel::Open,
            offered_operations_per_second: 1_000,
            maximum_in_flight: 512,
            maximum_queue_depth: 2_048,
            operation_mix_basis_points: BTreeMap::from([
                ("attendance.record".into(), 4_500),
                ("student.update".into(), 1_500),
                ("fee.record".into(), 1_000),
                ("read.sync".into(), 3_000),
            ]),
            payload_bytes: PercentileBytes {
                p50: 400,
                p95: 2_000,
                p99: 8_000,
            },
            offline_clients_basis_points: 2_000,
            reconnect_window_seconds: 30,
            network: NetworkModel {
                profile: NetworkProfile::Cellular,
                latency_millis: 80,
                jitter_millis: 20,
                loss_basis_points: 50,
                bandwidth_bytes_per_second: 1_000_000,
            },
            conflicts: ConflictModel {
                conflicting_operations_basis_points: 100,
                hot_entity_basis_points: 500,
            },
            dependencies: DependencyModel {
                dependent_operations_basis_points: 500,
                maximum_depth: 8,
                maximum_edges_per_operation: 4,
            },
            cache_state: CacheState::Mixed,
            dataset_reset: DatasetReset::Preloaded,
            synthetic_or_sanitized: true,
        })
    }

    #[test]
    fn validated_workloads_have_stable_fingerprints() -> Result<(), Box<dyn std::error::Error>> {
        let first = workload()?;
        let second = workload()?;
        assert_eq!(first.fingerprint()?, second.fingerprint()?);
        Ok(())
    }

    #[test]
    fn unsafe_or_unbounded_workloads_fail_closed() -> Result<(), Box<dyn std::error::Error>> {
        let mut spec = workload()?;
        spec.synthetic_or_sanitized = false;
        assert_eq!(spec.validate(), Err(WorkloadError::SensitiveDataset));
        spec.synthetic_or_sanitized = true;
        spec.maximum_queue_depth = 0;
        assert_eq!(spec.validate(), Err(WorkloadError::InvalidBounds));
        Ok(())
    }
}
