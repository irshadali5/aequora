//! Deterministic, platform-neutral scheduling and `QoS` policy.
//!
//! This crate is deliberately a planner. Durable intent remains in the outbox, bootstrap,
//! integrity, blob, and maintenance stores. Scheduling can defer or bound eligible work, but it
//! cannot mutate operation identity, payload, dependencies, authorization, or authoritative order.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Stable identity of one logical scheduler work item.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct WorkId(pub u64);

/// Scheduling class, independent of business authorization or importance.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
pub enum WorkClass {
    /// Explicitly certified urgent operational work.
    Critical,
    /// User-visible work for which a person is waiting.
    Interactive,
    /// Routine synchronization.
    Normal,
    /// Large imports or catch-up.
    Bulk,
    /// Deferrable verification and metadata work.
    Background,
    /// Deep optimization or exclusive operational work.
    Maintenance,
}

impl WorkClass {
    /// Default priority rank before aging and dependency inheritance.
    #[must_use]
    pub const fn rank(self) -> u8 {
        match self {
            Self::Critical => 5,
            Self::Interactive => 4,
            Self::Normal => 3,
            Self::Bulk => 2,
            Self::Background => 1,
            Self::Maintenance => 0,
        }
    }

    const fn from_rank(rank: u8) -> Self {
        match if rank > 5 { 5 } else { rank } {
            5 => Self::Critical,
            4 => Self::Interactive,
            3 => Self::Normal,
            2 => Self::Bulk,
            1 => Self::Background,
            _ => Self::Maintenance,
        }
    }
}

/// Bounded application metadata for one operation kind.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct OperationPriorityMetadata {
    /// Trusted application default.
    pub default: WorkClass,
    /// Highest untrusted caller hint the application will accept.
    pub maximum_client_hint: WorkClass,
}

impl OperationPriorityMetadata {
    /// Caps a caller hint without changing operation semantics.
    #[must_use]
    pub fn classify(self, hint: Option<WorkClass>) -> WorkClass {
        hint.map_or(self.default, |candidate| {
            WorkClass::from_rank(candidate.rank().min(self.maximum_client_hint.rank()))
        })
    }
}

/// Application-owned mapping from operation kind identifiers to trusted priority metadata.
#[derive(Clone, Debug, Default)]
pub struct OperationPriorityRegistry<K> {
    entries: BTreeMap<K, OperationPriorityMetadata>,
}

impl<K: Ord> OperationPriorityRegistry<K> {
    /// Registers or deliberately replaces one application's operation-kind policy.
    pub fn register(&mut self, kind: K, metadata: OperationPriorityMetadata) {
        self.entries.insert(kind, metadata);
    }

    /// Classifies a caller hint. Unknown kinds fail closed to routine normal priority.
    #[must_use]
    pub fn classify(&self, kind: &K, hint: Option<WorkClass>) -> WorkClass {
        self.entries
            .get(kind)
            .map_or(WorkClass::Normal, |metadata| metadata.classify(hint))
    }
}

/// Durable source/kind of scheduler work.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum WorkKind {
    PushOperations,
    PullChanges,
    Bootstrap,
    IntegrityCheck,
    Repair,
    QueueCompaction,
    BlobTransfer,
    Maintenance,
    ScopeTransition,
    /// Normal durable catch-up accelerated by an ephemeral live wake.
    LiveCatchUp,
    /// Bounded preparation or target batch for a durable bulk import/export job.
    BulkMigration,
    /// Verified single-writer authority cutover transaction.
    MigrationCutover,
    /// Snapshot construction or bounded chunk transfer work.
    LargeBootstrapTransfer,
    /// Verified atomic local generation activation.
    BootstrapActivation,
}

impl WorkKind {
    /// Conservative built-in class used unless an application registers a narrower policy.
    #[must_use]
    pub const fn default_class(self) -> WorkClass {
        match self {
            Self::PushOperations | Self::PullChanges => WorkClass::Normal,
            Self::Bootstrap | Self::LiveCatchUp => WorkClass::Interactive,
            Self::IntegrityCheck => WorkClass::Background,
            Self::Repair
            | Self::ScopeTransition
            | Self::MigrationCutover
            | Self::BootstrapActivation => WorkClass::Critical,
            Self::QueueCompaction | Self::Maintenance => WorkClass::Maintenance,
            Self::BlobTransfer | Self::BulkMigration | Self::LargeBootstrapTransfer => {
                WorkClass::Bulk
            }
        }
    }
}

/// Coarse bandwidth input; precise carrier accounting is outside the core.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub enum BandwidthEstimate {
    Slow,
    #[default]
    Normal,
    Fast,
}

/// Normalized network hints supplied by a platform integration.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct NetworkContext {
    pub online: bool,
    pub metered: Option<bool>,
    pub roaming: Option<bool>,
    pub bandwidth: Option<BandwidthEstimate>,
    pub estimated_rtt_ms: Option<u64>,
}

