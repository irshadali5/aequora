//! One-shot client synchronization and atomic reconciliation.

pub mod diagnostics;
pub mod resources;

/// Stable plug-and-play entry point for constructing a client engine.
#[derive(Clone, Copy, Debug, Default)]
pub struct AequoraClient;

impl AequoraClient {
    /// Starts the type-state client builder.
    #[must_use]
    pub fn builder() -> ClientSyncEngineBuilder {
        ClientSyncEngineBuilder::new()
    }
}

/// Focused imports for application client integrations.
pub mod prelude {
    pub use crate::diagnostics::{ClientDiagnosticSnapshot, ClientDiagnostics};
    pub use crate::resources::{
        AppLifecycleEvent, BackgroundBudget, ClientCapabilityProfile, ClientResourceAdmission,
        ClientResourceContext, ClientResourcePolicy, ClientResourceProfile, ClientStatus,
        ClientWork, ClientWorkKind, DefaultResourceAdmission, DurableWorkCheckpoint, EvictionPlan,
        LocalCommitReceipt, MemoryClass, PlatformResourceMonitor, ResourceDecision,
        ScopeCachePolicy, SnapshotCachePolicy, StorageState, ThermalState,
    };
    pub use crate::{
        AdaptiveBatchConfig, AdaptiveBatcher, AequoraClient, BootstrapOutcome, ClientBuildError,
        ClientConfig, ClientError, ClientReadSession, ClientSyncEngine, ClientSyncEngineBuilder,
        CoordinatorClosed, CoordinatorStatus, MultiProcessCoordinatorConfig, RetryConfig,
        SyncCoordinator, SyncCoordinatorConfig, SyncCoordinatorHandle, SyncHealth, SyncOutcome,
        SyncStatus, SyncSummary, SyncTrigger,
    };
    pub use aequora_coordination::{
        FencingToken, LocalProcessMode, LocalStoreGeneration, LocalStoreId, ProcessInstanceId,
    };
    pub use aequora_live::{HintWakeOutcome, HintWakeTracker, SyncHint};
    pub use aequora_protocol::{ClientLimits, SessionMetadata, SnapshotLimits};
    pub use aequora_scheduler::{
        AppActivity, NetworkContext, PowerContext, SchedulerPolicy, SchedulerState,
        SchedulingContext, SyncProfile, WorkClass,
    };
    pub use aequora_scope::{
        LocalScopeState, ScopeTransition, ScopeTransitionOutcome, Subscription,
    };
    pub use aequora_store::{
        AdapterCapabilities, AdapterManifest, AdapterManifestProvider, AdapterRequirements,
        AdapterRole, AdapterTier, LocalStore, ProductionAdapterPair, ScopeStateStore,
    };
    pub use aequora_transport::{StreamingSyncTransport, SyncTransport};
    pub use aequora_types::{DeviceId, SyncScopeId, TenantId};
}

use aequora_coordination::{
    LeaseGrant, LeaseKind, LeaseRequest, LocalCoordinationSupport, LocalProcessMode,
    ProcessInstanceId,
};
use aequora_live::{HintWakeOutcome, HintWakeTracker, SyncHint};
use aequora_observability::{
    LocalCoordinationEventKind, MetricEvent, NoopObserver, Observer, OutcomeKind,
    SchedulerEventKind, ScopeEventKind, TraceContext,
};
use aequora_protocol::{
    BootstrapRequest, BootstrapResponse, Capability, ClientLimits, PushHint, ResyncReason,
    SessionMetadata, SnapshotLimits, SyncDirective, SyncRequest, SyncResponse,
};
use aequora_queue::OptimizationRegistry;
use aequora_region::{ReadResponseMetadata, RegionError, SessionWatermark};
use aequora_scheduler::{
    AdaptiveScheduler, DeferralReason, SchedulerPolicy, SchedulerState, SchedulingContext,
    WorkClass, WorkDescriptor, WorkId, WorkKind,
};
use aequora_scope::{ScopeTransition, ScopeTransitionKind, ScopeTransitionOutcome, Subscription};
use aequora_store::{
    AdapterCompatibilityError, AdapterManifestProvider, AdapterRequirements,
    LocalCoordinationStore, LocalStore, ScopeStateStore, StoreError, StoreErrorKind,
};
use aequora_transport::{
    StreamingSyncTransport, SyncTransport, TransportError, TransportErrorKind,
};
use aequora_types::{
    AuthorityEpoch, AuthorityId, Cursor, OperationId, ProtocolVersion, RequestId, Sequence,
    SnapshotId,
};
use resources::{
    ClientResourceAdmission, ClientResourceContext, ClientResourcePolicy, ClientWork,
    ClientWorkKind, DefaultResourceAdmission, ResourceDecision, ResourceReason, WorkEstimate,
};
use std::{
    collections::HashSet,
    sync::{
        Arc, Mutex, MutexGuard,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use thiserror::Error;
use tokio::sync::{mpsc, watch};

trait ReservationFlag {
    fn try_set(&self) -> bool;
    fn clear(&self);
}

/// Session/read-your-writes lower bound for web or nonreplicated regional API reads.
///
/// Native synchronized datasets continue to read from their local store. This tracker is for
/// server reads whose response metadata carries an authoritative epoch and served sequence.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ClientReadSession {
    watermark: Option<SessionWatermark>,
}

impl ClientReadSession {
    /// Current lower bound to attach to a `Session` regional read.
    #[must_use]
    pub const fn watermark(self) -> Option<SessionWatermark> {
        self.watermark
    }

    /// Advances read-your-writes after an authoritative operation commits.
    ///
    /// # Errors
    ///
    /// Rejects an authority change or epoch rollback until Part 16 transition handling completes.
    pub fn observe_commit(
        &mut self,
        authority_id: AuthorityId,
        authority_epoch: AuthorityEpoch,
        sequence: Sequence,
    ) -> Result<(), RegionError> {
        self.observe(authority_id, authority_epoch, sequence)
    }

    /// Advances session consistency after a guarded regional or authority read.
    ///
    /// # Errors
    ///
    /// Rejects an authority change or epoch rollback.
    pub fn observe_read(&mut self, metadata: ReadResponseMetadata) -> Result<(), RegionError> {
        self.observe(
            metadata.authority_id,
            metadata.authority_epoch,
            metadata.served_sequence,
        )
    }

    fn observe(
        &mut self,
        authority_id: AuthorityId,
        authority_epoch: AuthorityEpoch,
        sequence: Sequence,
    ) -> Result<(), RegionError> {
        if let Some(watermark) = &mut self.watermark {
            watermark.observe(authority_id, authority_epoch, sequence)
        } else {
            self.watermark = Some(SessionWatermark::new(
                authority_id,
                authority_epoch,
                sequence,
            ));
            Ok(())
        }
    }
}

impl ReservationFlag for AtomicBool {
    fn try_set(&self) -> bool {
        self.compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed)
            .is_ok()
    }

    fn clear(&self) {
        self.store(false, Ordering::Release);
    }
}

struct ReservationCore<F> {
    flag: F,
}

impl<F> ReservationCore<F>
where
    F: ReservationFlag,
{
    const fn new(flag: F) -> Self {
        Self { flag }
    }

    fn try_acquire(&self) -> Option<ReservationGuard<'_, F>> {
        self.flag
            .try_set()
            .then_some(ReservationGuard { reservation: self })
    }
}

struct ReservationGuard<'a, F>
where
    F: ReservationFlag,
{
    reservation: &'a ReservationCore<F>,
}

impl<F> Drop for ReservationGuard<'_, F>
where
    F: ReservationFlag,
{
    fn drop(&mut self) {
        self.reservation.flag.clear();
    }
}

/// Bounded exponential retry policy with symmetric jitter.
#[derive(Clone, Copy, Debug)]
pub struct RetryConfig {
    /// Total exchange attempts, including the initial attempt.
    pub max_attempts: u32,
    /// Delay before the first retry.
    pub initial_delay: Duration,
    /// Maximum delay between attempts.
    pub max_delay: Duration,
    /// Integer exponential multiplier, at least one.
    pub multiplier: u32,
    /// Symmetric jitter as a percentage from zero through one hundred.
    pub jitter_percent: u8,
}

impl Default for RetryConfig {
    fn default() -> Self {
        Self {
            max_attempts: 5,
            initial_delay: Duration::from_millis(500),
            max_delay: Duration::from_secs(30),
            multiplier: 2,
            jitter_percent: 20,
        }
    }
}

impl RetryConfig {
    /// Calculates a capped delay for a zero-based retry number. `entropy` makes jitter
    /// deterministic for tests while callers may supply system entropy in production.
    #[must_use]
    pub fn delay(self, retry: u32, entropy: u64) -> Duration {
        let multiplier = u128::from(self.multiplier.max(1)).saturating_pow(retry);
        let initial_ms = self.initial_delay.as_millis();
        let maximum_ms = self.max_delay.as_millis();
        let base_ms = initial_ms.saturating_mul(multiplier).min(maximum_ms);
        let jitter_percent = u128::from(self.jitter_percent.min(100));
        let radius = base_ms.saturating_mul(jitter_percent) / 100;
        let width = radius.saturating_mul(2).saturating_add(1);
        let offset = if width == 0 {
            0
        } else {
            u128::from(entropy) % width
        };
        let jittered = base_ms.saturating_sub(radius).saturating_add(offset);
        Duration::from_millis(u64::try_from(jittered).unwrap_or(u64::MAX))
    }

    /// Applies a capped server `Retry-After` floor while retaining non-negative client jitter.
    #[must_use]
    pub fn delay_with_server(
        self,
        retry: u32,
        entropy: u64,
        server_retry_after: Option<Duration>,
    ) -> Duration {
        let local = self.delay(retry, entropy);
        let Some(server) = server_retry_after else {
            return local;
        };
        let floor_ms = server.as_millis().min(self.max_delay.as_millis());
        if floor_ms <= local.as_millis() {
            return local;
        }
        let jitter_window = floor_ms.saturating_mul(u128::from(self.jitter_percent.min(100))) / 100;
        let extra = if jitter_window == 0 {
            0
        } else {
            u128::from(entropy) % jitter_window.saturating_add(1)
        };
        let delayed = floor_ms
            .saturating_add(extra)
            .min(self.max_delay.as_millis());
        Duration::from_millis(u64::try_from(delayed).unwrap_or(u64::MAX))
    }
}

/// Conservative additive-increase/multiplicative-decrease batch tuning.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AdaptiveBatchConfig {
    /// Smallest push batch after congestion.
    pub minimum_operations: usize,
    /// Largest push batch regardless of observed latency.
    pub maximum_operations: usize,
    /// Additive growth after a fast successful exchange.
    pub increase_step: usize,
    /// Latency at or below which a complete batch may grow.
    pub target_latency: Duration,
}

/// Stateful deterministic batch-size controller.
#[derive(Clone, Debug)]
pub struct AdaptiveBatcher {
    current: usize,
    adaptive: Option<AdaptiveBatchConfig>,
}

impl AdaptiveBatcher {
    /// Creates a static controller or clamps the initial size to adaptive bounds.
    #[must_use]
    pub fn new(initial: usize, adaptive: Option<AdaptiveBatchConfig>) -> Self {
        let current = adaptive.map_or(initial.max(1), |config| {
            initial.clamp(
                config.minimum_operations.max(1),
                config.maximum_operations.max(1),
            )
        });
        Self { current, adaptive }
    }

    /// Current maximum operations for the next request.
    #[must_use]
    pub const fn limit(&self) -> usize {
        self.current
    }

    /// Records a valid exchange and tunes only when adaptive mode is enabled.
    pub fn record_success(&mut self, latency: Duration, submitted: usize, terminal: usize) {
        let Some(config) = self.adaptive else { return };
        let minimum = config.minimum_operations.max(1);
        let maximum = config.maximum_operations.max(minimum);
        if latency <= config.target_latency && submitted > 0 && terminal >= submitted {
            self.current = self
                .current
                .saturating_add(config.increase_step)
                .min(maximum);
        } else if latency > config.target_latency {
            self.current = self.current.div_ceil(2).max(minimum);
        }
    }

