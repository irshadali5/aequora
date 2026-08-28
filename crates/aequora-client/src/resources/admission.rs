use aequora_scheduler::SchedulingDecision;

use super::{
    BackgroundBudget, ClientResourceContext, ClientResourcePolicy, DataBudgetUsage, MemoryClass,
    NetworkAdmission, NetworkWorkClass, StorageState, ThermalState,
};

/// Resource-aware scheduler work classes from Part 20.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ClientWorkKind {
    CriticalSync,
    InteractivePush,
    InteractivePull,
    SmallRepair,
    Bootstrap,
    BlobTransfer,
    AntiEntropy,
    Maintenance,
    SecurityDirective,
    LocalMutation,
    DatabaseMaintenance,
}

impl ClientWorkKind {
    const fn network_class(self) -> Option<NetworkWorkClass> {
        match self {
            Self::CriticalSync | Self::SecurityDirective | Self::SmallRepair => {
                Some(NetworkWorkClass::Critical)
            }
            Self::InteractivePush | Self::InteractivePull => Some(NetworkWorkClass::Interactive),
            Self::Bootstrap | Self::BlobTransfer => Some(NetworkWorkClass::Bulk),
            Self::AntiEntropy | Self::Maintenance => Some(NetworkWorkClass::Optional),
            Self::LocalMutation | Self::DatabaseMaintenance => None,
        }
    }

    const fn is_bulk(self) -> bool {
        matches!(self, Self::Bootstrap | Self::BlobTransfer)
    }

    const fn is_optional(self) -> bool {
        matches!(
            self,
            Self::AntiEntropy | Self::Maintenance | Self::DatabaseMaintenance
        )
    }

    const fn is_security(self) -> bool {
        matches!(self, Self::SecurityDirective)
    }
}

/// Coarse CPU estimate; it is not uploaded as device telemetry.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum CpuClass {
    #[default]
    Light,
    Moderate,
    Heavy,
}

/// Rough bounded estimate for one durable unit of work.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct WorkEstimate {
    pub operations: u32,
    pub bytes: u64,
    pub storage_delta_bytes: u64,
    pub cpu: CpuClass,
}

/// Resource metadata only; authoritative work payload remains in its durable owner.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ClientWork {
    pub kind: ClientWorkKind,
    pub estimate: WorkEstimate,
    pub user_initiated: bool,
}

/// Compression choice constrained by both bandwidth and CPU/thermal state.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CompressionPreference {
    Disabled,
    Balanced,
    PreferBandwidth,
}

/// Hard execution ceiling applied after scheduler desire and server limits.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AdmissionLimits {
    pub max_operations: u32,
    pub max_bytes: u32,
    pub max_snapshot_chunk_bytes: u32,
    pub max_parallel_transfers: u8,
    pub max_cpu_workers: u8,
    pub compression: CompressionPreference,
}

impl AdmissionLimits {
    /// Applies `min(scheduler, client resource ceiling)` without changing eligibility or priority.
    #[must_use]
    pub fn constrain_scheduler(self, mut decision: SchedulingDecision) -> SchedulingDecision {
        decision.max_batch_ops = decision
            .max_batch_ops
            .min(self.max_operations as usize)
            .max(1);
        decision.max_batch_bytes = decision.max_batch_bytes.min(self.max_bytes as usize).max(1);
        decision.concurrency = decision
            .concurrency
            .min(usize::from(self.max_parallel_transfers))
            .max(1);
        decision
    }
}

/// Stable reason for a local admission outcome.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ResourceReason {
    Offline,
    WaitingForUnmetered,
    MeteredBudgetExhausted,
    LowPower,
    ThermalPressure,
    LowStorage,
    CriticalStorage,
    BackgroundSuspended,
    BackgroundBudget,
}

/// Client-side resource decision. Deferral never deletes its durable source.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ResourceDecision {
    RunNow(AdmissionLimits),
    RunReduced(AdmissionLimits),
    Defer(ResourceReason),
    RequireUserApproval(ResourceReason),
    RejectUntilResourceAvailable(ResourceReason),
}

/// Platform-neutral admission boundary used by the Part 06 scheduler integration.
pub trait ClientResourceAdmission: Send + Sync {
    fn allow(&self, work: &ClientWork, resources: &ClientResourceContext) -> ResourceDecision;
}

/// Built-in conservative policy. Applications may replace this trait implementation.
#[derive(Clone, Copy, Debug)]
pub struct DefaultResourceAdmission {
    policy: ClientResourcePolicy,
    metered_usage: DataBudgetUsage,
}

impl DefaultResourceAdmission {
    /// Creates admission only from a validated resource policy.
    ///
    /// # Errors
    ///
    /// Returns a policy error when any hard limit is invalid.
    pub fn new(policy: ClientResourcePolicy) -> Result<Self, super::PolicyError> {
        policy.validate()?;
        Ok(Self {
            policy,
            metered_usage: DataBudgetUsage::default(),
        })
    }

    /// Supplies privacy-local metered byte counters.
    #[must_use]
    pub const fn with_metered_usage(mut self, usage: DataBudgetUsage) -> Self {
        self.metered_usage = usage;
        self
    }