impl Default for NetworkContext {
    fn default() -> Self {
        Self {
            online: true,
            metered: None,
            roaming: None,
            bandwidth: None,
            estimated_rtt_ms: None,
        }
    }
}

/// Normalized power hints; absent values retain conservative defaults.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct PowerContext {
    pub charging: Option<bool>,
    pub battery_level_percent: Option<u8>,
    pub low_power_mode: Option<bool>,
}

/// Local application activity. This value need not be uploaded.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub enum AppActivity {
    ForegroundInteractive,
    #[default]
    ForegroundIdle,
    Background,
    SuspendedImminent,
}

/// Coarse local pressure input used only to reduce bounded work.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct ResourcePressure {
    pub memory_high: bool,
    pub disk_low: bool,
}

/// Optional bounded server cooperation hints.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct ServerSchedulingHints {
    pub retry_after_ms: Option<u64>,
    pub preferred_max_batch_ops: Option<usize>,
    pub preferred_max_batch_bytes: Option<usize>,
    pub background_allowed: Option<bool>,
}

/// One logical durable work descriptor. Payloads deliberately do not appear here.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct WorkDescriptor {
    pub work_id: WorkId,
    pub kind: WorkKind,
    pub class: WorkClass,
    pub enqueued_at_unix_ms: u64,
    pub earliest_at_unix_ms: u64,
    pub deadline_unix_ms: Option<u64>,
    pub dependencies: Vec<WorkId>,
    pub estimated_operations: usize,
    pub estimated_bytes: usize,
    /// Explicit application-certified override for constrained connectivity.
    pub emergency_override: bool,
}

/// Complete local scheduler input for one evaluation.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[allow(clippy::struct_excessive_bools)]
pub struct SchedulingContext {
    pub now_unix_ms: u64,
    pub network: NetworkContext,
    pub power: PowerContext,
    pub activity: AppActivity,
    pub pressure: ResourcePressure,
    pub server: ServerSchedulingHints,
    pub paused: bool,
    pub auth_available: bool,
    pub storage_healthy: bool,
    pub coordinator_leader: bool,
    pub remaining_execution_ms: Option<u64>,
}

impl Default for SchedulingContext {
    fn default() -> Self {
        Self {
            now_unix_ms: 0,
            network: NetworkContext::default(),
            power: PowerContext::default(),
            activity: AppActivity::default(),
            pressure: ResourcePressure::default(),
            server: ServerSchedulingHints::default(),
            paused: false,
            auth_available: true,
            storage_healthy: true,
            coordinator_leader: true,
            remaining_execution_ms: None,
        }
    }
}

impl SchedulingContext {
    /// Clamps untrusted platform hints while retaining absence as an explicit unknown value.
    #[must_use]
    pub fn normalized(mut self) -> Self {
        self.power.battery_level_percent =
            self.power.battery_level_percent.map(|level| level.min(100));
        self.server.preferred_max_batch_ops = self
            .server
            .preferred_max_batch_ops
            .map(|operations| operations.max(1));
        self.server.preferred_max_batch_bytes = self
            .server
            .preferred_max_batch_bytes
            .map(|bytes| bytes.max(1));
        self
    }
}

/// Stable reason why durable work remains deferred.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum DeferralReason {
    Paused,
    Offline,
    NotLeader,
    AuthenticationRequired,
    StorageUnhealthy,
    RetryBackoff,
    DependencyPending,
    MeteredPolicy,
    RoamingPolicy,
    LowPowerPolicy,
    ServerBackgroundPause,
    ExecutionBudget,
}

/// Scheduler-selected compression behavior.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum CompressionDecision {
    Disabled,
    Balanced,
    PreferBandwidth,
    PreferCpu,
}

/// Effective bounded priority after aging and dependency inheritance.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct EffectivePriority(pub u8);

/// Pure policy output for one work item.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct SchedulingDecision {
    pub eligible: bool,
    pub deferral: Option<DeferralReason>,
    pub priority: EffectivePriority,
    pub max_batch_ops: usize,
    pub max_batch_bytes: usize,
    pub concurrency: usize,
    pub compression: CompressionDecision,
}

/// Validated adaptive batch bounds.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct BatchBounds {
    pub min_ops: usize,
    pub target_ops: usize,
    pub max_ops: usize,
    pub min_bytes: usize,
    pub target_bytes: usize,
    pub max_bytes: usize,
    pub additive_ops: usize,
    pub additive_bytes: usize,
}

impl Default for BatchBounds {
    fn default() -> Self {
        SchedulerPolicy::default().batch
    }
}

impl BatchBounds {
    /// Validates non-zero monotonic limits.
    ///
    /// # Errors
    ///
    /// Returns [`SchedulerError::InvalidBatchBounds`] for zero or non-monotonic bounds.
    pub const fn validate(self) -> Result<(), SchedulerError> {
        if self.min_ops == 0
            || self.min_ops > self.target_ops
            || self.target_ops > self.max_ops
            || self.min_bytes == 0
            || self.min_bytes > self.target_bytes
            || self.target_bytes > self.max_bytes
            || self.additive_ops == 0
            || self.additive_bytes == 0
        {
            return Err(SchedulerError::InvalidBatchBounds);
        }
        Ok(())
    }
}