    /// Treats a transient exchange failure as congestion and halves the next batch.
    pub fn record_failure(&mut self) {
        let Some(config) = self.adaptive else { return };
        self.current = self
            .current
            .div_ceil(2)
            .max(config.minimum_operations.max(1));
    }
}

/// Deterministic client configuration for bounded batching, retries, and bootstrap.
#[derive(Clone, Debug)]
pub struct ClientConfig {
    /// Wire protocol version emitted and accepted by this client.
    pub protocol: ProtocolVersion,
    /// Stable session identity and requested scope.
    pub session: SessionMetadata,
    /// Maximum pending operations submitted in one exchange.
    pub push_batch_size: usize,
    /// Maximum uncompressed framed Postcard bytes sent in one exchange.
    pub push_batch_bytes: usize,
    /// Response limits advertised to the server.
    pub limits: ClientLimits,
    /// Supported protocol features.
    pub capabilities: Vec<Capability>,
    /// Retry policy for transient transport and storage failures.
    pub retry: RetryConfig,
    /// Safety bound for push batches and pull pages in one [`ClientSyncEngine::sync`] call.
    pub max_exchanges_per_sync: usize,
    /// Entity and byte bounds for each bootstrap page.
    pub snapshot_limits: SnapshotLimits,
    /// Optional latency-driven tuning. `None` retains static deterministic batching.
    pub adaptive_batching: Option<AdaptiveBatchConfig>,
    /// Platform-neutral `QoS` profile and hard scheduling limits.
    pub scheduler: SchedulerPolicy,
    /// Device resource caps layered after scheduler desire and server maximums.
    pub resources: ClientResourcePolicy,
}

impl ClientConfig {
    /// Creates a configuration with conservative deterministic defaults.
    #[must_use]
    pub fn new(session: SessionMetadata) -> Self {
        let resources = ClientResourcePolicy::default();
        let mut config = Self {
            protocol: ProtocolVersion::V1,
            session,
            push_batch_size: 256,
            push_batch_bytes: 1_024 * 1_024,
            limits: ClientLimits::default(),
            capabilities: vec![
                Capability::PostcardV1,
                Capability::Tombstones,
                Capability::LineageV1,
                Capability::IntegrityV1,
                Capability::AuthorityEpochV1,
                Capability::ResourceConstrainedV1,
            ],
            retry: RetryConfig::default(),
            max_exchanges_per_sync: 1_024,
            snapshot_limits: SnapshotLimits::default(),
            adaptive_batching: None,
            scheduler: SchedulerPolicy::default(),
            resources,
        };
        config.apply_resource_policy(resources);
        config
    }

    /// Selects a validated built-in resource baseline and caps all related wire/scheduler limits.
    #[must_use]
    pub fn with_resource_profile(mut self, profile: resources::ClientResourceProfile) -> Self {
        self.apply_resource_policy(ClientResourcePolicy::for_profile(profile));
        self
    }

    /// Applies client resource ceilings without ever increasing application-configured limits.
    pub fn apply_resource_policy(&mut self, policy: ClientResourcePolicy) {
        self.resources = policy;
        let memory = policy.memory;
        self.push_batch_size = self
            .push_batch_size
            .min(memory.max_batch_operations as usize)
            .max(1);
        self.push_batch_bytes = self
            .push_batch_bytes
            .min(memory.sync_decode_bytes as usize)
            .max(1);
        self.limits.max_changes = self
            .limits
            .max_changes
            .min(memory.max_batch_operations)
            .max(1);
        self.limits.max_response_bytes = self
            .limits
            .max_response_bytes
            .min(memory.sync_response_bytes)
            .max(1);
        self.snapshot_limits.max_entities = self
            .snapshot_limits
            .max_entities
            .min(memory.max_batch_operations.saturating_mul(4))
            .max(1);
        self.snapshot_limits.max_payload_bytes = self
            .snapshot_limits
            .max_payload_bytes
            .min(memory.snapshot_chunk_bytes)
            .max(1);
        self.scheduler.batch.max_ops = self
            .scheduler
            .batch
            .max_ops
            .min(memory.max_batch_operations as usize)
            .max(1);
        self.scheduler.batch.target_ops = self
            .scheduler
            .batch
            .target_ops
            .min(self.scheduler.batch.max_ops)
            .max(self.scheduler.batch.min_ops);
        self.scheduler.batch.max_bytes = self
            .scheduler
            .batch
            .max_bytes
            .min(memory.sync_decode_bytes as usize)
            .max(1);
        self.scheduler.batch.target_bytes = self
            .scheduler
            .batch
            .target_bytes
            .min(self.scheduler.batch.max_bytes)
            .max(self.scheduler.batch.min_bytes);
        self.scheduler.concurrency = self
            .scheduler
            .concurrency
            .min(usize::from(memory.max_parallel_transfers))
            .max(1);
        if !self
            .capabilities
            .contains(&Capability::ResourceConstrainedV1)
        {
            self.capabilities.push(Capability::ResourceConstrainedV1);
        }
    }
}

/// Summary of a successfully reconciled exchange.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SyncOutcome {
    /// Operations acknowledged, including safe retries.
    pub acknowledged: usize,
    /// Permanently rejected operations persisted for application inspection.
    pub rejected: usize,
    /// Conflicts persisted for application resolution.
    pub conflicts: usize,
    /// Authoritative changes atomically applied.
    pub changes: usize,
    /// Whether the server has another pull page ready.
    pub has_more: bool,
}

/// Aggregate result of a fully drained multi-batch synchronization run.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct SyncSummary {
    /// Successful exchanges performed.
    pub exchanges: usize,
    /// Total acknowledged operations.
    pub acknowledged: usize,
    /// Total permanent rejections.
    pub rejected: usize,
    /// Total conflicts.
    pub conflicts: usize,
    /// Total authoritative changes reconciled.
    pub changes: usize,
}

/// Result of atomically installing one resumable bootstrap snapshot.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BootstrapOutcome {
    /// Consistent snapshot installed locally.
    pub snapshot_id: SnapshotId,
    /// Snapshot pages durably staged.
    pub pages: usize,
    /// Total authoritative entities installed.
    pub entities: usize,
    /// Incremental synchronization boundary.
    pub cursor: Cursor,
}

impl SyncSummary {
    fn record(&mut self, outcome: SyncOutcome) {
        self.exchanges = self.exchanges.saturating_add(1);
        self.acknowledged = self.acknowledged.saturating_add(outcome.acknowledged);
        self.rejected = self.rejected.saturating_add(outcome.rejected);
        self.conflicts = self.conflicts.saturating_add(outcome.conflicts);
        self.changes = self.changes.saturating_add(outcome.changes);
    }
}

/// Event that may wake the background synchronization coordinator.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SyncTrigger {
    /// A local transaction appended a new outbox operation.
    LocalMutation,
    /// Connectivity became available.
    NetworkAvailable,
    /// Connectivity became unavailable.
    NetworkUnavailable,
    /// A user or host application explicitly requested synchronization.
    Manual,
    /// A payload-free server hint suggested pulling.
    PushHint,
    /// Stop the coordinator gracefully.
    Shutdown,
}

/// Observable transport-independent background synchronization state.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SyncStatus {
    /// Waiting for work, optionally with the most recent successful summary.
    Idle { last_sync: Option<SyncSummary> },
    /// The host reported no usable network path.
    Offline,
    /// A synchronization drain is in progress.
    Synchronizing,
    /// Synchronization completed but application conflict handling is required.
    Conflict { summary: SyncSummary },
    /// The latest drain failed. The coordinator remains alive for future triggers.
    Error { transient: bool },
    /// The coordinator stopped gracefully.
    Shutdown,
}

/// Observable local-process election role, independent of synchronization health.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CoordinatorStatus {
    /// The host selected the legacy one-process coordinator path.
    SingleProcess,
    /// This process owns the current durable sync lease.
    Leader {
        token: aequora_coordination::FencingToken,
    },
    /// Another process owns the lease or this process is awaiting takeover.
    Follower,
    /// Read-only process that does not participate in election.
    Observer,
    /// An exclusive maintenance lease currently blocks normal synchronization.
    Maintenance,
    /// No healthy leader is currently visible.
    NoLeader,
    /// The local coordinator stopped gracefully.
    Shutdown,
}

/// Observable UI-independent synchronization health and durable queue gauges.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SyncHealth {
    /// Current coordinator state.
    pub status: SyncStatus,
    /// Replayable durable outbox depth.
    pub pending_operations: usize,
    /// Age of the oldest replayable operation.
    pub oldest_pending_age_ms: Option<u64>,
    /// Durable unresolved conflicts awaiting application input.
    pub conflicts_pending: usize,
    /// Unix timestamp of the latest successful synchronization drain.
    pub last_successful_sync_unix_ms: Option<u64>,
}

/// Bounded coordinator channel and optional periodic wake interval.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SyncCoordinatorConfig {
    /// Maximum queued wake signals. Repeated signals may be coalesced by callers.
    pub channel_capacity: usize,
    /// Optional periodic synchronization interval. Zero disables periodic wakes.
    pub periodic_interval: Option<Duration>,
    /// Whether to drain immediately before waiting for the first trigger.
    pub sync_on_start: bool,
    /// Maximum time spent coalescing a burst of local-mutation wakes.
    pub mutation_debounce: Duration,
}

/// Durable multi-process election timing and runtime identity.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MultiProcessCoordinatorConfig {
    /// Ephemeral identity generated once for this runtime process.
    pub process_id: ProcessInstanceId,
    /// Lease duration. Must exceed twice the heartbeat interval.
    pub lease_ttl: Duration,
    /// Renewal and follower election interval.
    pub heartbeat_interval: Duration,
}

impl Default for MultiProcessCoordinatorConfig {
    fn default() -> Self {
        Self {
            process_id: ProcessInstanceId::new(),
            lease_ttl: Duration::from_secs(15),
            heartbeat_interval: Duration::from_secs(5),
        }
    }
}

impl Default for SyncCoordinatorConfig {
    fn default() -> Self {
        Self {
            channel_capacity: 32,
            periodic_interval: Some(Duration::from_secs(30)),
            sync_on_start: false,
            mutation_debounce: Duration::from_millis(200),
        }
    }
}

/// The background coordinator has already stopped.
#[derive(Clone, Copy, Debug, Error, Eq, PartialEq)]
#[error("sync coordinator is closed")]
pub struct CoordinatorClosed;

/// Cloneable control and observation handle for a running [`SyncCoordinator`].
#[derive(Clone)]
pub struct SyncCoordinatorHandle {
    triggers: mpsc::Sender<SyncTrigger>,
    status: watch::Receiver<SyncStatus>,
    health: watch::Receiver<SyncHealth>,
    coordinator_status: watch::Receiver<CoordinatorStatus>,
}

impl SyncCoordinatorHandle {
    /// Sends one bounded wake signal.
    ///
    /// # Errors
    ///
    /// Returns [`CoordinatorClosed`] after the coordinator exits.
    pub async fn trigger(&self, trigger: SyncTrigger) -> Result<(), CoordinatorClosed> {
        self.triggers
            .send(trigger)
            .await
            .map_err(|_| CoordinatorClosed)
    }

    /// Validates and coalesces one advisory live hint, then schedules only the normal durable
    /// exchange path. The tracker has no access to cursor or replica mutation APIs.
    ///
    /// # Errors
    ///
    /// Returns [`CoordinatorClosed`] when a valid new wake cannot reach the coordinator.
    pub async fn observe_live_hint(
        &self,
        tracker: &HintWakeTracker,
        hint: SyncHint,
    ) -> Result<HintWakeOutcome, CoordinatorClosed> {
        let outcome = tracker.observe(hint);
        if matches!(outcome, HintWakeOutcome::Wake { .. }) {
            self.trigger(SyncTrigger::PushHint).await?;
        }
        Ok(outcome)
    }

