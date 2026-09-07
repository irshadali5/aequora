//! Bounded load planning and correctness observation with no I/O or runtime dependency.

use aequora_benchkit::{CorrectnessResult, LoadCounters};
use aequora_workload::{ArrivalModel, DatasetGenerator, LoadModel, WorkloadSpec};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, VecDeque};
use thiserror::Error;

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct VirtualClientId(pub u64);

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct TenantKey(pub u32);

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct SyntheticOperationId(pub u128);

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct VirtualClientState {
    pub id: VirtualClientId,
    pub tenant: TenantKey,
    pub cursor: u64,
    pub pending_outbox: u32,
    pub batch_size: u16,
    pub retry_attempt: u16,
    pub offline_until_millis: Option<u64>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PlannedOperation {
    pub id: SyntheticOperationId,
    pub client: VirtualClientId,
    pub tenant: TenantKey,
    pub operation_kind: String,
    pub payload_bytes: u32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AdmissionOutcome {
    Admitted,
    RejectedAtCapacity,
}

/// Bounded stateful load planner. Runtime adapters decide when and how to execute planned work.
pub struct LoadGenerator {
    spec: WorkloadSpec,
    generator: DatasetGenerator,
    clients: Vec<VirtualClientState>,
    pending: VecDeque<PlannedOperation>,
    in_flight: u32,
    counters: LoadCounters,
    next_operation: u128,
}

impl LoadGenerator {
    /// Builds realistic virtual-client state from a validated workload.
    ///
    /// # Errors
    ///
    /// Rejects invalid workloads or clients too large for this process representation.
    pub fn new(spec: WorkloadSpec) -> Result<Self, LoadgenError> {
        spec.validate().map_err(|_| LoadgenError::InvalidWorkload)?;
        let client_count =
            usize::try_from(spec.clients).map_err(|_| LoadgenError::InvalidWorkload)?;
        let mut generator =
            DatasetGenerator::new(spec.dataset_seed).map_err(|_| LoadgenError::InvalidWorkload)?;
        let mut clients = Vec::new();
        clients
            .try_reserve_exact(client_count)
            .map_err(|_| LoadgenError::ClientCapacity)?;
        for index in 0..spec.clients {
            let offline = generator.choose_basis_points() < spec.offline_clients_basis_points;
            clients.push(VirtualClientState {
                id: VirtualClientId(u64::from(index) + 1),
                tenant: TenantKey(index % spec.tenants),
                cursor: 0,
                pending_outbox: 0,
                batch_size: 1,
                retry_attempt: 0,
                offline_until_millis: offline
                    .then_some(u64::from(spec.reconnect_window_seconds) * 1_000),
            });
        }
        Ok(Self {
            spec,
            generator,
            clients,
            pending: VecDeque::new(),
            in_flight: 0,
            counters: LoadCounters::default(),
            next_operation: 1,
        })
    }

    /// Offers one operation while enforcing queue and in-flight budgets.
    pub fn offer(&mut self) -> AdmissionOutcome {
        self.counters.offered = self.counters.offered.saturating_add(1);
        if self.pending.len() >= self.spec.maximum_queue_depth as usize
            || self.in_flight >= self.spec.maximum_in_flight
        {
            self.counters.rejected = self.counters.rejected.saturating_add(1);
            return AdmissionOutcome::RejectedAtCapacity;
        }
        let client_index = usize::try_from(
            self.generator.next_u64() % u64::try_from(self.clients.len()).unwrap_or(u64::MAX),
        )
        .unwrap_or_default();
        let client = &mut self.clients[client_index];
        let operation_kind = choose_operation_kind(
            &self.spec.operation_mix_basis_points,
            self.generator.choose_basis_points(),
        );
        let payload_bytes = choose_payload(
            self.spec.payload_bytes,
            self.generator.choose_basis_points(),
        );
        self.pending.push_back(PlannedOperation {
            id: SyntheticOperationId(self.next_operation),
            client: client.id,
            tenant: client.tenant,
            operation_kind,
            payload_bytes,
        });
        self.next_operation = self.next_operation.saturating_add(1);
        client.pending_outbox = client.pending_outbox.saturating_add(1);
        self.counters.admitted = self.counters.admitted.saturating_add(1);
        AdmissionOutcome::Admitted
    }

    /// Claims the next bounded operation for a runtime executor.
    #[must_use]
    pub fn claim(&mut self) -> Option<PlannedOperation> {
        let operation = self.pending.pop_front()?;
        self.in_flight = self.in_flight.saturating_add(1);
        self.counters.accepted = self.counters.accepted.saturating_add(1);
        Some(operation)
    }

    /// Records completion and authoritative goodput without conflating offered HTTP work.
    ///
    /// # Errors
    ///
    /// Rejects completion when nothing is in flight.
    pub fn complete(&mut self, authoritative: bool, useful: bool) -> Result<(), LoadgenError> {
        if self.in_flight == 0 || (useful && !authoritative) {
            return Err(LoadgenError::InvalidCompletion);
        }
        self.in_flight -= 1;
        self.counters.completed = self.counters.completed.saturating_add(1);
        if authoritative {
            self.counters.authoritative = self.counters.authoritative.saturating_add(1);
        }
        if useful {
            self.counters.goodput = self.counters.goodput.saturating_add(1);
        }
        Ok(())
    }

    #[must_use]
    pub const fn counters(&self) -> LoadCounters {
        self.counters
    }

    #[must_use]
    pub fn queue_depth(&self) -> usize {
        self.pending.len()
    }

    #[must_use]
    pub fn clients(&self) -> &[VirtualClientState] {
        &self.clients
    }

    /// Indicates whether independent arrivals continue under latency rather than self-throttling.
    #[must_use]
    pub const fn preserves_offered_load(&self) -> bool {
        matches!(self.spec.load_model, LoadModel::Open)
    }

    /// Returns whether this scenario intentionally models a burst family.
    #[must_use]
    pub const fn is_burst(&self) -> bool {
        matches!(
            self.spec.arrival,
            ArrivalModel::Burst | ArrivalModel::ReconnectStorm | ArrivalModel::Step
        )
    }
}

fn choose_operation_kind(mix: &BTreeMap<String, u16>, choice: u16) -> String {
    let mut upper = 0_u32;
    for (kind, weight) in mix {
        upper = upper.saturating_add(u32::from(*weight));
        if u32::from(choice) < upper {
            return kind.clone();
        }
    }
    mix.last_key_value()
        .map_or_else(String::new, |(kind, _)| kind.clone())
}

fn choose_payload(distribution: aequora_workload::PercentileBytes, choice: u16) -> u32 {
    match choice {
        0..5_000 => distribution.p50,
        5_000..9_500 => distribution.p95,
        _ => distribution.p99,
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AuthoritativeObservation {
    pub operation: SyntheticOperationId,
    pub tenant: TenantKey,
    pub entity_version: u64,
    pub journal_sequence: u64,
}

/// Incremental, memory-bounded correctness checker for load-test observations.
pub struct CorrectnessOracle {
    maximum_operations: usize,
    accepted: BTreeMap<SyntheticOperationId, TenantKey>,
    authoritative: BTreeMap<SyntheticOperationId, AuthoritativeObservation>,
    last_sequence: BTreeMap<TenantKey, u64>,
    last_version: BTreeMap<TenantKey, u64>,
    result: CorrectnessResult,
}

impl CorrectnessOracle {
    /// Creates a finite oracle budget.
    ///
    /// # Errors
    ///
    /// Rejects zero capacity.
    pub fn new(maximum_operations: usize) -> Result<Self, LoadgenError> {
        if maximum_operations == 0 {
            return Err(LoadgenError::OracleCapacity);
        }
        Ok(Self {
            maximum_operations,
            accepted: BTreeMap::new(),
            authoritative: BTreeMap::new(),
            last_sequence: BTreeMap::new(),
            last_version: BTreeMap::new(),
            result: CorrectnessResult {
                duplicate_authoritative_effects: 0,
                cursor_skips: 0,
                lost_accepted_operations: 0,
                cross_tenant_leaks: 0,
                broken_versions: 0,
                invalid_journal_orderings: 0,
            },
        })
    }

    /// Records an accepted operation and its immutable tenant binding.
    ///
    /// # Errors
    ///
    /// Fails closed at the configured evidence capacity.
    pub fn accepted(
        &mut self,
        operation: SyntheticOperationId,
        tenant: TenantKey,
    ) -> Result<(), LoadgenError> {
        if self.accepted.len() >= self.maximum_operations && !self.accepted.contains_key(&operation)
        {
            return Err(LoadgenError::OracleCapacity);
        }
        if self
            .accepted
            .insert(operation, tenant)
            .is_some_and(|previous| previous != tenant)
        {
            self.result.cross_tenant_leaks = self.result.cross_tenant_leaks.saturating_add(1);
        }
        Ok(())
    }

    /// Records authoritative outcome ordering and version evidence.
    ///
    /// # Errors
    ///
    /// Fails closed at the configured evidence capacity.
    pub fn authoritative(
        &mut self,
        observation: AuthoritativeObservation,
    ) -> Result<(), LoadgenError> {
        if self.authoritative.len() >= self.maximum_operations
            && !self.authoritative.contains_key(&observation.operation)
        {
            return Err(LoadgenError::OracleCapacity);
        }
        if let Some(previous) = self.authoritative.get(&observation.operation) {
            self.result.duplicate_authoritative_effects = self
                .result
                .duplicate_authoritative_effects
                .saturating_add(1);
            if previous.tenant != observation.tenant {
                self.result.cross_tenant_leaks = self.result.cross_tenant_leaks.saturating_add(1);
            }
            return Ok(());
        }
        if self
            .accepted
            .get(&observation.operation)
            .is_some_and(|tenant| *tenant != observation.tenant)
        {
            self.result.cross_tenant_leaks = self.result.cross_tenant_leaks.saturating_add(1);
        }
        if self
            .last_sequence
            .get(&observation.tenant)
            .is_some_and(|last| observation.journal_sequence != last.saturating_add(1))
        {
            self.result.cursor_skips = self.result.cursor_skips.saturating_add(1);
            self.result.invalid_journal_orderings =
                self.result.invalid_journal_orderings.saturating_add(1);
        }
        if self
            .last_version
            .get(&observation.tenant)
            .is_some_and(|last| observation.entity_version < *last)
        {
            self.result.broken_versions = self.result.broken_versions.saturating_add(1);
        }
        self.last_sequence
            .insert(observation.tenant, observation.journal_sequence);
        self.last_version
            .insert(observation.tenant, observation.entity_version);
        self.authoritative
            .insert(observation.operation, observation);
        Ok(())
    }

    #[must_use]
    pub fn finish(mut self) -> CorrectnessResult {
        self.result.lost_accepted_operations = u64::try_from(
            self.accepted
                .keys()
                .filter(|operation| !self.authoritative.contains_key(operation))
                .count(),
        )
        .unwrap_or(u64::MAX);
        self.result
    }
}

#[derive(Clone, Copy, Debug, Error, Eq, PartialEq)]
pub enum LoadgenError {
    #[error("workload is invalid")]
    InvalidWorkload,
    #[error("virtual-client allocation exceeds process capacity")]
    ClientCapacity,
    #[error("completion accounting is inconsistent")]
    InvalidCompletion,
    #[error("correctness oracle reached its configured evidence capacity")]
    OracleCapacity,
}

#[cfg(test)]
mod tests {
    use super::*;
    use aequora_workload::{
        CacheState, ConflictModel, DatasetReset, DependencyModel, DomainProfile, NetworkModel,
        NetworkProfile, PercentileBytes, ScenarioId,
    };

    fn workload() -> Result<WorkloadSpec, Box<dyn std::error::Error>> {
        Ok(WorkloadSpec {
            format_version: 1,
            scenario: ScenarioId::new("generic_small/v1")?,
            domain: DomainProfile::GenericCrud,
            dataset_seed: 47,
            tenants: 2,
            clients: 8,
            active_clients: 4,
            duration_seconds: 10,
            arrival: ArrivalModel::Constant,
            load_model: LoadModel::Open,
            offered_operations_per_second: 10,
            maximum_in_flight: 2,
            maximum_queue_depth: 3,
            operation_mix_basis_points: BTreeMap::from([("entity.update".into(), 10_000)]),
            payload_bytes: PercentileBytes {
                p50: 100,
                p95: 500,
                p99: 1_000,
            },
            offline_clients_basis_points: 0,
            reconnect_window_seconds: 0,
            network: NetworkModel {
                profile: NetworkProfile::Lan,
                latency_millis: 1,
                jitter_millis: 0,
                loss_basis_points: 0,
                bandwidth_bytes_per_second: 1_000_000,
            },
            conflicts: ConflictModel {
                conflicting_operations_basis_points: 100,
                hot_entity_basis_points: 100,
            },
            dependencies: DependencyModel {
                dependent_operations_basis_points: 100,
                maximum_depth: 2,
                maximum_edges_per_operation: 2,
            },
            cache_state: CacheState::Warm,
            dataset_reset: DatasetReset::Fresh,
            synthetic_or_sanitized: true,
        })
    }

    #[test]
    fn offered_load_is_bounded_and_rejections_are_visible() -> Result<(), Box<dyn std::error::Error>>
    {
        let mut generator = LoadGenerator::new(workload()?)?;
        assert_eq!(generator.offer(), AdmissionOutcome::Admitted);
        assert_eq!(generator.offer(), AdmissionOutcome::Admitted);
        assert_eq!(generator.offer(), AdmissionOutcome::Admitted);
        assert_eq!(generator.offer(), AdmissionOutcome::RejectedAtCapacity);
        assert_eq!(generator.queue_depth(), 3);
        assert_eq!(generator.counters().offered, 4);
        assert_eq!(generator.counters().rejected, 1);
        Ok(())
    }

    #[test]
    fn oracle_detects_duplicates_gaps_versions_and_losses() -> Result<(), Box<dyn std::error::Error>>
    {
        let mut oracle = CorrectnessOracle::new(4)?;
        oracle.accepted(SyntheticOperationId(1), TenantKey(1))?;
        oracle.accepted(SyntheticOperationId(2), TenantKey(1))?;
        oracle.accepted(SyntheticOperationId(3), TenantKey(1))?;
        oracle.accepted(SyntheticOperationId(4), TenantKey(1))?;
        let first = AuthoritativeObservation {
            operation: SyntheticOperationId(1),
            tenant: TenantKey(1),
            entity_version: 2,
            journal_sequence: 1,
        };
        oracle.authoritative(first)?;
        oracle.authoritative(first)?;
        oracle.authoritative(AuthoritativeObservation {
            operation: SyntheticOperationId(3),
            tenant: TenantKey(2),
            entity_version: 1,
            journal_sequence: 1,
        })?;
        oracle.authoritative(AuthoritativeObservation {
            operation: SyntheticOperationId(4),
            tenant: TenantKey(1),
            entity_version: 1,
            journal_sequence: 3,
        })?;
        let result = oracle.finish();
        assert_eq!(result.duplicate_authoritative_effects, 1);
        assert_eq!(result.invalid_journal_orderings, 1);
        assert_eq!(result.cursor_skips, 1);
        assert_eq!(result.broken_versions, 1);
        assert_eq!(result.cross_tenant_leaks, 1);
        assert_eq!(result.lost_accepted_operations, 1);
        Ok(())
    }
}