/// Named safe-default scheduler profiles.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum SyncProfile {
    Desktop,
    Mobile,
    HighLatency,
    LowBandwidth,
    EnterpriseLan,
}

/// Deterministic scheduler policy and hard safety bounds.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(default, deny_unknown_fields)]
#[allow(clippy::struct_excessive_bools)]
pub struct SchedulerPolicy {
    pub version: u32,
    pub profile: SyncProfile,
    pub batch: BatchBounds,
    pub concurrency: usize,
    pub interactive_debounce_ms: u64,
    pub aging_quantum_ms: u64,
    pub maximum_age_boost: u8,
    pub defer_bulk_on_metered: bool,
    pub defer_background_on_metered: bool,
    pub defer_bulk_on_roaming: bool,
    pub defer_background_on_roaming: bool,
    pub defer_background_on_low_power: bool,
    pub maximum_retry_deferral_ms: u64,
}

impl SchedulerPolicy {
    /// Returns conservative deterministic defaults for one deployment profile.
    #[must_use]
    pub const fn for_profile(profile: SyncProfile) -> Self {
        match profile {
            SyncProfile::Desktop => policy(profile, 64, 256, 1_048_576, 2, 100, false),
            SyncProfile::Mobile => policy(profile, 32, 128, 524_288, 1, 150, true),
            SyncProfile::HighLatency => policy(profile, 128, 512, 2_097_152, 1, 200, false),
            SyncProfile::LowBandwidth => policy(profile, 16, 64, 262_144, 1, 200, true),
            SyncProfile::EnterpriseLan => policy(profile, 64, 256, 1_048_576, 2, 50, false),
        }
    }

    /// Validates every hard bound and timing relationship.
    ///
    /// # Errors
    ///
    /// Returns a typed scheduler error for inconsistent limits or timings.
    pub const fn validate(self) -> Result<(), SchedulerError> {
        if self.version == 0
            || self.concurrency == 0
            || self.aging_quantum_ms == 0
            || self.maximum_age_boost > 5
            || self.maximum_retry_deferral_ms == 0
        {
            return Err(SchedulerError::InvalidPolicy);
        }
        self.batch.validate()
    }
}

const fn policy(
    profile: SyncProfile,
    target_ops: usize,
    max_ops: usize,
    max_bytes: usize,
    concurrency: usize,
    debounce_ms: u64,
    constrained: bool,
) -> SchedulerPolicy {
    SchedulerPolicy {
        version: 1,
        profile,
        batch: BatchBounds {
            min_ops: 1,
            target_ops,
            max_ops,
            min_bytes: 1_024,
            target_bytes: max_bytes / 2,
            max_bytes,
            additive_ops: 8,
            additive_bytes: 32 * 1_024,
        },
        concurrency,
        interactive_debounce_ms: debounce_ms,
        aging_quantum_ms: 60_000,
        maximum_age_boost: 5,
        defer_bulk_on_metered: constrained,
        defer_background_on_metered: constrained,
        defer_bulk_on_roaming: true,
        defer_background_on_roaming: true,
        defer_background_on_low_power: true,
        maximum_retry_deferral_ms: 30 * 60 * 1_000,
    }
}

impl Default for SchedulerPolicy {
    fn default() -> Self {
        Self::for_profile(SyncProfile::Desktop)
    }
}

/// Side-effect-free application policy boundary.
pub trait SchedulingPolicy: Send + Sync {
    fn version(&self) -> u32;
    fn aging_quantum_ms(&self) -> u64;
    fn maximum_age_boost(&self) -> u8;
    fn maximum_retry_deferral_ms(&self) -> u64;
    fn decide(
        &self,
        context: &SchedulingContext,
        work: &WorkDescriptor,
        inherited_priority: EffectivePriority,
        completed: &BTreeSet<WorkId>,
    ) -> SchedulingDecision;
}

impl SchedulingPolicy for SchedulerPolicy {
    fn version(&self) -> u32 {
        self.version
    }

    fn aging_quantum_ms(&self) -> u64 {
        self.aging_quantum_ms
    }

    fn maximum_age_boost(&self) -> u8 {
        self.maximum_age_boost
    }

    fn maximum_retry_deferral_ms(&self) -> u64 {
        self.maximum_retry_deferral_ms
    }