    /// Subscribes to status changes without coupling the engine to a UI framework.
    #[must_use]
    pub fn subscribe(&self) -> watch::Receiver<SyncStatus> {
        self.status.clone()
    }

    /// Returns the current status snapshot.
    #[must_use]
    pub fn status(&self) -> SyncStatus {
        *self.status.borrow()
    }

    /// Subscribes to state plus durable queue/conflict gauges.
    #[must_use]
    pub fn subscribe_health(&self) -> watch::Receiver<SyncHealth> {
        self.health.clone()
    }

    /// Returns the current health snapshot.
    #[must_use]
    pub fn health(&self) -> SyncHealth {
        *self.health.borrow()
    }

    /// Subscribes to durable local-process leadership changes.
    #[must_use]
    pub fn subscribe_coordinator_status(&self) -> watch::Receiver<CoordinatorStatus> {
        self.coordinator_status.clone()
    }

    /// Returns the current local-process leadership role.
    #[must_use]
    pub fn coordinator_status(&self) -> CoordinatorStatus {
        *self.coordinator_status.borrow()
    }
}

/// Generic background worker that drains the same verified [`ClientSyncEngine`] API used by
/// foreground callers.
pub struct SyncCoordinator<L, T> {
    engine: Arc<ClientSyncEngine<L, T>>,
    config: SyncCoordinatorConfig,
    triggers: mpsc::Receiver<SyncTrigger>,
    status: watch::Sender<SyncStatus>,
    health: watch::Sender<SyncHealth>,
    coordinator_status: watch::Sender<CoordinatorStatus>,
    online: bool,
    last_successful_sync_unix_ms: Option<u64>,
}

impl<L, T> SyncCoordinator<L, T> {
    /// Creates a coordinator and its cloneable host/UI handle.
    #[must_use]
    pub fn new(
        engine: Arc<ClientSyncEngine<L, T>>,
        config: SyncCoordinatorConfig,
    ) -> (Self, SyncCoordinatorHandle) {
        let (trigger_sender, triggers) = mpsc::channel(config.channel_capacity.max(1));
        let (status, status_receiver) = watch::channel(SyncStatus::Idle { last_sync: None });
        let (health, health_receiver) = watch::channel(SyncHealth {
            status: SyncStatus::Idle { last_sync: None },
            pending_operations: 0,
            oldest_pending_age_ms: None,
            conflicts_pending: 0,
            last_successful_sync_unix_ms: None,
        });
        let (coordinator_status, coordinator_status_receiver) =
            watch::channel(CoordinatorStatus::SingleProcess);
        (
            Self {
                engine,
                config,
                triggers,
                status,
                health,
                coordinator_status,
                online: true,
                last_successful_sync_unix_ms: None,
            },
            SyncCoordinatorHandle {
                triggers: trigger_sender,
                status: status_receiver,
                health: health_receiver,
                coordinator_status: coordinator_status_receiver,
            },
        )
    }
}

impl<L, T> SyncCoordinator<L, T>
where
    L: LocalStore + 'static,
    T: SyncTransport + 'static,
{
    /// Runs until shutdown is requested or every control handle is dropped.
    pub async fn run(mut self) {
        if self.config.sync_on_start {
            self.engine.request_work_class(WorkClass::Normal);
            self.synchronize().await;
        }
        let mut periodic = self
            .config
            .periodic_interval
            .filter(|interval| !interval.is_zero())
            .map(|period| tokio::time::interval_at(tokio::time::Instant::now() + period, period));
        loop {
            let trigger = match &mut periodic {
                Some(timer) => tokio::select! {
                    trigger = self.triggers.recv() => trigger,
                    _instant = timer.tick() => {
                        self.engine.request_work_class(WorkClass::Background);
                        Some(SyncTrigger::Manual)
                    },
                },
                None => self.triggers.recv().await,
            };
            let Some(trigger) = trigger else {
                self.set_status(SyncStatus::Shutdown);
                return;
            };
            match trigger {
                SyncTrigger::NetworkUnavailable => {
                    self.online = false;
                    self.set_status(SyncStatus::Offline);
                }
                SyncTrigger::NetworkAvailable => {
                    self.online = true;
                    self.engine.request_work_class(WorkClass::Normal);
                    self.synchronize().await;
                }
                SyncTrigger::Shutdown => {
                    self.set_status(SyncStatus::Shutdown);
                    return;
                }
                SyncTrigger::Manual | SyncTrigger::PushHint if self.online => {
                    if trigger == SyncTrigger::PushHint
                        || *self.engine.requested_work_class() != WorkClass::Background
                    {
                        self.engine.request_work_class(WorkClass::Interactive);
                    }
                    self.synchronize().await;
                }
                SyncTrigger::LocalMutation if self.online => {
                    self.engine.request_work_class(WorkClass::Interactive);
                    if !self.debounce_local_mutations().await {
                        return;
                    }
                }
                SyncTrigger::LocalMutation | SyncTrigger::Manual | SyncTrigger::PushHint => {}
            }
        }
    }

    /// Runs durable local election, heartbeat renewal, follower takeover, and leader-only sync.
    ///
    /// Followers remain idle sync observers; application repositories may continue committing
    /// domain state plus outbox operations through the shared local store.
    ///
    /// # Errors
    ///
    /// Rejects an unsupported adapter or unsafe timing configuration. Permanent storage failures
    /// during election are also returned; ordinary lease contention remains follower state.
    pub async fn run_multi_process(
        mut self,
        election: MultiProcessCoordinatorConfig,
    ) -> Result<(), ClientError>
    where
        L: LocalCoordinationStore,
    {
        if self.engine.store().coordination_support() != LocalCoordinationSupport::Full {
            return Err(ClientError::CoordinationUnsupported);
        }
        if election.heartbeat_interval.is_zero()
            || election.lease_ttl <= election.heartbeat_interval.saturating_mul(2)
        {
            return Err(ClientError::CoordinationConfig);
        }
        if matches!(
            self.engine.process_mode,
            LocalProcessMode::SingleProcess | LocalProcessMode::Observer
        ) {
            return Err(ClientError::CoordinationMode);
        }
        self.engine
            .coordination_required
            .store(true, Ordering::Release);
        self.set_coordinator_status(CoordinatorStatus::NoLeader);
        let mut lease = None;
        let mut heartbeat = tokio::time::interval(election.heartbeat_interval);
        let mut next_periodic = self
            .config
            .periodic_interval
            .map(|period| Instant::now() + period);
        loop {
            tokio::select! {
                trigger = self.triggers.recv() => {
                    let Some(trigger) = trigger else {
                        self.release_coordination_lease(&mut lease).await;
                        return Ok(());
                    };
                    if trigger == SyncTrigger::Shutdown {
                        self.release_coordination_lease(&mut lease).await;
                        self.set_status(SyncStatus::Shutdown);
                        return Ok(());
                    }
                    match trigger {
                        SyncTrigger::NetworkUnavailable => {
                            self.online = false;
                            self.set_status(SyncStatus::Offline);
                        }
                        SyncTrigger::NetworkAvailable => self.online = true,
                        SyncTrigger::LocalMutation | SyncTrigger::Manual | SyncTrigger::PushHint
                            if self.online => {
                                self.engine.request_work_class(match trigger {
                                    SyncTrigger::LocalMutation | SyncTrigger::Manual => WorkClass::Interactive,
                                    _ => WorkClass::Normal,
                                });
                                self.ensure_coordination_lease(&election, &mut lease).await?;
                                if lease.is_some() {
                                    self.synchronize_multi_process(&election, &mut lease).await;
                                }
                            }
                        SyncTrigger::LocalMutation
                        | SyncTrigger::Manual
                        | SyncTrigger::PushHint
                        | SyncTrigger::Shutdown => {}
                    }
                }
                _instant = heartbeat.tick() => {
                    self.ensure_coordination_lease(&election, &mut lease).await?;
                    if lease.is_some()
                        && self.online
                        && next_periodic.is_some_and(|deadline| Instant::now() >= deadline)
                    {
                        self.engine.request_work_class(WorkClass::Background);
                        self.synchronize_multi_process(&election, &mut lease).await;
                        next_periodic = self
                            .config
                            .periodic_interval
                            .map(|period| Instant::now() + period);
                    }
                }
            }
        }
    }

    async fn ensure_coordination_lease(
        &self,
        election: &MultiProcessCoordinatorConfig,
        lease: &mut Option<LeaseGrant>,
    ) -> Result<(), ClientError>
    where
        L: LocalCoordinationStore,
    {
        let now = unix_time_ms();
        let ttl_ms = u64::try_from(election.lease_ttl.as_millis()).unwrap_or(u64::MAX);
        if let Some(current) = *lease {
            if let Ok(renewed) = self.engine.store().renew_lease(current, now, ttl_ms).await {
                *lease = Some(renewed);
                self.engine.install_lease(renewed);
                self.record_coordination(LocalCoordinationEventKind::Renewed);
                return Ok(());
            }
            *lease = None;
            self.record_coordination(LocalCoordinationEventKind::Lost);
            self.set_coordinator_status(CoordinatorStatus::Follower);
        }
        let request = LeaseRequest {
            owner_id: election.process_id,
            kind: LeaseKind::SyncCoordinator,
            now_unix_ms: now,
            ttl_ms,
        };
        match self.engine.store().acquire_lease(request).await {
            Ok(acquired) => {
                self.engine.install_lease(acquired);
                *lease = Some(acquired);
                self.record_coordination(LocalCoordinationEventKind::Acquired);
                self.set_coordinator_status(CoordinatorStatus::Leader {
                    token: acquired.fencing_token,
                });
            }
            Err(error) if error.reason == aequora_store::StoreErrorReason::LeadershipLost => {
                let snapshot = self.engine.store().coordination_snapshot().await?;
                let status = if snapshot.is_active(now) && snapshot.kind == LeaseKind::Maintenance {
                    CoordinatorStatus::Maintenance
                } else if snapshot.is_active(now) {
                    CoordinatorStatus::Follower
                } else {
                    CoordinatorStatus::NoLeader
                };
                self.set_coordinator_status(status);
            }
            Err(error) => return Err(error.into()),
        }
        Ok(())
    }

    async fn synchronize_multi_process(
        &mut self,
        election: &MultiProcessCoordinatorConfig,
        lease: &mut Option<LeaseGrant>,
    ) where
        L: LocalCoordinationStore,
    {
        self.set_status(SyncStatus::Synchronizing);
        let mut synchronization = Box::pin(self.engine.sync());
        let mut heartbeat = tokio::time::interval(election.heartbeat_interval);
        let _initial_tick = heartbeat.tick().await;
        let result = loop {
            tokio::select! {
                result = &mut synchronization => break Some(result),
                _instant = heartbeat.tick() => {
                    let Some(current) = *lease else {
                        break None;
                    };
                    let now = unix_time_ms();
                    let ttl_ms = u64::try_from(election.lease_ttl.as_millis()).unwrap_or(u64::MAX);
                    if let Ok(renewed) = self.engine.store().renew_lease(current, now, ttl_ms).await {
                        *lease = Some(renewed);
                        self.engine.install_lease(renewed);
                        self.record_coordination(LocalCoordinationEventKind::Renewed);
                    } else {
                        *lease = None;
                        self.record_coordination(LocalCoordinationEventKind::Lost);
                        self.set_coordinator_status(CoordinatorStatus::Follower);
                        break None;
                    }
                }
            }
        };
        let status = match result {
            Some(Ok(summary)) if summary.conflicts > 0 => {
                self.last_successful_sync_unix_ms = Some(unix_time_ms());
                SyncStatus::Conflict { summary }
            }
            Some(Ok(summary)) => {
                self.last_successful_sync_unix_ms = Some(unix_time_ms());
                SyncStatus::Idle {
                    last_sync: Some(summary),
                }
            }
            Some(Err(error)) => SyncStatus::Error {
                transient: error.is_transient(),
            },
            None => SyncStatus::Idle { last_sync: None },
        };
        self.set_status(status);
        self.refresh_health(status).await;
        self.engine.request_work_class(WorkClass::Normal);
    }

    async fn release_coordination_lease(&self, lease: &mut Option<LeaseGrant>)
    where
        L: LocalCoordinationStore,
    {
        if let Some(current) = lease.take() {
            if self
                .engine
                .store()
                .release_lease(current, unix_time_ms())
                .await
                .is_ok()
            {
                self.record_coordination(LocalCoordinationEventKind::Released);
            }
        }
        self.engine.clear_lease();
        self.set_coordinator_status(CoordinatorStatus::Shutdown);
    }

    fn set_coordinator_status(&self, status: CoordinatorStatus) {
        let _previous = self.coordinator_status.send_replace(status);
    }

    fn record_coordination(&self, kind: LocalCoordinationEventKind) {
        self.engine
            .observer
            .record(MetricEvent::LocalCoordination { kind });
    }

    async fn debounce_local_mutations(&mut self) -> bool {
        if !self.config.mutation_debounce.is_zero() {
            tokio::time::sleep(self.config.mutation_debounce).await;
        }
        while let Ok(trigger) = self.triggers.try_recv() {
            match trigger {
                SyncTrigger::NetworkUnavailable => {
                    self.online = false;
                    self.set_status(SyncStatus::Offline);
                }
                SyncTrigger::NetworkAvailable => self.online = true,
                SyncTrigger::Shutdown => {
                    self.set_status(SyncStatus::Shutdown);
                    return false;
                }
                SyncTrigger::LocalMutation | SyncTrigger::Manual | SyncTrigger::PushHint => {}
            }
        }
        if self.online {
            self.synchronize().await;
        }
        true
    }

    async fn synchronize(&mut self) {
        self.set_status(SyncStatus::Synchronizing);
        let status = match self.engine.sync().await {
            Ok(summary) if summary.conflicts > 0 => {
                self.last_successful_sync_unix_ms = Some(unix_time_ms());
                SyncStatus::Conflict { summary }
            }
            Ok(summary) => {
                self.last_successful_sync_unix_ms = Some(unix_time_ms());
                SyncStatus::Idle {
                    last_sync: Some(summary),
                }
            }
            Err(error) => SyncStatus::Error {
                transient: error.is_transient(),
            },
        };
        self.set_status(status);
        self.refresh_health(status).await;
        self.engine.request_work_class(WorkClass::Normal);
    }

    fn set_status(&self, status: SyncStatus) {
        let _previous = self.status.send_replace(status);
        let mut health = *self.health.borrow();
        health.status = status;
        let _previous_health = self.health.send_replace(health);
    }

    async fn refresh_health(&self, current_status: SyncStatus) {
        let queue_stats = match self.engine.store().outbox_stats().await {
            Ok(queue_stats) => queue_stats,
            Err(error) => {
                self.set_status(SyncStatus::Error {
                    transient: error.kind == StoreErrorKind::Transient,
                });
                return;
            }
        };
        let conflicts_pending = match self.engine.store().unresolved_conflict_count().await {
            Ok(count) => count,
            Err(error) => {
                self.set_status(SyncStatus::Error {
                    transient: error.kind == StoreErrorKind::Transient,
                });
                return;
            }
        };
        let now = unix_time_ms();
        let oldest_pending_age_ms = queue_stats.oldest_pending_at.and_then(|timestamp| {
            u64::try_from(timestamp.physical_ms)
                .ok()
                .map(|created| now.saturating_sub(created))
        });
        let health = SyncHealth {
            status: current_status,
            pending_operations: queue_stats.replayable(),
            oldest_pending_age_ms,
            conflicts_pending,
            last_successful_sync_unix_ms: self.last_successful_sync_unix_ms,
        };
        let _previous = self.health.send_replace(health);
        self.engine.observer.record(MetricEvent::ClientState {
            outbox_pending: usize_to_u64(health.pending_operations),
            oldest_pending_age_ms,
            last_success_unix_ms: health.last_successful_sync_unix_ms,
            conflicts_pending: usize_to_u64(conflicts_pending),
        });
    }
}

