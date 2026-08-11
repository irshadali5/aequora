//! One-shot client synchronization and atomic reconciliation.

use aequora_observability::{MetricEvent, NoopObserver, Observer, OutcomeKind, TraceContext};
use aequora_protocol::{
    BootstrapRequest, BootstrapResponse, Capability, ClientLimits, PushHint, ResyncReason,
    SessionMetadata, SnapshotLimits, SyncDirective, SyncRequest, SyncResponse,
};
use aequora_store::StoreErrorKind;
use aequora_store::{LocalStore, StoreError};
use aequora_transport::{
    StreamingSyncTransport, SyncTransport, TransportError, TransportErrorKind,
};
use aequora_types::{Cursor, OperationId, ProtocolVersion, RequestId, SnapshotId};
use std::{
    collections::HashSet,
    sync::{Arc, Mutex, MutexGuard},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use thiserror::Error;
use tokio::sync::{mpsc, watch};

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
}

impl ClientConfig {
    /// Creates a configuration with conservative deterministic defaults.
    #[must_use]
    pub fn new(session: SessionMetadata) -> Self {
        Self {
            protocol: ProtocolVersion::V1,
            session,
            push_batch_size: 256,
            push_batch_bytes: 1_024 * 1_024,
            limits: ClientLimits::default(),
            capabilities: vec![Capability::PostcardV1, Capability::Tombstones],
            retry: RetryConfig::default(),
            max_exchanges_per_sync: 1_024,
            snapshot_limits: SnapshotLimits::default(),
            adaptive_batching: None,
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
}

/// Generic background worker that drains the same verified [`ClientSyncEngine`] API used by
/// foreground callers.
pub struct SyncCoordinator<L, T> {
    engine: Arc<ClientSyncEngine<L, T>>,
    config: SyncCoordinatorConfig,
    triggers: mpsc::Receiver<SyncTrigger>,
    status: watch::Sender<SyncStatus>,
    health: watch::Sender<SyncHealth>,
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
        (
            Self {
                engine,
                config,
                triggers,
                status,
                health,
                online: true,
                last_successful_sync_unix_ms: None,
            },
            SyncCoordinatorHandle {
                triggers: trigger_sender,
                status: status_receiver,
                health: health_receiver,
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
                    _instant = timer.tick() => Some(SyncTrigger::Manual),
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
                    self.synchronize().await;
                }
                SyncTrigger::Shutdown => {
                    self.set_status(SyncStatus::Shutdown);
                    return;
                }
                SyncTrigger::Manual | SyncTrigger::PushHint if self.online => {
                    self.synchronize().await;
                }
                SyncTrigger::LocalMutation if self.online => {
                    if !self.debounce_local_mutations().await {
                        return;
                    }
                }
                SyncTrigger::LocalMutation | SyncTrigger::Manual | SyncTrigger::PushHint => {}
            }
        }
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
        matches!(
            self,
            Self::Store(error) if matches!(error.kind, StoreErrorKind::Transient)
        ) || matches!(
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
    observer: Arc<dyn Observer>,
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
}

impl Default for ClientSyncEngineBuilder {
    fn default() -> Self {
        Self {
            store: MissingClientStore,
            transport: MissingClientTransport,
            config: None,
            observer: Arc::new(NoopObserver),
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
}

/// Client builder validation failure.
#[derive(Clone, Copy, Debug, Error, Eq, PartialEq)]
pub enum ClientBuildError {
    /// A session-bearing client configuration was not supplied.
    #[error("client configuration is required")]
    MissingConfig,
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
        Ok(ClientSyncEngine::new(self.store, self.transport, config).with_observer(self.observer))
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
        Self {
            store,
            transport,
            config,
            batcher,
            observer: Arc::new(NoopObserver),
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
        let retry_delay = self.config.retry.delay(0, system_entropy());
        self.run_once_with_retry_delay(retry_delay).await
    }

    async fn run_once_with_retry_delay(
        &self,
        retry_delay: Duration,
    ) -> Result<SyncOutcome, ClientError> {
        let cursor = self.store.load_cursor(self.config.session.scope_id).await?;
        let batch_limit = self.current_batch_limit();
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
            if encoded.len() <= self.config.push_batch_bytes {
                break;
            }
            if request.operations.len() <= 1 {
                return Err(ClientError::PushBatchTooLarge {
                    actual: encoded.len(),
                    maximum: self.config.push_batch_bytes,
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
        self.store.mark_sending(&operation_ids).await?;
        let trace = trace_context(request.request_id, &request.session);
        let started = Instant::now();
        let response = match self.transport.exchange(request).await {
            Ok(response) => response,
            Err(error) => {
                if error.kind == TransportErrorKind::Transient {
                    self.batcher().record_failure();
                }
                self.store
                    .mark_retry(&operation_ids, retry_not_before(retry_delay))
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
        if result.is_err() {
            self.store
                .mark_retry(&operation_ids, retry_not_before(retry_delay))
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
        match response.directive {
            SyncDirective::Continue => {}
            SyncDirective::UpgradeRequired { minimum, current } => {
                return Err(ClientError::UpgradeRequired { minimum, current });
            }
            SyncDirective::ResyncRequired { reason } => {
                return Err(ClientError::ResyncRequired { reason });
            }
        }
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
        self.store.reconcile(&response).await?;
        self.batcher().record_success(
            latency,
            submitted_operations.len(),
            outcome
                .acknowledged
                .saturating_add(outcome.rejected)
                .saturating_add(outcome.conflicts),
        );
        Ok(outcome)
    }

    /// Performs one exchange with bounded retries for typed transient failures.
    ///
    /// # Errors
    ///
    /// Returns the final [`ClientError`] after a permanent failure or retry exhaustion.
    pub async fn run_with_retry(&self) -> Result<SyncOutcome, ClientError> {
        let max_attempts = self.config.retry.max_attempts.max(1);
        let mut attempt = 0_u32;
        loop {
            let entropy = system_entropy() ^ u64::from(attempt);
            let delay = self.config.retry.delay(attempt, entropy);
            match self.run_once_with_retry_delay(delay).await {
                Ok(outcome) => return Ok(outcome),
                Err(error) if error.is_transient() && attempt + 1 < max_attempts => {
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
        let mut summary = SyncSummary::default();
        if self
            .store
            .load_cursor(self.config.session.scope_id)
            .await?
            .is_none()
        {
            self.bootstrap().await?;
        }
        for _ in 0..self.config.max_exchanges_per_sync {
            let outcome = match self.run_with_retry().await {
                Ok(outcome) => outcome,
                Err(ClientError::ResyncRequired { .. }) => {
                    self.bootstrap().await?;
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
            self.store.stage_snapshot(&response).await?;
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
                    let delay = self.config.retry.delay(attempt, entropy);
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
                    let delay = self.config.retry.delay(attempt, system_entropy());
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
                        let delay = self.config.retry.delay(attempt, system_entropy());
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
                self.store.stage_snapshot(&response).await?;
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
}