    fn decide(
        &self,
        context: &SchedulingContext,
        work: &WorkDescriptor,
        inherited_priority: EffectivePriority,
        completed: &BTreeSet<WorkId>,
    ) -> SchedulingDecision {
        let deferral = eligibility(self, context, work, completed);
        let mut max_ops = self.batch.max_ops;
        let mut max_bytes = self.batch.max_bytes;
        if context.pressure.memory_high {
            max_ops = max_ops.div_ceil(2).max(self.batch.min_ops);
            max_bytes = max_bytes.div_ceil(2).max(self.batch.min_bytes);
        }
        if context.network.bandwidth == Some(BandwidthEstimate::Slow) {
            max_ops = max_ops.div_ceil(2).max(self.batch.min_ops);
            max_bytes = max_bytes.div_ceil(2).max(self.batch.min_bytes);
        }
        if let Some(server) = context.server.preferred_max_batch_ops {
            max_ops = max_ops.min(server.max(1));
        }
        if let Some(server) = context.server.preferred_max_batch_bytes {
            max_bytes = max_bytes.min(server.max(1));
        }
        let compression = if context.power.low_power_mode == Some(true) {
            CompressionDecision::PreferCpu
        } else if context.network.metered == Some(true)
            || context.network.bandwidth == Some(BandwidthEstimate::Slow)
        {
            CompressionDecision::PreferBandwidth
        } else if self.profile == SyncProfile::EnterpriseLan {
            CompressionDecision::Disabled
        } else {
            CompressionDecision::Balanced
        };
        SchedulingDecision {
            eligible: deferral.is_none(),
            deferral,
            priority: inherited_priority,
            max_batch_ops: max_ops,
            max_batch_bytes: max_bytes,
            concurrency: if context.power.low_power_mode == Some(true) {
                1
            } else {
                self.concurrency
            },
            compression,
        }
    }
}

fn eligibility(
    policy: &SchedulerPolicy,
    context: &SchedulingContext,
    work: &WorkDescriptor,
    completed: &BTreeSet<WorkId>,
) -> Option<DeferralReason> {
    if context.paused {
        return Some(DeferralReason::Paused);
    }
    if !context.network.online {
        return Some(DeferralReason::Offline);
    }
    if !context.coordinator_leader {
        return Some(DeferralReason::NotLeader);
    }
    if !context.auth_available {
        return Some(DeferralReason::AuthenticationRequired);
    }
    if !context.storage_healthy {
        return Some(DeferralReason::StorageUnhealthy);
    }
    if context.now_unix_ms < work.earliest_at_unix_ms {
        return Some(DeferralReason::RetryBackoff);
    }
    if work
        .dependencies
        .iter()
        .any(|dependency| !completed.contains(dependency))
    {
        return Some(DeferralReason::DependencyPending);
    }
    let constrained_override = work.class == WorkClass::Critical && work.emergency_override;
    if !constrained_override && context.network.roaming == Some(true) {
        if policy.defer_bulk_on_roaming && work.class == WorkClass::Bulk {
            return Some(DeferralReason::RoamingPolicy);
        }
        if policy.defer_background_on_roaming
            && matches!(work.class, WorkClass::Background | WorkClass::Maintenance)
        {
            return Some(DeferralReason::RoamingPolicy);
        }
    }
    if !constrained_override && context.network.metered == Some(true) {
        if policy.defer_bulk_on_metered && work.class == WorkClass::Bulk {
            return Some(DeferralReason::MeteredPolicy);
        }
        if policy.defer_background_on_metered
            && matches!(work.class, WorkClass::Background | WorkClass::Maintenance)
        {
            return Some(DeferralReason::MeteredPolicy);
        }
    }
    if !constrained_override
        && context.power.low_power_mode == Some(true)
        && policy.defer_background_on_low_power
        && matches!(work.class, WorkClass::Background | WorkClass::Maintenance)
    {
        return Some(DeferralReason::LowPowerPolicy);
    }
    if context.server.background_allowed == Some(false)
        && matches!(work.class, WorkClass::Background | WorkClass::Maintenance)
    {
        return Some(DeferralReason::ServerBackgroundPause);
    }
    if context.activity == AppActivity::SuspendedImminent
        && work.class != WorkClass::Critical
        && context.remaining_execution_ms.is_some_and(|remaining| {
            work.deadline_unix_ms
                .is_none_or(|deadline| deadline.saturating_sub(context.now_unix_ms) > remaining)
        })
    {
        return Some(DeferralReason::ExecutionBudget);
    }
    None
}

/// Adaptive count/byte controller that never exceeds validated hard limits.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct BatchController {
    bounds: BatchBounds,
    pub target_ops: usize,
    pub target_bytes: usize,
    pub success_streak: u32,
}

impl BatchController {
    /// Creates a bounded controller.
    ///
    /// # Errors
    ///
    /// Returns [`SchedulerError::InvalidBatchBounds`] for inconsistent bounds.
    pub fn new(bounds: BatchBounds) -> Result<Self, SchedulerError> {
        bounds.validate()?;
        Ok(Self {
            target_ops: bounds.target_ops,
            target_bytes: bounds.target_bytes,
            success_streak: 0,
            bounds,
        })
    }