    fn limits(
        &self,
        work: &ClientWork,
        resources: ClientResourceContext,
    ) -> (AdmissionLimits, bool) {
        let memory = self.policy.memory;
        let mut operations = memory.max_batch_operations;
        let mut bytes = memory.sync_decode_bytes;
        let mut snapshot = memory.snapshot_chunk_bytes;
        let mut transfers = memory.max_parallel_transfers;
        let workers = self
            .policy
            .thermal
            .workers(memory.max_cpu_workers, resources.thermal);
        let mut reduced = false;

        if resources.memory == MemoryClass::VeryLow {
            operations = operations.div_ceil(4).max(1);
            bytes = bytes.div_ceil(4).max(1);
            snapshot = snapshot.div_ceil(2).max(1);
            transfers = 1;
            reduced = true;
        } else if resources.memory == MemoryClass::Low {
            operations = operations.div_ceil(2).max(1);
            bytes = bytes.div_ceil(2).max(1);
            transfers = 1;
            reduced = true;
        }
        if resources.network.high_loss || resources.storage == StorageState::Low {
            operations = operations.div_ceil(2).max(1);
            bytes = bytes.div_ceil(2).max(1);
            transfers = 1;
            reduced = true;
        }
        if matches!(resources.background, BackgroundBudget::Short) {
            operations = operations.clamp(1, 20);
            bytes = bytes.clamp(1, 256 * 1_024);
            transfers = 1;
            reduced = true;
        }
        if resources.thermal != ThermalState::Normal || resources.power.constrained() {
            transfers = 1;
            reduced = true;
        }

        let compression = if matches!(
            resources.thermal,
            ThermalState::Hot | ThermalState::Critical
        ) || work.estimate.bytes < 8 * 1_024
        {
            CompressionPreference::Disabled
        } else if resources.network.metered == Some(true) {
            CompressionPreference::PreferBandwidth
        } else {
            CompressionPreference::Balanced
        };
        (
            AdmissionLimits {
                max_operations: operations,
                max_bytes: bytes,
                max_snapshot_chunk_bytes: snapshot,
                max_parallel_transfers: transfers,
                max_cpu_workers: workers,
                compression,
            },
            reduced,
        )
    }
}

impl ClientResourceAdmission for DefaultResourceAdmission {
    fn allow(&self, work: &ClientWork, resources: &ClientResourceContext) -> ResourceDecision {
        let resources = resources.normalized();
        if resources.background == BackgroundBudget::Suspended {
            return ResourceDecision::Defer(ResourceReason::BackgroundSuspended);
        }
        if matches!(
            resources.storage,
            StorageState::Critical | StorageState::ReadOnlyRisk
        ) {
            return if matches!(work.kind, ClientWorkKind::LocalMutation)
                || work.estimate.storage_delta_bytes > 0
            {
                ResourceDecision::RejectUntilResourceAvailable(ResourceReason::CriticalStorage)
            } else {
                ResourceDecision::Defer(ResourceReason::CriticalStorage)
            };
        }
        if resources.storage == StorageState::Low && work.kind.is_bulk() {
            return ResourceDecision::Defer(ResourceReason::LowStorage);
        }
        if let Some(class) = work.kind.network_class() {
            match self
                .policy
                .network
                .admit(resources.network, class, work.user_initiated)
            {
                NetworkAdmission::Allow => {}
                NetworkAdmission::DeferOffline => {
                    return ResourceDecision::Defer(ResourceReason::Offline);
                }
                NetworkAdmission::DeferForUnmetered => {
                    return ResourceDecision::Defer(ResourceReason::WaitingForUnmetered);
                }
                NetworkAdmission::RequireUserApproval => {
                    return ResourceDecision::RequireUserApproval(
                        ResourceReason::WaitingForUnmetered,
                    );
                }
            }
            if resources.network.metered == Some(true)
                && class != NetworkWorkClass::Critical
                && !self
                    .policy
                    .data_budget
                    .permits(self.metered_usage, work.estimate.bytes)
            {
                return if work.user_initiated {
                    ResourceDecision::RequireUserApproval(ResourceReason::MeteredBudgetExhausted)
                } else {
                    ResourceDecision::Defer(ResourceReason::MeteredBudgetExhausted)
                };
            }
        }
        if work.kind.is_optional()
            && (self.policy.thermal.defer_maintenance(resources.thermal)
                || resources.thermal == ThermalState::Critical)
        {
            return ResourceDecision::Defer(ResourceReason::ThermalPressure);
        }
        if work.kind.is_optional()
            && !self.policy.power.maintenance_allowed(resources.power)
            && !work.kind.is_security()
        {
            return ResourceDecision::Defer(ResourceReason::LowPower);
        }
        if work.kind.is_bulk() && self.policy.power.defer_bulk(resources.power) {
            return ResourceDecision::Defer(ResourceReason::LowPower);
        }
        if resources.background == BackgroundBudget::Limited
            && (work.kind.is_optional() || work.kind.is_bulk())
        {
            return ResourceDecision::Defer(ResourceReason::BackgroundBudget);
        }
        let (limits, reduced) = self.limits(work, resources);
        if reduced {
            ResourceDecision::RunReduced(limits)
        } else {
            ResourceDecision::RunNow(limits)
        }
    }
}