/// Client synchronization failure.
#[derive(Debug, Error)]
pub enum ClientError {
    /// Another public synchronization or bootstrap call already owns this engine.
    #[error("a synchronization call is already running on this client engine")]
    SyncInProgress,
    /// Multi-process mode was requested from a single-process-only adapter.
    #[error("local adapter does not support multi-process coordination")]
    CoordinationUnsupported,
    /// Lease TTL and heartbeat timing cannot safely sustain leadership.
    #[error("local coordination lease TTL must exceed twice the positive heartbeat interval")]
    CoordinationConfig,
    /// The selected process mode cannot participate in durable leader election.
    #[error("selected local process mode cannot run the multi-process coordinator")]
    CoordinationMode,
    /// This process does not own the current durable coordinator lease.
    #[error("leader-only local synchronization requires the current durable lease")]
    NotLocalLeader,
    /// Durable work remains queued until one explicit scheduler constraint clears.
    #[error("sync work deferred by scheduler: {reason:?}")]
    SchedulerDeferred {
        /// Stable local-only deferral reason.
        reason: DeferralReason,
    },
    /// Durable work remains queued until one local resource constraint clears.
    #[error("client work deferred by resource policy: {reason:?}")]
    ResourceDeferred { reason: ResourceReason },
    /// Explicit user approval is needed for the requested data/resource override.
    #[error("client work requires explicit resource override approval: {reason:?}")]
    ResourceApprovalRequired { reason: ResourceReason },
    /// A hard storage/background limit makes this unit unsafe to start.
    #[error("client resource is unavailable for safe durable work: {reason:?}")]
    ResourceUnavailable { reason: ResourceReason },
    /// Client resource limits are internally inconsistent.
    #[error(transparent)]
    ResourcePolicy(#[from] resources::PolicyError),
    /// Local persistence failed.
    #[error("local sync storage failed: {0}")]
    Store(#[from] StoreError),
    /// The transport exchange failed.
    #[error("sync exchange failed: {0}")]
    Transport(#[from] TransportError),
    /// Server returned a protocol version this engine cannot reconcile.
    #[error("server returned an incompatible protocol version")]
    Protocol,
    /// The configured client protocol falls outside the server compatibility window.
    #[error("client upgrade required; server accepts protocol {minimum:?} through {current:?}")]
    UpgradeRequired {
        /// Oldest server-supported protocol.
        minimum: ProtocolVersion,
        /// Current server protocol.
        current: ProtocolVersion,
    },
    /// Incremental progress cannot continue and a bootstrap snapshot is required.
    #[error("incremental synchronization requires bootstrap: {reason:?}")]
    ResyncRequired {
        /// Stable recovery reason supplied by the server.
        reason: ResyncReason,
    },
    /// The server moved to a newer authority timeline; pending sent work is frozen for review.
    #[error(
        "authority epoch changed from {previous_epoch:?} to {current_epoch:?}; bootstrap and operation recovery are required"
    )]
    AuthorityChanged {
        authority_id: aequora_types::AuthorityId,
        previous_epoch: aequora_types::AuthorityEpoch,
        current_epoch: aequora_types::AuthorityEpoch,
    },
    /// A server presented an epoch below the highest durable client cursor.
    #[error("authority rollback detected")]
    AuthorityRollbackDetected,
    /// A server endpoint presented a different logical authority identity.
    #[error("server authority identity does not match the durable client cursor")]
    AuthorityIdChanged,
    /// Server returned a cursor for another scope.
    #[error("server returned a cursor for another sync scope")]
    CursorScope,
    /// Server cursor moved behind the client's durable cursor.
    #[error("server cursor moved backward")]
    CursorRegression,
    /// A returned change escaped the authenticated tenant or requested scope.
    #[error("server returned a change outside the authenticated tenant or sync scope")]
    ChangeBoundary,
    /// Returned journal changes were not strictly increasing or exceeded the response cursor.
    #[error("server returned invalid journal sequence ordering")]
    ChangeSequence,
    /// Server operation results were missing, duplicated, or referred to an unsubmitted command.
    #[error("server returned invalid terminal operation results")]
    OperationResults,
    /// Server exceeded the response limits advertised by this client.
    #[error("server response exceeded advertised client limits")]
    ResponseLimits,
    /// A locally constructed request could not be encoded with the production wire codec.
    #[error("outgoing synchronization request could not be encoded: {0}")]
    Codec(#[from] aequora_codec::CodecError),
    /// Even one pending operation cannot fit inside the configured outgoing frame limit.
    #[error("outgoing synchronization frame is {actual} bytes, exceeding limit {maximum}")]
    PushBatchTooLarge { actual: usize, maximum: usize },
    /// Server reported another page without returning or acknowledging any progress.
    #[error("sync exchange reported more data but made no progress")]
    NoProgress,
    /// A full synchronization exceeded its configured exchange safety bound.
    #[error("sync exceeded the maximum number of exchanges")]
    ExchangeLimit,
    /// Bootstrap response did not match the requested snapshot, offset, or scope.
    #[error("server returned inconsistent bootstrap snapshot metadata")]
    SnapshotMismatch,
    /// A snapshot stream ended before delivering its declared final page.
    #[error("snapshot stream ended before the final page")]
    SnapshotStreamEnded,
    /// A push hint used an incompatible protocol version.
    #[error("push hint uses an incompatible protocol version")]
    PushHintProtocol,
    /// A push hint escaped the configured tenant or scope boundary.
    #[error("push hint is outside the configured tenant or sync scope")]
    PushHintBoundary,
}

impl ClientError {
    /// Returns true only when an unchanged operation is safe and useful to retry.
    #[must_use]
    pub const fn is_transient(&self) -> bool {
        matches!(self, Self::SyncInProgress)
            || matches!(self, Self::SchedulerDeferred { .. })
            || matches!(self, Self::ResourceDeferred { .. })
            || matches!(self, Self::ResourceUnavailable { .. })
            || matches!(
                self,
                Self::Store(error) if matches!(error.kind, StoreErrorKind::Transient)
            )
            || matches!(
                self,
                Self::Transport(error) if matches!(error.kind, TransportErrorKind::Transient)
            )
    }
}

/// Transport- and database-independent client engine.
pub struct ClientSyncEngine<L, T> {
    store: L,
    transport: T,
    config: ClientConfig,
    batcher: Mutex<AdaptiveBatcher>,
    reservation: ReservationCore<AtomicBool>,
    observer: Arc<dyn Observer>,
    queue_optimization: Option<QueueOptimization>,
    coordination_lease: Mutex<Option<LeaseGrant>>,
    process_mode: LocalProcessMode,
    coordination_required: AtomicBool,
    scheduler: Mutex<AdaptiveScheduler>,
    scheduling_context: Mutex<SchedulingContext>,
    resource_context: Mutex<ClientResourceContext>,
    requested_work_class: Mutex<WorkClass>,
}

#[derive(Clone)]
struct QueueOptimization {
    registry: Arc<OptimizationRegistry>,
    max_operations_per_pass: usize,
}

/// Marker used only before a client builder receives its local store.
#[doc(hidden)]
#[derive(Clone, Copy, Debug, Default)]
pub struct MissingClientStore;

/// Marker used only before a client builder receives its transport.
#[doc(hidden)]
#[derive(Clone, Copy, Debug, Default)]
pub struct MissingClientTransport;

/// Fluent, type-safe client assembly. Store and transport presence is enforced by the type
/// system; the session-bearing [`ClientConfig`] is checked when building.
pub struct ClientSyncEngineBuilder<L = MissingClientStore, T = MissingClientTransport> {
    store: L,
    transport: T,
    config: Option<ClientConfig>,
    observer: Arc<dyn Observer>,
    queue_optimization: Option<QueueOptimization>,
    process_mode: LocalProcessMode,
    scheduler_state: Option<SchedulerState>,
}

impl Default for ClientSyncEngineBuilder {
    fn default() -> Self {
        Self {
            store: MissingClientStore,
            transport: MissingClientTransport,
            config: None,
            observer: Arc::new(NoopObserver),
            queue_optimization: None,
            process_mode: LocalProcessMode::Auto,
            scheduler_state: None,
        }
    }
}

impl ClientSyncEngineBuilder {
    /// Starts an empty type-state builder.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }
}

impl<L, T> ClientSyncEngineBuilder<L, T> {
    /// Installs or replaces the capability-based local store.
    #[must_use]
    pub fn store<L2>(self, store: L2) -> ClientSyncEngineBuilder<L2, T> {
        ClientSyncEngineBuilder {
            store,
            transport: self.transport,
            config: self.config,
            observer: self.observer,
            queue_optimization: self.queue_optimization,
            process_mode: self.process_mode,
            scheduler_state: self.scheduler_state,
        }
    }

    /// Installs or replaces the synchronization transport.
    #[must_use]
    pub fn transport<T2>(self, transport: T2) -> ClientSyncEngineBuilder<L, T2> {
        ClientSyncEngineBuilder {
            store: self.store,
            transport,
            config: self.config,
            observer: self.observer,
            queue_optimization: self.queue_optimization,
            process_mode: self.process_mode,
            scheduler_state: self.scheduler_state,
        }
    }

    /// Installs the session identity, bounds, and retry policy.
    #[must_use]
    pub fn config(mut self, config: ClientConfig) -> Self {
        self.config = Some(config);
        self
    }

    /// Installs a non-blocking, payload-free observer.
    #[must_use]
    pub fn observer(mut self, observer: Arc<dyn Observer>) -> Self {
        self.observer = observer;
        self
    }

    /// Selects how this runtime may share its local store with other processes.
    #[must_use]
    pub fn process_mode(mut self, process_mode: LocalProcessMode) -> Self {
        self.process_mode = process_mode;
        self
    }

    /// Restores bounded persisted controller/backoff state; durable work remains in its stores.
    #[must_use]
    pub fn scheduler_state(mut self, scheduler_state: SchedulerState) -> Self {
        self.scheduler_state = Some(scheduler_state);
        self
    }

    /// Enables one bounded deterministic compaction pass before each upload batch is built.
    #[must_use]
    pub fn queue_optimization(
        mut self,
        registry: Arc<OptimizationRegistry>,
        max_operations_per_pass: usize,
    ) -> Self {
        self.queue_optimization = Some(QueueOptimization {
            registry,
            max_operations_per_pass: max_operations_per_pass.max(1),
        });
        self
    }
}

/// Client builder validation failure.
#[derive(Clone, Copy, Debug, Error, Eq, PartialEq)]
pub enum ClientBuildError {
    /// A session-bearing client configuration was not supplied.
    #[error("client configuration is required")]
    MissingConfig,
    /// Selected local adapter does not meet production role/tier/capability requirements.
    #[error(transparent)]
    Adapter(#[from] AdapterCompatibilityError),
    /// Multi-process mode requires a manifest with durable local coordination.
    #[error("multi-process mode requires adapter local-coordination capability")]
    CoordinationUnsupported,
    /// Scheduler limits are internally inconsistent.
    #[error(transparent)]
    Scheduler(#[from] aequora_scheduler::SchedulerError),
    /// Client resource limits are internally inconsistent.
    #[error(transparent)]
    Resources(#[from] resources::PolicyError),
}

impl<L, T> ClientSyncEngineBuilder<L, T>
where
    L: LocalStore,
    T: SyncTransport,
{
    /// Builds the client once every required capability is installed.
    ///
    /// # Errors
    ///
    /// Returns [`ClientBuildError::MissingConfig`] when no session configuration was supplied.
    pub fn build(self) -> Result<ClientSyncEngine<L, T>, ClientBuildError> {
        let config = self.config.ok_or(ClientBuildError::MissingConfig)?;
        config.scheduler.validate()?;
        config.resources.validate()?;
        let scheduler_policy = config.scheduler;
        let mut engine =
            ClientSyncEngine::new(self.store, self.transport, config).with_observer(self.observer);
        engine.queue_optimization = self.queue_optimization;
        engine.process_mode = self.process_mode;
        engine.coordination_required.store(
            self.process_mode == LocalProcessMode::MultiProcess,
            Ordering::Release,
        );
        if let Some(mut state) = self.scheduler_state {
            if state.policy_version != scheduler_policy.version {
                return Err(aequora_scheduler::SchedulerError::PolicyVersionMismatch.into());
            }
            state.normalize(unix_time_ms(), scheduler_policy.maximum_retry_deferral_ms);
            engine.scheduler = Mutex::new(AdaptiveScheduler::from_state(scheduler_policy, state));
        }
        Ok(engine)
    }
}

impl<L, T> ClientSyncEngineBuilder<L, T>
where
    L: LocalStore + AdapterManifestProvider,
    T: SyncTransport,
{
    /// Verifies the local adapter's production manifest before building the client.
    ///
    /// Reference/test stores should continue to use [`Self::build`]. Production bootstrap should
    /// use this method so unsupported durability or role combinations fail before synchronization.
    ///
    /// # Errors
    ///
    /// Returns a missing-config or typed adapter compatibility failure.
    pub fn build_production(self) -> Result<ClientSyncEngine<L, T>, ClientBuildError> {
        let manifest = self.store.adapter_manifest();
        if self.process_mode == LocalProcessMode::MultiProcess
            && !manifest
                .capabilities
                .contains(aequora_store::AdapterCapabilities::LOCAL_COORDINATION)
        {
            return Err(ClientBuildError::CoordinationUnsupported);
        }
        AdapterRequirements::PRODUCTION_LOCAL.verify(manifest)?;
        self.build()
    }
}

impl<L, T> ClientSyncEngine<L, T> {
    /// Creates a client sync engine.
    #[must_use]
    pub fn new(store: L, transport: T, config: ClientConfig) -> Self {
        let batcher = Mutex::new(AdaptiveBatcher::new(
            config.push_batch_size,
            config.adaptive_batching,
        ));
        let scheduler = AdaptiveScheduler::new_or_default(config.scheduler);
        Self {
            store,
            transport,
            config,
            batcher,
            reservation: ReservationCore::new(AtomicBool::new(false)),
            observer: Arc::new(NoopObserver),
            queue_optimization: None,
            coordination_lease: Mutex::new(None),
            process_mode: LocalProcessMode::Auto,
            coordination_required: AtomicBool::new(false),
            scheduler: Mutex::new(scheduler),
            scheduling_context: Mutex::new(SchedulingContext::default()),
            resource_context: Mutex::new(ClientResourceContext::default()),
            requested_work_class: Mutex::new(WorkClass::Normal),
        }
    }

    /// Installs a non-blocking payload-free metrics and tracing observer.
    #[must_use]
    pub fn with_observer(mut self, observer: Arc<dyn Observer>) -> Self {
        self.observer = observer;
        self
    }

    /// Returns the local store for application integration or inspection.
    #[must_use]
    pub const fn store(&self) -> &L {
        &self.store
    }

    fn current_lease(&self) -> Option<LeaseGrant> {
        *self
            .coordination_lease
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    fn install_lease(&self, lease: LeaseGrant) {
        *self
            .coordination_lease
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(lease);
    }

    fn clear_lease(&self) {
        *self
            .coordination_lease
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = None;
    }

    fn require_local_leadership(&self) -> Result<(), ClientError> {
        if self.coordination_required.load(Ordering::Acquire) && self.current_lease().is_none() {
            return Err(ClientError::NotLocalLeader);
        }
        Ok(())
    }

    /// Pauses network scheduling while preserving local writes and durable work.
    pub fn pause(&self) {
        self.scheduling_context().paused = true;
    }

    /// Clears an explicit pause. Other eligibility constraints still apply.
    pub fn resume(&self) {
        self.scheduling_context().paused = false;
    }

    /// Replaces normalized platform hints used by subsequent scheduler decisions.
    pub fn update_scheduling_context(&self, context: SchedulingContext) {
        *self.scheduling_context() = context.normalized();
    }

    /// Replaces normalized platform resource signals used by subsequent admission decisions.
    pub fn update_resource_context(&self, context: ClientResourceContext) {
        *self.resource_context() = context.normalized();
    }

    /// Requests a bounded `QoS` class for the next synchronization drain.
    ///
    /// This is local scheduling metadata only and cannot bypass authorization or dependencies.
    pub fn request_work_class(&self, class: WorkClass) {
        *self.requested_work_class() = class;
    }

    /// Returns persistable bounded controller/backoff state, excluding durable work payloads.
    #[must_use]
    pub fn scheduler_state(&self) -> SchedulerState {
        self.scheduler().state()
    }

    fn scheduling_context(&self) -> MutexGuard<'_, SchedulingContext> {
        self.scheduling_context
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    fn scheduler(&self) -> MutexGuard<'_, AdaptiveScheduler> {
        self.scheduler
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    fn resource_context(&self) -> MutexGuard<'_, ClientResourceContext> {
        self.resource_context
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    fn requested_work_class(&self) -> MutexGuard<'_, WorkClass> {
        self.requested_work_class
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    fn scheduler_decision(
        &self,
        kind: WorkKind,
        class: WorkClass,
    ) -> Result<aequora_scheduler::SchedulingDecision, ClientError> {
        let mut context = *self.scheduling_context();
        context.now_unix_ms = unix_time_ms();
        context.coordinator_leader =
            !self.coordination_required.load(Ordering::Acquire) || self.current_lease().is_some();
        let work = WorkDescriptor {
            work_id: WorkId(0),
            kind,
            class,
            enqueued_at_unix_ms: context.now_unix_ms,
            earliest_at_unix_ms: context.now_unix_ms,
            deadline_unix_ms: None,
            dependencies: Vec::new(),
            estimated_operations: self.current_batch_limit(),
            estimated_bytes: self.config.push_batch_bytes,
            emergency_override: false,
        };
        let resource_work = ClientWork {
            kind: client_work_kind(kind, class),
            estimate: WorkEstimate {
                operations: u32::try_from(work.estimated_operations).unwrap_or(u32::MAX),
                bytes: u64::try_from(work.estimated_bytes).unwrap_or(u64::MAX),
                storage_delta_bytes: if matches!(
                    kind,
                    WorkKind::Bootstrap | WorkKind::LargeBootstrapTransfer
                ) {
                    u64::try_from(work.estimated_bytes).unwrap_or(u64::MAX)
                } else {
                    0
                },
                cpu: resources::CpuClass::Moderate,
            },
            user_initiated: class == WorkClass::Interactive,
        };
        let admission = DefaultResourceAdmission::new(self.config.resources)?;
        let resource_limits = match admission.allow(&resource_work, &self.resource_context()) {
            ResourceDecision::RunNow(limits) | ResourceDecision::RunReduced(limits) => limits,
            ResourceDecision::Defer(reason) => {
                return Err(ClientError::ResourceDeferred { reason });
            }
            ResourceDecision::RequireUserApproval(reason) => {
                return Err(ClientError::ResourceApprovalRequired { reason });
            }
            ResourceDecision::RejectUntilResourceAvailable(reason) => {
                return Err(ClientError::ResourceUnavailable { reason });
            }
        };
        let selected = self
            .scheduler()
            .select(&[work], &std::collections::BTreeSet::new(), &context)
            .map(|selected| resource_limits.constrain_scheduler(selected.decision));
        if let Some(decision) = selected {
            self.observer.record(MetricEvent::Scheduler {
                kind: SchedulerEventKind::Selected,
                class_rank: decision.priority.0,
                operations: usize_to_u64(decision.max_batch_ops),
                bytes: usize_to_u64(decision.max_batch_bytes),
            });
            return Ok(decision);
        }
        let error = {
            let reason = if context.paused {
                DeferralReason::Paused
            } else if !context.network.online {
                DeferralReason::Offline
            } else if !context.coordinator_leader {
                DeferralReason::NotLeader
            } else if !context.auth_available {
                DeferralReason::AuthenticationRequired
            } else if !context.storage_healthy {
                DeferralReason::StorageUnhealthy
            } else {
                DeferralReason::RetryBackoff
            };
            ClientError::SchedulerDeferred { reason }
        };
        self.observer.record(MetricEvent::Scheduler {
            kind: SchedulerEventKind::Deferred,
            class_rank: class.rank(),
            operations: 0,
            bytes: 0,
        });
        Err(error)
    }

    /// Current push batch limit after any adaptive observations.
    #[must_use]
    pub fn current_batch_limit(&self) -> usize {
        self.batcher().limit()
    }

    fn batcher(&self) -> MutexGuard<'_, AdaptiveBatcher> {
        self.batcher
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    fn record_retry(&self, delay: Duration) {
        self.observer.record(MetricEvent::ClientRetry {
            delay_millis: u64::try_from(delay.as_millis()).unwrap_or(u64::MAX),
        });
    }

    fn reserve(&self) -> Result<ReservationGuard<'_, AtomicBool>, ClientError> {
        self.reservation
            .try_acquire()
            .ok_or(ClientError::SyncInProgress)
    }
}

impl<L, T> ClientSyncEngine<L, T>
where
    L: LocalStore + ScopeStateStore,
{
    /// Durably installs one server-resolved subscription without activating unbootstrapped data.
    ///
    /// # Errors
    ///
    /// Returns a leadership or storage failure without changing subscription state.
    pub async fn install_subscription(
        &self,
        subscription: &Subscription,
    ) -> Result<(), ClientError> {
        self.require_local_leadership()?;
        self.store.install_subscription(subscription).await?;
        let active_scopes = self
            .store
            .load_scope_state()
            .await?
            .active_subscription_count();
        self.observer.record(MetricEvent::ScopeTransition {
            kind: ScopeEventKind::Resolved,
            entities: 0,
            active_scopes: usize_to_u64(active_scopes),
        });
        Ok(())
    }

    /// Applies one idempotent, fenced scope transition through the local adapter.
    ///
    /// Expansion/bootstrap activation is scheduler-bounded bulk work. Contraction and revocation
    /// are critical because they close a local authorization boundary.
    ///
    /// # Errors
    ///
    /// Returns a scheduler, leadership, or atomic storage error; partial activation is forbidden.
    pub async fn apply_scope_transition(
        &self,
        transition: &ScopeTransition,
    ) -> Result<ScopeTransitionOutcome, ClientError> {
        self.require_local_leadership()?;
        let (class, kind) = match transition.kind {
            ScopeTransitionKind::Contraction { .. } => {
                (WorkClass::Critical, ScopeEventKind::Contracted)
            }
            ScopeTransitionKind::Revocation => (WorkClass::Critical, ScopeEventKind::Revoked),
            ScopeTransitionKind::Expansion { .. }
            | ScopeTransitionKind::FullBootstrap
            | ScopeTransitionKind::GenerationReset => (WorkClass::Bulk, ScopeEventKind::Expanded),
            ScopeTransitionKind::Suspension => (WorkClass::Critical, ScopeEventKind::Contracted),
        };
        let _decision = self.scheduler_decision(WorkKind::ScopeTransition, class)?;
        let outcome = self
            .store
            .apply_scope_transition_fenced(transition, self.current_lease())
            .await?;
        let active_scopes = self
            .store
            .load_scope_state()
            .await?
            .active_subscription_count();
        self.observer.record(MetricEvent::ScopeTransition {
            kind,
            entities: usize_to_u64(
                transition
                    .additions
                    .len()
                    .saturating_add(transition.removals.len()),
            ),
            active_scopes: usize_to_u64(active_scopes),
        });
        Ok(outcome)
    }
}

fn client_work_kind(kind: WorkKind, class: WorkClass) -> ClientWorkKind {
    if class == WorkClass::Critical {
        return match kind {
            WorkKind::Repair => ClientWorkKind::SmallRepair,
            WorkKind::PushOperations | WorkKind::PullChanges | WorkKind::LiveCatchUp => {
                ClientWorkKind::CriticalSync
            }
            _ => ClientWorkKind::SecurityDirective,
        };
    }
    match kind {
        WorkKind::PushOperations => ClientWorkKind::InteractivePush,
        WorkKind::PullChanges | WorkKind::LiveCatchUp => ClientWorkKind::InteractivePull,
        WorkKind::Bootstrap
        | WorkKind::ScopeTransition
        | WorkKind::BulkMigration
        | WorkKind::LargeBootstrapTransfer => ClientWorkKind::Bootstrap,
        WorkKind::IntegrityCheck => ClientWorkKind::AntiEntropy,
        WorkKind::Repair => ClientWorkKind::SmallRepair,
        WorkKind::QueueCompaction | WorkKind::Maintenance => ClientWorkKind::Maintenance,
        WorkKind::BlobTransfer => ClientWorkKind::BlobTransfer,
        WorkKind::MigrationCutover | WorkKind::BootstrapActivation => {
            ClientWorkKind::SecurityDirective
        }
    }
}

impl<L, T> ClientSyncEngine<L, T>
where
    L: LocalStore,
    T: SyncTransport,
{
    /// Performs one push/pull exchange and atomically reconciles its response.
    ///
    /// # Errors
    ///
    /// Returns [`ClientError`] for local storage or transport failures and for any response
    /// that violates protocol, tenant, scope, cursor, or sequence invariants.
    pub async fn run_once(&self) -> Result<SyncOutcome, ClientError> {
        let _reservation = self.reserve()?;
        self.require_local_leadership()?;
        let retry_delay = self.config.retry.delay(0, system_entropy());
        self.run_once_with_retry_delay(retry_delay).await
    }

    async fn compact_outbox_if_configured(&self) -> Result<(), ClientError> {
        let Some(optimization) = &self.queue_optimization else {
            return Ok(());
        };
        let started = Instant::now();
        let result = self
            .store
            .compact_outbox_fenced(
                optimization.registry.as_ref(),
                optimization.max_operations_per_pass,
                self.current_lease(),
            )
            .await;
        match result {
            Ok(plan) => self.observer.record(MetricEvent::QueueCompaction {
                duration_micros: duration_micros(started.elapsed()),
                operations_before: usize_to_u64(plan.operations_before),
                operations_after: usize_to_u64(plan.operations_after),
                bytes_saved: plan.bytes_saved,
                failed: false,
            }),
            Err(error) => {
                self.observer.record(MetricEvent::QueueCompaction {
                    duration_micros: duration_micros(started.elapsed()),
                    operations_before: 0,
                    operations_after: 0,
                    bytes_saved: 0,
                    failed: true,
                });
                return Err(error.into());
            }
        }
        Ok(())
    }

    #[allow(clippy::too_many_lines)]
    async fn run_once_with_retry_delay(
        &self,
        retry_delay: Duration,
    ) -> Result<SyncOutcome, ClientError> {
        let scheduling =
            self.scheduler_decision(WorkKind::PushOperations, *self.requested_work_class())?;
        self.compact_outbox_if_configured().await?;
        let cursor = self.store.load_cursor(self.config.session.scope_id).await?;
        let batch_limit = self.current_batch_limit().min(scheduling.max_batch_ops);
        let operations = self.store.pending_operations(batch_limit).await?;
        let request_id = RequestId::new();
        let mut request = SyncRequest {
            protocol: self.config.protocol,
            request_id,
            session: self.config.session.clone(),
            cursor,
            operations,
            limits: self.config.limits,
            capabilities: self.config.capabilities.clone(),
        };
        loop {
            let encoded = aequora_codec::encode(
                self.config.protocol,
                aequora_codec::MessageKind::SyncRequest,
                &request,
            )?;
            let maximum_batch_bytes = self.config.push_batch_bytes.min(scheduling.max_batch_bytes);
            if encoded.len() <= maximum_batch_bytes {
                break;
            }
            if request.operations.len() <= 1 {
                return Err(ClientError::PushBatchTooLarge {
                    actual: encoded.len(),
                    maximum: maximum_batch_bytes,
                });
            }
            let next_len = request.operations.len().div_ceil(2);
            request.operations.truncate(next_len);
        }
        let submitted = request.operations.len();
        let operation_ids: Vec<_> = request
            .operations
            .iter()
            .map(|operation| operation.operation_id)
            .collect();
        self.store
            .mark_sending_fenced(&operation_ids, self.current_lease())
            .await?;
        let trace = trace_context(request.request_id, &request.session);
        let started = Instant::now();
        let response = match self.transport.exchange(request).await {
            Ok(response) => response,
            Err(error) => {
                if error.kind == TransportErrorKind::Transient {
                    self.batcher().record_failure();
                    let retry_after_ms = error
                        .retry_after
                        .map(|delay| u64::try_from(delay.as_millis()).unwrap_or(u64::MAX));
                    self.scheduler().record_overload(
                        unix_time_ms(),
                        aequora_scheduler::ServerSchedulingHints {
                            retry_after_ms,
                            ..aequora_scheduler::ServerSchedulingHints::default()
                        },
                    );
                    self.observer.record(MetricEvent::Scheduler {
                        kind: SchedulerEventKind::OverloadBackoff,
                        class_rank: WorkClass::Normal.rank(),
                        operations: usize_to_u64(submitted),
                        bytes: 0,
                    });
                }
                let durable_retry_delay = error
                    .retry_after
                    .map_or(retry_delay, |server| retry_delay.max(server))
                    .min(self.config.retry.max_delay);
                self.store
                    .mark_retry_fenced(
                        &operation_ids,
                        retry_not_before(durable_retry_delay),
                        self.current_lease(),
                    )
                    .await?;
                self.observer.record_with_context(
                    trace,
                    MetricEvent::ClientExchange {
                        duration_micros: duration_micros(started.elapsed()),
                        operations: usize_to_u64(submitted),
                        changes: 0,
                        conflicts: 0,
                        rejections: 0,
                        outcome: transport_outcome(error.kind),
                    },
                );
                return Err(ClientError::Transport(error));
            }
        };
        let latency = started.elapsed();
        let changes = response.changes.len();
        let conflicts = response.conflicts.len();
        let rejections = response.rejected.len();
        let result = self
            .reconcile_exchange(cursor, response, &operation_ids, latency)
            .await;
        if matches!(&result, Err(ClientError::AuthorityChanged { .. })) {
            self.store
                .mark_retry_fenced(&operation_ids, u64::MAX, self.current_lease())
                .await?;
        } else if result.is_err() {
            self.store
                .mark_retry_fenced(
                    &operation_ids,
                    retry_not_before(retry_delay),
                    self.current_lease(),
                )
                .await?;
        }
        self.observer.record_with_context(
            trace,
            MetricEvent::ClientExchange {
                duration_micros: duration_micros(started.elapsed()),
                operations: usize_to_u64(submitted),
                changes: usize_to_u64(changes),
                conflicts: usize_to_u64(conflicts),
                rejections: usize_to_u64(rejections),
                outcome: result_outcome(&result),
            },
        );
        result
    }

    async fn reconcile_exchange(
        &self,
        cursor: Option<Cursor>,
        response: SyncResponse,
        submitted_operations: &[OperationId],
        latency: Duration,
    ) -> Result<SyncOutcome, ClientError> {
        validate_sync_directive(&response.directive)?;
        if response.protocol != self.config.protocol {
            return Err(ClientError::Protocol);
        }
        let response_bytes = aequora_codec::encode(
            self.config.protocol,
            aequora_codec::MessageKind::SyncResponse,
            &response,
        )?
        .len();
        if response.changes.len()
            > usize::try_from(self.config.limits.max_changes).unwrap_or(usize::MAX)
            || response_bytes
                > usize::try_from(self.config.limits.max_response_bytes).unwrap_or(usize::MAX)
        {
            return Err(ClientError::ResponseLimits);
        }
        if response.next_cursor.scope != self.config.session.scope_id {
            return Err(ClientError::CursorScope);
        }
        if let Some(old) = cursor {
            if old.authority_id != aequora_types::AuthorityId::LEGACY_UNBOUND
                && response.next_cursor.authority_id != old.authority_id
            {
                return Err(ClientError::AuthorityIdChanged);
            }
            if response.next_cursor.authority_epoch < old.authority_epoch {
                return Err(ClientError::AuthorityRollbackDetected);
            }
            if response.next_cursor.authority_epoch > old.authority_epoch {
                return Err(ClientError::AuthorityChanged {
                    authority_id: response.next_cursor.authority_id,
                    previous_epoch: old.authority_epoch,
                    current_epoch: response.next_cursor.authority_epoch,
                });
            }
        }
        if cursor.is_some_and(|old| response.next_cursor.sequence < old.sequence) {
            return Err(ClientError::CursorRegression);
        }
        validate_operation_results(&response, submitted_operations)?;
        let mut previous = cursor.map_or(aequora_types::Sequence(0), |old| old.sequence);
        for change in &response.changes {
            if change.tenant_id != self.config.session.tenant_id
                || change.scope_id != self.config.session.scope_id
            {
                return Err(ClientError::ChangeBoundary);
            }
            if change.sequence <= previous || change.sequence > response.next_cursor.sequence {
                return Err(ClientError::ChangeSequence);
            }
            previous = change.sequence;
        }
        if response.next_cursor.sequence != previous {
            return Err(ClientError::ChangeSequence);
        }
        let outcome = SyncOutcome {
            acknowledged: response.acknowledged.len(),
            rejected: response.rejected.len(),
            conflicts: response.conflicts.len(),
            changes: response.changes.len(),
            has_more: response.has_more,
        };
        if outcome.has_more
            && outcome.acknowledged == 0
            && outcome.rejected == 0
            && outcome.conflicts == 0
            && outcome.changes == 0
            && cursor.map_or(aequora_types::Sequence(0), |old| old.sequence)
                == response.next_cursor.sequence
        {
            return Err(ClientError::NoProgress);
        }
        self.store
            .reconcile_fenced(&response, self.current_lease())
            .await?;
        self.batcher().record_success(
            latency,
            submitted_operations.len(),
            outcome
                .acknowledged
                .saturating_add(outcome.rejected)
                .saturating_add(outcome.conflicts),
        );
        self.scheduler().record_success();
        Ok(outcome)
    }

    /// Performs one exchange with bounded retries for typed transient failures.
    ///
    /// # Errors
    ///
    /// Returns the final [`ClientError`] after a permanent failure or retry exhaustion.
    pub async fn run_with_retry(&self) -> Result<SyncOutcome, ClientError> {
        let _reservation = self.reserve()?;
        self.require_local_leadership()?;
        self.run_with_retry_unreserved().await
    }

    async fn run_with_retry_unreserved(&self) -> Result<SyncOutcome, ClientError> {
        let max_attempts = self.config.retry.max_attempts.max(1);
        let mut attempt = 0_u32;
        loop {
            let entropy = system_entropy() ^ u64::from(attempt);
            let local_delay = self.config.retry.delay(attempt, entropy);
            match self.run_once_with_retry_delay(local_delay).await {
                Ok(outcome) => return Ok(outcome),
                Err(error) if error.is_transient() && attempt + 1 < max_attempts => {
                    let server_retry_after = match &error {
                        ClientError::Transport(error) => error.retry_after,
                        _ => None,
                    };
                    let delay =
                        self.config
                            .retry
                            .delay_with_server(attempt, entropy, server_retry_after);
                    self.record_retry(delay);
                    tokio::time::sleep(delay).await;
                    attempt = attempt.saturating_add(1);
                }
                Err(error) => return Err(error),
            }
        }
    }

    /// Drains all current outbox batches and server pull pages, retrying transient failures.
    ///
    /// # Errors
    ///
    /// Returns [`ClientError`] when an exchange fails permanently, retries are exhausted,
    /// local state cannot be read, or the configured exchange safety bound is reached.
    pub async fn sync(&self) -> Result<SyncSummary, ClientError> {
        let _reservation = self.reserve()?;
        self.require_local_leadership()?;
        let mut summary = SyncSummary::default();
        if self
            .store
            .load_cursor(self.config.session.scope_id)
            .await?
            .is_none()
        {
            self.bootstrap_unreserved().await?;
        }
        for _ in 0..self.config.max_exchanges_per_sync {
            let outcome = match self.run_with_retry_unreserved().await {
                Ok(outcome) => outcome,
                Err(ClientError::ResyncRequired { .. }) => {
                    self.bootstrap_unreserved().await?;
                    continue;
                }
                Err(error) => return Err(error),
            };
            summary.record(outcome);
            let pending = self.store.pending_operations(1).await?;
            if !outcome.has_more && pending.is_empty() {
                return Ok(summary);
            }
        }
        Err(ClientError::ExchangeLimit)
    }

    /// Begins or resumes a consistent snapshot and atomically installs it on the final page.
    ///
    /// # Errors
    ///
    /// Returns [`ClientError`] for authorization/transport/storage failures, inconsistent
    /// snapshot pages, retry exhaustion, no-progress pages, or the exchange safety bound.
    pub async fn bootstrap(&self) -> Result<BootstrapOutcome, ClientError> {
        let _reservation = self.reserve()?;
        self.require_local_leadership()?;
        let _decision = self.scheduler_decision(WorkKind::Bootstrap, WorkClass::Interactive)?;
        self.bootstrap_unreserved().await
    }

    async fn bootstrap_unreserved(&self) -> Result<BootstrapOutcome, ClientError> {
        let trusted_cursor = self.store.load_cursor(self.config.session.scope_id).await?;
        let progress = self
            .store
            .snapshot_progress(self.config.session.scope_id)
            .await?;
        let mut snapshot_id = progress.map(|value| value.snapshot_id);
        let mut offset = progress.map_or(0, |value| value.next_offset);
        let mut cursor = progress.map(|value| value.cursor);
        let mut pages = 0_usize;
        let mut entities = 0_usize;
        for _ in 0..self.config.max_exchanges_per_sync {
            let request = BootstrapRequest {
                protocol: self.config.protocol,
                request_id: RequestId::new(),
                session: self.config.session.clone(),
                snapshot_id,
                offset,
                limits: self.config.snapshot_limits,
                capabilities: self.config.capabilities.clone(),
            };
            let response = self.bootstrap_page_with_retry(request).await?;
            if let Some(trusted) = trusted_cursor {
                if trusted.authority_id != aequora_types::AuthorityId::LEGACY_UNBOUND
                    && response.cursor.authority_id != trusted.authority_id
                {
                    return Err(ClientError::AuthorityIdChanged);
                }
                if response.cursor.authority_epoch < trusted.authority_epoch {
                    return Err(ClientError::AuthorityRollbackDetected);
                }
            }
            validate_snapshot_page(
                &response,
                self.config.protocol,
                snapshot_id,
                cursor,
                offset,
                &self.config.session,
                self.config.snapshot_limits,
            )?;
            if response.has_more
                && response.entities.is_empty()
                && response.next_offset == response.offset
            {
                return Err(ClientError::NoProgress);
            }
            self.store
                .stage_snapshot_fenced(&response, self.current_lease())
                .await?;
            pages = pages.saturating_add(1);
            entities = entities.saturating_add(response.entities.len());
            snapshot_id = Some(response.snapshot_id);
            cursor = Some(response.cursor);
            offset = response.next_offset;
            if !response.has_more {
                return Ok(BootstrapOutcome {
                    snapshot_id: response.snapshot_id,
                    pages,
                    entities,
                    cursor: response.cursor,
                });
            }
        }
        Err(ClientError::ExchangeLimit)
    }

    /// Waits for and validates one advisory server hint. The caller still performs a normal
    /// authenticated synchronization exchange; hints never contain authoritative state.
    ///
    /// # Errors
    ///
    /// Returns [`ClientError`] when the transport fails or the hint crosses protocol,
    /// tenant, or scope boundaries.
    pub async fn wait_for_push_hint(&self) -> Result<PushHint, ClientError> {
        let hint = self.transport.next_push_hint().await?;
        if hint.protocol != self.config.protocol {
            return Err(ClientError::PushHintProtocol);
        }
        if hint.tenant_id != self.config.session.tenant_id
            || hint.scope_id != self.config.session.scope_id
        {
            return Err(ClientError::PushHintBoundary);
        }
        Ok(hint)
    }

    async fn bootstrap_page_with_retry(
        &self,
        request: BootstrapRequest,
    ) -> Result<BootstrapResponse, ClientError> {
        let trace = trace_context(request.request_id, &request.session);
        let max_attempts = self.config.retry.max_attempts.max(1);
        let mut attempt = 0_u32;
        loop {
            let started = Instant::now();
            match self.transport.bootstrap(request.clone()).await {
                Ok(response) => {
                    self.observer.record_with_context(
                        trace,
                        MetricEvent::BootstrapPage {
                            duration_micros: duration_micros(started.elapsed()),
                            entities: usize_to_u64(response.entities.len()),
                            outcome: OutcomeKind::Success,
                        },
                    );
                    return Ok(response);
                }
                Err(error)
                    if error.kind == TransportErrorKind::Transient
                        && attempt + 1 < max_attempts =>
                {
                    self.observer.record_with_context(
                        trace,
                        MetricEvent::BootstrapPage {
                            duration_micros: duration_micros(started.elapsed()),
                            entities: 0,
                            outcome: OutcomeKind::TransientFailure,
                        },
                    );
                    let entropy = system_entropy() ^ u64::from(attempt);
                    let delay =
                        self.config
                            .retry
                            .delay_with_server(attempt, entropy, error.retry_after);
                    self.record_retry(delay);
                    tokio::time::sleep(delay).await;
                    attempt = attempt.saturating_add(1);
                }
                Err(error) => {
                    self.observer.record_with_context(
                        trace,
                        MetricEvent::BootstrapPage {
                            duration_micros: duration_micros(started.elapsed()),
                            entities: 0,
                            outcome: transport_outcome(error.kind),
                        },
                    );
                    return Err(ClientError::Transport(error));
                }
            }
        }
    }
}

impl<L, T> ClientSyncEngine<L, T>
where
    L: LocalStore,
    T: StreamingSyncTransport,
{
    /// Installs a resumable snapshot from a transport stream while staging one bounded page
    /// at a time. A transient stream failure reopens from the last durable page offset.
    ///
    /// # Errors
    ///
    /// Returns [`ClientError`] for invalid pages, premature stream completion, exhausted
    /// retries, storage failures, or the configured page safety bound.
    #[allow(clippy::too_many_lines)]
    pub async fn bootstrap_streaming(&self) -> Result<BootstrapOutcome, ClientError> {
        let _reservation = self.reserve()?;
        self.require_local_leadership()?;
        let _decision = self.scheduler_decision(WorkKind::Bootstrap, WorkClass::Interactive)?;
        let progress = self
            .store
            .snapshot_progress(self.config.session.scope_id)
            .await?;
        let mut snapshot_id = progress.map(|value| value.snapshot_id);
        let mut offset = progress.map_or(0, |value| value.next_offset);
        let mut cursor = progress.map(|value| value.cursor);
        let mut pages = 0_usize;
        let mut entities = 0_usize;
        let mut attempt = 0_u32;
        let max_attempts = self.config.retry.max_attempts.max(1);

        while pages < self.config.max_exchanges_per_sync {
            let mut capabilities = self.config.capabilities.clone();
            if !capabilities.contains(&Capability::StreamingSnapshots) {
                capabilities.push(Capability::StreamingSnapshots);
            }
            let request = BootstrapRequest {
                protocol: self.config.protocol,
                request_id: RequestId::new(),
                session: self.config.session.clone(),
                snapshot_id,
                offset,
                limits: self.config.snapshot_limits,
                capabilities,
            };
            let trace = trace_context(request.request_id, &request.session);
            let mut stream = match self.transport.bootstrap_stream(request).await {
                Ok(stream) => stream,
                Err(error)
                    if error.kind == TransportErrorKind::Transient
                        && attempt + 1 < max_attempts =>
                {
                    let entropy = system_entropy();
                    let delay =
                        self.config
                            .retry
                            .delay_with_server(attempt, entropy, error.retry_after);
                    self.record_retry(delay);
                    tokio::time::sleep(delay).await;
                    attempt = attempt.saturating_add(1);
                    continue;
                }
                Err(error) => return Err(ClientError::Transport(error)),
            };

            loop {
                let started = Instant::now();
                let response = match stream.next_page().await {
                    Ok(Some(response)) => response,
                    Ok(None) => return Err(ClientError::SnapshotStreamEnded),
                    Err(error)
                        if error.kind == TransportErrorKind::Transient
                            && attempt + 1 < max_attempts =>
                    {
                        self.observer.record_with_context(
                            trace,
                            MetricEvent::BootstrapPage {
                                duration_micros: duration_micros(started.elapsed()),
                                entities: 0,
                                outcome: OutcomeKind::TransientFailure,
                            },
                        );
                        let entropy = system_entropy();
                        let delay = self.config.retry.delay_with_server(
                            attempt,
                            entropy,
                            error.retry_after,
                        );
                        self.record_retry(delay);
                        tokio::time::sleep(delay).await;
                        attempt = attempt.saturating_add(1);
                        break;
                    }
                    Err(error) => return Err(ClientError::Transport(error)),
                };
                validate_snapshot_page(
                    &response,
                    self.config.protocol,
                    snapshot_id,
                    cursor,
                    offset,
                    &self.config.session,
                    self.config.snapshot_limits,
                )?;
                if response.has_more
                    && response.entities.is_empty()
                    && response.next_offset == response.offset
                {
                    return Err(ClientError::NoProgress);
                }
                self.store
                    .stage_snapshot_fenced(&response, self.current_lease())
                    .await?;
                self.observer.record_with_context(
                    trace,
                    MetricEvent::BootstrapPage {
                        duration_micros: duration_micros(started.elapsed()),
                        entities: usize_to_u64(response.entities.len()),
                        outcome: OutcomeKind::Success,
                    },
                );
                pages = pages.saturating_add(1);
                entities = entities.saturating_add(response.entities.len());
                snapshot_id = Some(response.snapshot_id);
                cursor = Some(response.cursor);
                offset = response.next_offset;
                attempt = 0;
                if !response.has_more {
                    return Ok(BootstrapOutcome {
                        snapshot_id: response.snapshot_id,
                        pages,
                        entities,
                        cursor: response.cursor,
                    });
                }
                if pages >= self.config.max_exchanges_per_sync {
                    return Err(ClientError::ExchangeLimit);
                }
            }
        }
        Err(ClientError::ExchangeLimit)
    }
}

fn validate_operation_results(
    response: &SyncResponse,
    submitted_operations: &[OperationId],
) -> Result<(), ClientError> {
    let submitted: HashSet<_> = submitted_operations.iter().copied().collect();
    if submitted.len() != submitted_operations.len() {
        return Err(ClientError::OperationResults);
    }
    let mut terminal = HashSet::with_capacity(submitted.len());
    for operation_id in response
        .acknowledged
        .iter()
        .map(|result| result.operation_id)
        .chain(response.rejected.iter().map(|result| result.operation_id))
        .chain(response.conflicts.iter().map(|result| result.operation_id))
    {
        if !submitted.contains(&operation_id) || !terminal.insert(operation_id) {
            return Err(ClientError::OperationResults);
        }
    }
    if terminal != submitted {
        return Err(ClientError::OperationResults);
    }
    Ok(())
}

fn validate_sync_directive(directive: &SyncDirective) -> Result<(), ClientError> {
    match directive {
        SyncDirective::Continue => Ok(()),
        SyncDirective::UpgradeRequired { minimum, current } => Err(ClientError::UpgradeRequired {
            minimum: *minimum,
            current: *current,
        }),
        SyncDirective::ResyncRequired { reason } => {
            Err(ClientError::ResyncRequired { reason: *reason })
        }
        SyncDirective::AuthorityChanged {
            authority_id,
            previous_epoch,
            current_epoch,
        } => Err(ClientError::AuthorityChanged {
            authority_id: *authority_id,
            previous_epoch: *previous_epoch,
            current_epoch: *current_epoch,
        }),
    }
}

fn trace_context(request_id: RequestId, session: &SessionMetadata) -> TraceContext {
    TraceContext {
        sync_session_id: session.session_id,
        request_id,
        device_id: session.device_id,
        tenant_id: session.tenant_id,
    }
}

fn duration_micros(duration: Duration) -> u64 {
    u64::try_from(duration.as_micros()).unwrap_or(u64::MAX)
}

fn usize_to_u64(value: usize) -> u64 {
    u64::try_from(value).unwrap_or(u64::MAX)
}

const fn transport_outcome(kind: TransportErrorKind) -> OutcomeKind {
    match kind {
        TransportErrorKind::Transient => OutcomeKind::TransientFailure,
        TransportErrorKind::Permanent => OutcomeKind::PermanentFailure,
    }
}

fn result_outcome(result: &Result<SyncOutcome, ClientError>) -> OutcomeKind {
    match result {
        Ok(_) => OutcomeKind::Success,
        Err(error) if error.is_transient() => OutcomeKind::TransientFailure,
        Err(_) => OutcomeKind::PermanentFailure,
    }
}

fn validate_snapshot_page(
    response: &BootstrapResponse,
    expected_protocol: ProtocolVersion,
    expected_snapshot: Option<SnapshotId>,
    expected_cursor: Option<Cursor>,
    expected_offset: u64,
    session: &SessionMetadata,
    limits: SnapshotLimits,
) -> Result<(), ClientError> {
    if response.protocol != expected_protocol
        || response.cursor.scope != session.scope_id
        || response.offset != expected_offset
        || expected_snapshot.is_some_and(|id| id != response.snapshot_id)
        || expected_cursor.is_some_and(|cursor| cursor != response.cursor)
        || response.next_offset
            != response
                .offset
                .saturating_add(u64::try_from(response.entities.len()).unwrap_or(u64::MAX))
    {
        return Err(ClientError::SnapshotMismatch);
    }
    let payload_bytes = response
        .entities
        .iter()
        .map(|entity| entity.payload.len())
        .fold(0_usize, usize::saturating_add);
    if response.entities.len() > usize::try_from(limits.max_entities).unwrap_or(usize::MAX)
        || payload_bytes > usize::try_from(limits.max_payload_bytes).unwrap_or(usize::MAX)
    {
        return Err(ClientError::ResponseLimits);
    }
    Ok(())
}

fn system_entropy() -> u64 {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_nanos());
    u64::try_from(nanos).unwrap_or_else(|_| {
        let high = u64::try_from(nanos >> 64).unwrap_or(0);
        let low = u64::try_from(nanos & u128::from(u64::MAX)).unwrap_or(0);
        high ^ low
    })
}

fn retry_not_before(delay: Duration) -> u64 {
    unix_time_ms().saturating_add(u64::try_from(delay.as_millis()).unwrap_or(u64::MAX))
}

fn unix_time_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| {
            u64::try_from(duration.as_millis()).unwrap_or(u64::MAX)
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use loom::{
        sync::{
            Arc as LoomArc,
            atomic::{AtomicBool as LoomAtomicBool, AtomicUsize as LoomAtomicUsize},
        },
        thread,
    };

    impl ReservationFlag for LoomAtomicBool {
        fn try_set(&self) -> bool {
            self.compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed)
                .is_ok()
        }

        fn clear(&self) {
            self.store(false, Ordering::Release);
        }
    }

    #[test]
    fn backoff_is_exponential_capped_and_jittered() {
        let retry = RetryConfig {
            max_attempts: 5,
            initial_delay: Duration::from_millis(100),
            max_delay: Duration::from_millis(500),
            multiplier: 2,
            jitter_percent: 20,
        };
        assert_eq!(retry.delay(0, 20).as_millis(), 100);
        assert!((320..=480).contains(&retry.delay(2, 0).as_millis()));
        assert!((400..=600).contains(&retry.delay(20, u64::MAX).as_millis()));
        let guided = retry.delay_with_server(0, 20, Some(Duration::from_millis(400)));
        assert!((400..=480).contains(&guided.as_millis()));
        assert_eq!(
            retry.delay_with_server(0, 20, Some(Duration::from_secs(60))),
            retry.max_delay
        );
    }

    #[test]
    fn adaptive_batching_uses_additive_increase_and_multiplicative_decrease() {
        let config = AdaptiveBatchConfig {
            minimum_operations: 8,
            maximum_operations: 64,
            increase_step: 8,
            target_latency: Duration::from_millis(100),
        };
        let mut batcher = AdaptiveBatcher::new(16, Some(config));
        batcher.record_success(Duration::from_millis(50), 16, 16);
        assert_eq!(batcher.limit(), 24);
        batcher.record_success(Duration::from_millis(200), 24, 24);
        assert_eq!(batcher.limit(), 12);
        batcher.record_failure();
        assert_eq!(batcher.limit(), 8);
    }

    #[tokio::test]
    async fn live_hint_handle_coalesces_before_bounded_coordinator_trigger() {
        let (triggers, mut receiver) = mpsc::channel(1);
        let (status, status_receiver) = watch::channel(SyncStatus::Idle { last_sync: None });
        let (health, health_receiver) = watch::channel(SyncHealth {
            status: SyncStatus::Idle { last_sync: None },
            pending_operations: 0,
            oldest_pending_age_ms: None,
            conflicts_pending: 0,
            last_successful_sync_unix_ms: None,
        });
        let (coordinator_status, coordinator_status_receiver) =
            watch::channel(CoordinatorStatus::SingleProcess);
        let handle = SyncCoordinatorHandle {
            triggers,
            status: status_receiver,
            health: health_receiver,
            coordinator_status: coordinator_status_receiver,
        };
        drop((status, health, coordinator_status));
        let tenant = aequora_types::TenantId::new();
        let scope = aequora_types::SyncScopeId::new();
        let tracker = HintWakeTracker::new(tenant, [scope]);
        let hint = SyncHint::v1(
            tenant,
            scope,
            Some(aequora_types::Sequence(1)),
            aequora_live::SyncHintReason::NewAuthoritativeChange,
        );
        assert_eq!(
            handle.observe_live_hint(&tracker, hint).await,
            Ok(HintWakeOutcome::Wake { generation: 1 })
        );
        assert_eq!(receiver.recv().await, Some(SyncTrigger::PushHint));
        assert_eq!(
            handle.observe_live_hint(&tracker, hint).await,
            Ok(HintWakeOutcome::Coalesced { generation: 1 })
        );
        assert!(receiver.try_recv().is_err());
    }

    #[test]
    fn loom_single_flight_reservation_never_has_two_owners() {
        loom::model(|| {
            let reservation = LoomArc::new(ReservationCore::new(LoomAtomicBool::new(false)));
            let active = LoomArc::new(LoomAtomicUsize::new(0));
            let entered = LoomArc::new(LoomAtomicUsize::new(0));
            let mut workers = Vec::new();
            for _ in 0..2 {
                let reservation = LoomArc::clone(&reservation);
                let active = LoomArc::clone(&active);
                let entered = LoomArc::clone(&entered);
                workers.push(thread::spawn(move || {
                    if let Some(_guard) = reservation.try_acquire() {
                        assert_eq!(active.fetch_add(1, Ordering::SeqCst), 0);
                        entered.fetch_add(1, Ordering::SeqCst);
                        thread::yield_now();
                        assert_eq!(active.fetch_sub(1, Ordering::SeqCst), 1);
                    }
                }));
            }
            for worker in workers {
                worker
                    .join()
                    .unwrap_or_else(|_| panic!("modeled reservation worker panicked"));
            }
            assert_eq!(active.load(Ordering::SeqCst), 0);
            assert!(entered.load(Ordering::SeqCst) >= 1);
        });
    }
}