    /// Applies a successful low-latency observation with additive growth.
    pub fn record_success(&mut self) {
        self.success_streak = self.success_streak.saturating_add(1);
        self.target_ops = self
            .target_ops
            .saturating_add(self.bounds.additive_ops)
            .min(self.bounds.max_ops);
        self.target_bytes = self
            .target_bytes
            .saturating_add(self.bounds.additive_bytes)
            .min(self.bounds.max_bytes);
    }

    /// Applies overload, timeout, or pressure with multiplicative decrease.
    pub fn record_overload(&mut self) {
        self.success_streak = 0;
        self.target_ops = self.target_ops.div_ceil(2).max(self.bounds.min_ops);
        self.target_bytes = self.target_bytes.div_ceil(2).max(self.bounds.min_bytes);
    }

    /// Applies bounded server preferences without increasing local hard limits.
    pub fn apply_server_hints(&mut self, hints: ServerSchedulingHints) {
        if let Some(operations) = hints.preferred_max_batch_ops {
            self.target_ops = self.target_ops.min(operations.max(self.bounds.min_ops));
        }
        if let Some(bytes) = hints.preferred_max_batch_bytes {
            self.target_bytes = self.target_bytes.min(bytes.max(self.bounds.min_bytes));
        }
    }
}

/// Circuit-breaker state safe to persist across restarts.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum CircuitState {
    Closed,
    Open { until_unix_ms: u64 },
    HalfOpen,
}

/// Deterministic endpoint circuit breaker.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct CircuitBreaker {
    pub state: CircuitState,
    pub consecutive_failures: u32,
    pub failure_threshold: u32,
    pub cooldown_ms: u64,
}

impl CircuitBreaker {
    #[must_use]
    pub const fn new(failure_threshold: u32, cooldown_ms: u64) -> Self {
        Self {
            state: CircuitState::Closed,
            consecutive_failures: 0,
            failure_threshold,
            cooldown_ms,
        }
    }

    /// Returns whether one request may start, transitioning an expired open breaker to half-open.
    pub fn allow(&mut self, now_unix_ms: u64) -> bool {
        if let CircuitState::Open { until_unix_ms } = self.state {
            if now_unix_ms < until_unix_ms {
                return false;
            }
            self.state = CircuitState::HalfOpen;
        }
        true
    }

    pub fn record_success(&mut self) {
        self.consecutive_failures = 0;
        self.state = CircuitState::Closed;
    }

    pub fn record_failure(&mut self, now_unix_ms: u64) {
        self.consecutive_failures = self.consecutive_failures.saturating_add(1);
        if self.consecutive_failures >= self.failure_threshold.max(1) {
            self.state = CircuitState::Open {
                until_unix_ms: now_unix_ms.saturating_add(self.cooldown_ms),
            };
        }
    }
}

/// Durable scheduler control state; ordinary work itself is not duplicated here.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct SchedulerState {
    pub policy_version: u32,
    pub batch: BatchController,
    pub circuit: CircuitBreaker,
    pub server_backoff_until_unix_ms: Option<u64>,
    pub background_bytes_today: u64,
    pub accounting_day: u64,
    fair_cursor: usize,
}

impl SchedulerState {
    /// Creates initial persistable controller state for a validated policy.
    ///
    /// # Errors
    ///
    /// Returns a typed scheduler error when the supplied policy is invalid.
    pub fn new(policy: SchedulerPolicy) -> Result<Self, SchedulerError> {
        policy.validate()?;
        Ok(Self {
            policy_version: policy.version,
            batch: BatchController::new(policy.batch)?,
            circuit: CircuitBreaker::new(5, 30_000),
            server_backoff_until_unix_ms: None,
            background_bytes_today: 0,
            accounting_day: 0,
            fair_cursor: 0,
        })
    }

    /// Normalizes persisted transient state after restart and clock anomalies.
    pub fn normalize(&mut self, now_unix_ms: u64, maximum_deferral_ms: u64) {
        if let Some(deadline) = self.server_backoff_until_unix_ms {
            self.server_backoff_until_unix_ms =
                Some(deadline.min(now_unix_ms.saturating_add(maximum_deferral_ms)));
        }
        if let CircuitState::Open { until_unix_ms } = &mut self.circuit.state {
            *until_unix_ms = (*until_unix_ms).min(now_unix_ms.saturating_add(maximum_deferral_ms));
        }
    }
}

impl Default for SchedulerState {
    fn default() -> Self {
        let policy = SchedulerPolicy::default();
        Self {
            policy_version: policy.version,
            batch: BatchController {
                bounds: policy.batch,
                target_ops: policy.batch.target_ops,
                target_bytes: policy.batch.target_bytes,
                success_streak: 0,
            },
            circuit: CircuitBreaker::new(5, 30_000),
            server_backoff_until_unix_ms: None,
            background_bytes_today: 0,
            accounting_day: 0,
            fair_cursor: 0,
        }
    }
}

/// One selected item plus its pure bounded decision.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SelectedWork {
    pub work: WorkDescriptor,
    pub decision: SchedulingDecision,
}

/// Stateful weighted-fair selector over durable work descriptors.
pub struct AdaptiveScheduler<P = SchedulerPolicy> {
    policy: P,
    state: SchedulerState,
}

const FAIR_CYCLE: [WorkClass; 32] = [
    WorkClass::Critical,
    WorkClass::Critical,
    WorkClass::Critical,
    WorkClass::Critical,
    WorkClass::Critical,
    WorkClass::Critical,
    WorkClass::Critical,
    WorkClass::Critical,
    WorkClass::Critical,
    WorkClass::Critical,
    WorkClass::Critical,
    WorkClass::Critical,
    WorkClass::Critical,
    WorkClass::Critical,
    WorkClass::Critical,
    WorkClass::Critical,
    WorkClass::Interactive,
    WorkClass::Interactive,
    WorkClass::Interactive,
    WorkClass::Interactive,
    WorkClass::Interactive,
    WorkClass::Interactive,
    WorkClass::Interactive,
    WorkClass::Interactive,
    WorkClass::Normal,
    WorkClass::Normal,
    WorkClass::Normal,
    WorkClass::Normal,
    WorkClass::Bulk,
    WorkClass::Bulk,
    WorkClass::Background,
    WorkClass::Maintenance,
];

impl AdaptiveScheduler<SchedulerPolicy> {
    /// Creates a deterministic scheduler and initial controller state.
    ///
    /// # Errors
    ///
    /// Returns a typed scheduler error when the supplied policy is invalid.
    pub fn new(policy: SchedulerPolicy) -> Result<Self, SchedulerError> {
        let state = SchedulerState::new(policy)?;
        Ok(Self { policy, state })
    }

    /// Creates a scheduler, falling back to safe defaults for direct legacy construction.
    #[must_use]
    pub fn new_or_default(policy: SchedulerPolicy) -> Self {
        Self::new(policy).unwrap_or_else(|_| Self {
            policy: SchedulerPolicy::default(),
            state: SchedulerState::default(),
        })
    }
}

impl<P: SchedulingPolicy> AdaptiveScheduler<P> {
    pub fn from_state(policy: P, state: SchedulerState) -> Self {
        Self { policy, state }
    }

    #[must_use]
    pub const fn state(&self) -> SchedulerState {
        self.state
    }

    /// Selects one eligible item using bounded aging, dependency inheritance, and weighted turns.
    pub fn select(
        &mut self,
        work: &[WorkDescriptor],
        completed: &BTreeSet<WorkId>,
        context: &SchedulingContext,
    ) -> Option<SelectedWork> {
        if !self.state.circuit.allow(context.now_unix_ms)
            || self
                .state
                .server_backoff_until_unix_ms
                .is_some_and(|deadline| context.now_unix_ms < deadline)
        {
            return None;
        }
        let priorities = inherited_priorities(work, context.now_unix_ms, &self.policy);
        let candidates = work
            .iter()
            .filter_map(|item| {
                let priority = priorities.get(&item.work_id).copied()?;
                let mut decision = self.policy.decide(context, item, priority, completed);
                decision.max_batch_ops = decision.max_batch_ops.min(self.state.batch.target_ops);
                decision.max_batch_bytes =
                    decision.max_batch_bytes.min(self.state.batch.target_bytes);
                decision.eligible.then_some((item, decision))
            })
            .collect::<Vec<_>>();
        if candidates.is_empty() {
            return None;
        }
        for offset in 0..FAIR_CYCLE.len() {
            let cursor = (self.state.fair_cursor + offset) % FAIR_CYCLE.len();
            let class = FAIR_CYCLE[cursor];
            let selected = candidates
                .iter()
                .filter(|(_, decision)| WorkClass::from_rank(decision.priority.0) == class)
                .max_by_key(|(item, decision)| {
                    (
                        decision.priority,
                        std::cmp::Reverse(item.enqueued_at_unix_ms),
                        std::cmp::Reverse(item.work_id),
                    )
                });
            if let Some((item, decision)) = selected {
                self.state.fair_cursor = (cursor + 1) % FAIR_CYCLE.len();
                return Some(SelectedWork {
                    work: (*item).clone(),
                    decision: *decision,
                });
            }
        }
        None
    }

    pub fn record_success(&mut self) {
        self.state.batch.record_success();
        self.state.circuit.record_success();
    }

    pub fn record_overload(&mut self, now_unix_ms: u64, hints: ServerSchedulingHints) {
        self.state.batch.record_overload();
        self.state.batch.apply_server_hints(hints);
        self.state.circuit.record_failure(now_unix_ms);
        self.state.server_backoff_until_unix_ms = hints.retry_after_ms.map(|delay| {
            now_unix_ms.saturating_add(delay.min(self.policy.maximum_retry_deferral_ms()))
        });
    }
}

fn inherited_priorities<P: SchedulingPolicy>(
    work: &[WorkDescriptor],
    now_unix_ms: u64,
    policy: &P,
) -> BTreeMap<WorkId, EffectivePriority> {
    let mut priorities = work
        .iter()
        .map(|item| {
            let age_ms = now_unix_ms.saturating_sub(item.enqueued_at_unix_ms);
            let age_boost = u8::try_from(age_ms / policy.aging_quantum_ms().max(1))
                .unwrap_or(u8::MAX)
                .min(policy.maximum_age_boost());
            (
                item.work_id,
                EffectivePriority(item.class.rank().saturating_add(age_boost).min(5)),
            )
        })
        .collect::<BTreeMap<_, _>>();
    for _ in 0..work.len() {
        let mut changed = false;
        for item in work {
            let dependent = priorities[&item.work_id];
            for dependency in &item.dependencies {
                if let Some(prerequisite) = priorities.get_mut(dependency) {
                    if *prerequisite < dependent {
                        *prerequisite = dependent;
                        changed = true;
                    }
                }
            }
        }
        if !changed {
            break;
        }
    }
    priorities
}

/// Deterministic retry inputs.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct RetryPolicy {
    pub initial_ms: u64,
    pub maximum_ms: u64,
    pub multiplier: u32,
    pub jitter_percent: u8,
}

impl RetryPolicy {
    /// Computes bounded stable jitter and honors larger server Retry-After within the hard cap.
    #[must_use]
    pub fn delay_ms(self, retry: u32, stable_seed: u64, server_retry_after_ms: Option<u64>) -> u64 {
        let multiplier = u128::from(self.multiplier.max(1)).saturating_pow(retry);
        let base = u128::from(self.initial_ms)
            .saturating_mul(multiplier)
            .min(u128::from(self.maximum_ms));
        let radius = base.saturating_mul(u128::from(self.jitter_percent.min(100))) / 100;
        let width = radius.saturating_mul(2).saturating_add(1);
        let entropy = mix(stable_seed ^ u64::from(retry));
        let jittered = base
            .saturating_sub(radius)
            .saturating_add(u128::from(entropy) % width);
        u64::try_from(jittered)
            .unwrap_or(u64::MAX)
            .max(server_retry_after_ms.unwrap_or(0))
            .min(self.maximum_ms)
    }
}

const fn mix(mut value: u64) -> u64 {
    value ^= value >> 30;
    value = value.wrapping_mul(0xbf58_476d_1ce4_e5b9);
    value ^= value >> 27;
    value = value.wrapping_mul(0x94d0_49bb_1331_11eb);
    value ^ (value >> 31)
}

/// Scheduler configuration or persisted-state error.
#[derive(Clone, Copy, Debug, Error, Eq, PartialEq)]
pub enum SchedulerError {
    #[error("scheduler batch bounds must be non-zero and monotonic")]
    InvalidBatchBounds,
    #[error("scheduler policy has an invalid version, concurrency, or timing bound")]
    InvalidPolicy,
    #[error("persisted scheduler policy version does not match configured policy")]
    PolicyVersionMismatch,
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    fn work(id: u64, class: WorkClass, enqueued_at: u64) -> WorkDescriptor {
        WorkDescriptor {
            work_id: WorkId(id),
            kind: WorkKind::PushOperations,
            class,
            enqueued_at_unix_ms: enqueued_at,
            earliest_at_unix_ms: 0,
            deadline_unix_ms: None,
            dependencies: Vec::new(),
            estimated_operations: 1,
            estimated_bytes: 100,
            emergency_override: false,
        }
    }

    #[test]
    fn metered_and_low_power_defer_background_but_not_certified_critical_work() {
        let policy = SchedulerPolicy::for_profile(SyncProfile::Mobile);
        let context = SchedulingContext {
            network: NetworkContext {
                metered: Some(true),
                roaming: Some(true),
                ..NetworkContext::default()
            },
            power: PowerContext {
                low_power_mode: Some(true),
                ..PowerContext::default()
            },
            ..SchedulingContext::default()
        };
        let background = work(1, WorkClass::Background, 0);
        assert_eq!(
            policy
                .decide(
                    &context,
                    &background,
                    EffectivePriority(1),
                    &BTreeSet::new(),
                )
                .deferral,
            Some(DeferralReason::RoamingPolicy)
        );
        let mut critical = work(2, WorkClass::Critical, 0);
        critical.emergency_override = true;
        assert!(
            policy
                .decide(&context, &critical, EffectivePriority(5), &BTreeSet::new(),)
                .eligible
        );
    }

    #[test]
    fn dependency_inherits_priority_and_executes_before_dependent() {
        let mut dependent = work(2, WorkClass::Interactive, 1);
        dependent.dependencies.push(WorkId(1));
        let prerequisite = work(1, WorkClass::Background, 0);
        let mut scheduler = AdaptiveScheduler::new(SchedulerPolicy::default())
            .unwrap_or_else(|error| panic!("{error}"));
        let selected = scheduler
            .select(
                &[dependent, prerequisite],
                &BTreeSet::new(),
                &SchedulingContext::default(),
            )
            .unwrap_or_else(|| panic!("prerequisite should be selected"));
        assert_eq!(selected.work.work_id, WorkId(1));
        assert_eq!(selected.decision.priority, EffectivePriority(4));
    }

    #[test]
    fn weighted_cycle_gives_background_work_an_execution_opportunity() {
        let mut scheduler = AdaptiveScheduler::new(SchedulerPolicy::default())
            .unwrap_or_else(|error| panic!("{error}"));
        let context = SchedulingContext::default();
        let mut saw_background = false;
        for turn in 0..64 {
            let selected = scheduler
                .select(
                    &[
                        work(1_000 + turn, WorkClass::Critical, turn),
                        work(2_000 + turn, WorkClass::Background, turn),
                    ],
                    &BTreeSet::new(),
                    &context,
                )
                .unwrap_or_else(|| panic!("eligible work"));
            saw_background |= selected.work.class == WorkClass::Background;
        }
        assert!(saw_background);
    }

    #[test]
    fn circuit_breaker_and_persisted_backoff_survive_restart_without_unbounded_sleep() {
        let policy = SchedulerPolicy::default();
        let mut scheduler =
            AdaptiveScheduler::new(policy).unwrap_or_else(|error| panic!("{error}"));
        for now in 0..5 {
            scheduler.record_overload(
                now,
                ServerSchedulingHints {
                    retry_after_ms: Some(u64::MAX),
                    ..ServerSchedulingHints::default()
                },
            );
        }
        let mut state = scheduler.state();
        state.normalize(10, policy.maximum_retry_deferral_ms);
        assert!(state.server_backoff_until_unix_ms <= Some(10 + policy.maximum_retry_deferral_ms));
        assert!(matches!(state.circuit.state, CircuitState::Open { .. }));
    }

    #[test]
    fn server_hints_only_reduce_bounds_and_retry_after_is_capped() {
        let policy = SchedulerPolicy::default();
        let mut controller =
            BatchController::new(policy.batch).unwrap_or_else(|error| panic!("{error}"));
        let before = (controller.target_ops, controller.target_bytes);
        controller.apply_server_hints(ServerSchedulingHints {
            preferred_max_batch_ops: Some(usize::MAX),
            preferred_max_batch_bytes: Some(usize::MAX),
            ..ServerSchedulingHints::default()
        });
        assert_eq!((controller.target_ops, controller.target_bytes), before);

        let mut scheduler =
            AdaptiveScheduler::new(policy).unwrap_or_else(|error| panic!("{error}"));
        scheduler.record_overload(
            10,
            ServerSchedulingHints {
                retry_after_ms: Some(u64::MAX),
                ..ServerSchedulingHints::default()
            },
        );
        assert_eq!(
            scheduler.state().server_backoff_until_unix_ms,
            Some(10 + policy.maximum_retry_deferral_ms)
        );
    }

    #[test]
    fn selection_is_deterministic_and_does_not_mutate_descriptors() {
        let input = vec![work(2, WorkClass::Normal, 2), work(1, WorkClass::Normal, 1)];
        let original = input.clone();
        let mut first = AdaptiveScheduler::new(SchedulerPolicy::default())
            .unwrap_or_else(|error| panic!("{error}"));
        let mut second = AdaptiveScheduler::new(SchedulerPolicy::default())
            .unwrap_or_else(|error| panic!("{error}"));
        let context = SchedulingContext::default();
        let selected_first = first.select(&input, &BTreeSet::new(), &context);
        let selected_second = second.select(&input, &BTreeSet::new(), &context);
        assert_eq!(selected_first, selected_second);
        assert_eq!(input, original);
    }

    #[test]
    fn retry_delay_honors_server_minimum_without_crossing_hard_maximum() {
        let retry = RetryPolicy {
            initial_ms: 100,
            maximum_ms: 10_000,
            multiplier: 2,
            jitter_percent: 20,
        };
        assert_eq!(retry.delay_ms(1, 7, Some(5_000)), 5_000);
        assert_eq!(retry.delay_ms(1, 7, Some(u64::MAX)), 10_000);
    }

    proptest! {
        #[test]
        fn adaptive_batch_never_crosses_hard_bounds(overloads in 0_u8..20, successes in 0_u8..100) {
            let bounds = SchedulerPolicy::default().batch;
            let mut controller = BatchController::new(bounds)
                .unwrap_or_else(|error| panic!("{error}"));
            for _ in 0..overloads {
                controller.record_overload();
            }
            for _ in 0..successes {
                controller.record_success();
            }
            prop_assert!((bounds.min_ops..=bounds.max_ops).contains(&controller.target_ops));
            prop_assert!((bounds.min_bytes..=bounds.max_bytes).contains(&controller.target_bytes));
        }
    }
}
