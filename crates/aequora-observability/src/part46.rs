//! Production observability policy, bounded export, SLO, and alert contracts.

use aequora_types::{CorrelationId, OperationId, RequestId};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet, VecDeque},
    sync::{
        Mutex,
        atomic::{AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};
use thiserror::Error;

/// Maximum UTF-8 bytes in one bounded telemetry attribute or safe field.
pub const MAX_ATTRIBUTE_BYTES: usize = 96;
/// Maximum structured fields retained on one event.
pub const MAX_EVENT_FIELDS: usize = 24;
/// Maximum events emitted by one logical span.
pub const MAX_SPAN_EVENTS: usize = 32;

/// Stable, bounded metric identity. Names are operational API contracts.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
pub enum MetricId {
    HttpRequests,
    HttpRequestDuration,
    SyncExchanges,
    SyncExchangeDuration,
    OperationOutcomes,
    DatabasePoolWait,
    DatabaseTransactionDuration,
    TimelineLockWait,
    JournalAppendDuration,
    JobsPending,
    BootstrapOutcomes,
    BootstrapDuration,
    AuthorityContinuityErrors,
    OutboxPending,
    OutboxOldestPendingAge,
    TelemetryQueueDepth,
    TelemetryDropped,
    TelemetryExportFailures,
    TelemetryExportDuration,
    BuildInfo,
}

/// Recommended initial bounded metric catalog in stable order.
pub const INITIAL_METRICS: [MetricId; 20] = [
    MetricId::HttpRequests,
    MetricId::HttpRequestDuration,
    MetricId::SyncExchanges,
    MetricId::SyncExchangeDuration,
    MetricId::OperationOutcomes,
    MetricId::DatabasePoolWait,
    MetricId::DatabaseTransactionDuration,
    MetricId::TimelineLockWait,
    MetricId::JournalAppendDuration,
    MetricId::JobsPending,
    MetricId::BootstrapOutcomes,
    MetricId::BootstrapDuration,
    MetricId::AuthorityContinuityErrors,
    MetricId::OutboxPending,
    MetricId::OutboxOldestPendingAge,
    MetricId::TelemetryQueueDepth,
    MetricId::TelemetryDropped,
    MetricId::TelemetryExportFailures,
    MetricId::TelemetryExportDuration,
    MetricId::BuildInfo,
];

impl MetricId {
    /// Stable Prometheus-compatible metric name.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::HttpRequests => "aequora_http_requests_total",
            Self::HttpRequestDuration => "aequora_http_request_duration_seconds",
            Self::SyncExchanges => "aequora_sync_exchange_total",
            Self::SyncExchangeDuration => "aequora_sync_exchange_duration_seconds",
            Self::OperationOutcomes => "aequora_operation_outcomes_total",
            Self::DatabasePoolWait => "aequora_db_pool_wait_seconds",
            Self::DatabaseTransactionDuration => "aequora_db_transaction_duration_seconds",
            Self::TimelineLockWait => "aequora_timeline_lock_wait_seconds",
            Self::JournalAppendDuration => "aequora_journal_append_duration_seconds",
            Self::JobsPending => "aequora_jobs_pending",
            Self::BootstrapOutcomes => "aequora_bootstrap_outcomes_total",
            Self::BootstrapDuration => "aequora_bootstrap_duration_seconds",
            Self::AuthorityContinuityErrors => "aequora_authority_continuity_errors_total",
            Self::OutboxPending => "aequora_outbox_pending",
            Self::OutboxOldestPendingAge => "aequora_outbox_oldest_pending_age_seconds",
            Self::TelemetryQueueDepth => "aequora_telemetry_queue_depth",
            Self::TelemetryDropped => "aequora_telemetry_dropped_total",
            Self::TelemetryExportFailures => "aequora_telemetry_export_failures_total",
            Self::TelemetryExportDuration => "aequora_telemetry_export_duration_seconds",
            Self::BuildInfo => "aequora_build_info",
        }
    }

    /// Correct aggregation type.
    #[must_use]
    pub const fn kind(self) -> MetricKind {
        match self {
            Self::HttpRequests
            | Self::SyncExchanges
            | Self::OperationOutcomes
            | Self::BootstrapOutcomes
            | Self::AuthorityContinuityErrors
            | Self::TelemetryDropped
            | Self::TelemetryExportFailures => MetricKind::Counter,
            Self::JobsPending
            | Self::OutboxPending
            | Self::OutboxOldestPendingAge
            | Self::TelemetryQueueDepth
            | Self::BuildInfo => MetricKind::Gauge,
            Self::HttpRequestDuration
            | Self::SyncExchangeDuration
            | Self::DatabasePoolWait
            | Self::DatabaseTransactionDuration
            | Self::TimelineLockWait
            | Self::JournalAppendDuration
            | Self::BootstrapDuration
            | Self::TelemetryExportDuration => MetricKind::Histogram,
        }
    }
}

/// Supported metric aggregation behavior.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum MetricKind {
    Counter,
    Gauge,
    Histogram,
}

/// Closed, low-cardinality metric dimension set.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
pub enum AttributeKey {
    Route,
    Method,
    StatusClass,
    ResultClass,
    OperationKind,
    EntityType,
    Adapter,
    Environment,
    Region,
    Release,
    Platform,
    Component,
    JobKind,
}

/// Validated bounded metric dimension value.
#[derive(Clone, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct AttributeValue(String);

impl AttributeValue {
    /// Creates a printable, bounded label value.
    ///
    /// # Errors
    ///
    /// Rejects empty, oversized, control-character, or secret-looking values.
    pub fn new(value: impl Into<String>) -> Result<Self, ObservabilityError> {
        let value = value.into();
        if !valid_text(&value, MAX_ATTRIBUTE_BYTES) || looks_secret(&value) {
            return Err(ObservabilityError::UnsafeAttribute);
        }
        Ok(Self(value))
    }

    /// Validated text representation.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Bounded metric labels. Unbounded semantic identities have no representable key.
#[derive(Clone, Debug, Default, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct Attributes(BTreeMap<AttributeKey, AttributeValue>);

impl Attributes {
    /// Constructs labels while enforcing the attribute-count budget.
    ///
    /// # Errors
    ///
    /// Rejects more than eight dimensions.
    pub fn new(
        values: impl IntoIterator<Item = (AttributeKey, AttributeValue)>,
    ) -> Result<Self, ObservabilityError> {
        let values = values.into_iter().collect::<BTreeMap<_, _>>();
        if values.len() > 8 {
            return Err(ObservabilityError::AttributeBudget);
        }
        Ok(Self(values))
    }

    /// Read-only validated dimensions.
    #[must_use]
    pub const fn values(&self) -> &BTreeMap<AttributeKey, AttributeValue> {
        &self.0
    }
}

/// One backend-neutral metric observation.
#[derive(Clone, Debug, PartialEq)]
pub struct MetricObservation {
    pub id: MetricId,
    pub value: f64,
    pub attributes: Attributes,
}

/// Non-blocking instrumentation boundary implemented by runtime exporters.
pub trait MetricsSink: Send + Sync {
    fn record(&self, observation: MetricObservation);
}

/// Zero-telemetry implementation that leaves correctness paths unchanged.
#[derive(Clone, Copy, Debug, Default)]
pub struct NoopMetrics;

impl MetricsSink for NoopMetrics {
    fn record(&self, _observation: MetricObservation) {}
}

/// CI/runtime guard against accidental metric-series cardinality growth.
pub struct SeriesBudget {
    maximum: usize,
    observed: Mutex<BTreeSet<(MetricId, Attributes)>>,
}

impl SeriesBudget {
    /// Creates a non-zero series budget.
    ///
    /// # Errors
    ///
    /// Rejects zero capacity.
    pub fn new(maximum: usize) -> Result<Self, ObservabilityError> {
        if maximum == 0 {
            return Err(ObservabilityError::ZeroCapacity);
        }
        Ok(Self {
            maximum,
            observed: Mutex::new(BTreeSet::new()),
        })
    }

    /// Records a series identity or fails closed at the configured budget.
    ///
    /// # Errors
    ///
    /// Returns an error if the lock is unavailable or a new series exceeds the limit.
    pub fn observe(&self, id: MetricId, attributes: &Attributes) -> Result<(), ObservabilityError> {
        let mut observed = self
            .observed
            .try_lock()
            .map_err(|_| ObservabilityError::Busy)?;
        if observed.contains(&(id, attributes.clone())) {
            return Ok(());
        }
        if observed.len() >= self.maximum {
            return Err(ObservabilityError::SeriesBudget);
        }
        observed.insert((id, attributes.clone()));
        Ok(())
    }

    /// Current unique bounded series count.
    #[must_use]
    pub fn len(&self) -> usize {
        self.observed
            .try_lock()
            .map_or(self.maximum, |set| set.len())
    }

    /// Whether no metric series has been observed.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.observed.try_lock().is_ok_and(|set| set.is_empty())
    }
}

/// Telemetry delivery priority. Durable security events belong in audit, not this queue.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum TelemetryPriority {
    Verbose,
    Debug,
    OperationalInfo,
    OperationalError,
    CriticalSecurity,
}

/// One item accepted by the bounded exporter queue.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TelemetryItem<T> {
    pub priority: TelemetryPriority,
    pub payload: T,
}

/// Backend adapter for already-sanitized bounded batches.
pub trait TelemetryExporter<T>: Send + Sync {
    type Error;

    /// Exports one already-sanitized bounded batch.
    ///
    /// # Errors
    ///
    /// Returns the adapter-specific failure, which the bounded export worker consumes as a
    /// best-effort self-metric rather than propagating into synchronization.
    fn export(&self, batch: &[TelemetryItem<T>]) -> Result<(), Self::Error>;
}

/// Bounded, best-effort queue whose failure never changes domain results.
pub struct BoundedTelemetryQueue<T> {
    capacity: usize,
    batch_size: usize,
    queue: Mutex<VecDeque<TelemetryItem<T>>>,
    dropped: AtomicU64,
    export_failures: AtomicU64,
}

impl<T> BoundedTelemetryQueue<T> {
    /// Creates a queue with hard capacity and export batch bounds.
    ///
    /// # Errors
    ///
    /// Rejects zero values or a batch larger than queue capacity.
    pub fn new(capacity: usize, batch_size: usize) -> Result<Self, ObservabilityError> {
        if capacity == 0 || batch_size == 0 || batch_size > capacity {
            return Err(ObservabilityError::InvalidQueueBudget);
        }
        Ok(Self {
            capacity,
            batch_size,
            queue: Mutex::new(VecDeque::with_capacity(capacity)),
            dropped: AtomicU64::new(0),
            export_failures: AtomicU64::new(0),
        })
    }

    /// Attempts to enqueue without waiting for a contended lock.
    ///
    /// Lower-priority data is evicted for operational errors and critical security hints. A
    /// required durable security record must still be written separately to durable audit.
    #[must_use]
    pub fn try_push(&self, item: TelemetryItem<T>) -> QueueOutcome {
        let Ok(mut queue) = self.queue.try_lock() else {
            self.dropped.fetch_add(1, Ordering::Relaxed);
            return QueueOutcome::DroppedBusy;
        };
        if queue.len() < self.capacity {
            queue.push_back(item);
            return QueueOutcome::Accepted;
        }
        if item.priority >= TelemetryPriority::OperationalError {
            let candidate = queue
                .iter()
                .enumerate()
                .filter(|(_, queued)| queued.priority < item.priority)
                .min_by_key(|(_, queued)| queued.priority)
                .map(|(index, _)| index);
            if let Some(index) = candidate {
                let _ = queue.remove(index);
                queue.push_back(item);
                self.dropped.fetch_add(1, Ordering::Relaxed);
                return QueueOutcome::AcceptedAfterEviction;
            }
        }
        self.dropped.fetch_add(1, Ordering::Relaxed);
        QueueOutcome::DroppedFull
    }

    /// Drains one bounded batch on an exporter-owned worker.
    ///
    /// Export failure increments self-metrics and is deliberately not returned to domain callers.
    pub fn export_once<E>(&self, exporter: &E)
    where
        E: TelemetryExporter<T>,
    {
        let batch = {
            let Ok(mut queue) = self.queue.try_lock() else {
                return;
            };
            let count = self.batch_size.min(queue.len());
            queue.drain(..count).collect::<Vec<_>>()
        };
        if !batch.is_empty() && exporter.export(&batch).is_err() {
            self.export_failures.fetch_add(1, Ordering::Relaxed);
            self.dropped.fetch_add(
                u64::try_from(batch.len()).unwrap_or(u64::MAX),
                Ordering::Relaxed,
            );
        }
    }

    /// Queue depth and failure counters suitable for self-metrics.
    #[must_use]
    pub fn snapshot(&self) -> TelemetryQueueSnapshot {
        TelemetryQueueSnapshot {
            depth: self
                .queue
                .try_lock()
                .map_or(self.capacity, |queue| queue.len()),
            capacity: self.capacity,
            dropped: self.dropped.load(Ordering::Relaxed),
            export_failures: self.export_failures.load(Ordering::Relaxed),
        }
    }
}

/// Result of one non-blocking enqueue attempt.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum QueueOutcome {
    Accepted,
    AcceptedAfterEviction,
    DroppedBusy,
    DroppedFull,
}

/// Bounded exporter self-metrics.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TelemetryQueueSnapshot {
    pub depth: usize,
    pub capacity: usize,
    pub dropped: u64,
    pub export_failures: u64,
}

/// Privacy classification for log, trace, and diagnostic fields.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TelemetryFieldClass {
    SafeOperational,
    Identifier,
    Sensitive,
    PersonallyIdentifiable,
    Secret,
    Financial,
}

/// Sanitized value retained after classification policy is applied.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SanitizedValue {
    Shown(String),
    Redacted,
    Omitted,
}

/// One sanitized structured field; raw input is never retained.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SanitizedField {
    pub key: String,
    pub value: SanitizedValue,
}

impl SanitizedField {
    /// Applies privacy-safe production policy to one untrusted field.
    ///
    /// # Errors
    ///
    /// Rejects invalid field names and oversized values before sanitization.
    pub fn production(
        key: impl Into<String>,
        value: impl Into<String>,
        class: TelemetryFieldClass,
    ) -> Result<Self, ObservabilityError> {
        let key = key.into();
        let value = value.into();
        if !valid_key(&key) || value.len() > 4096 || value.chars().any(char::is_control) {
            return Err(ObservabilityError::UnsafeField);
        }
        let value = match class {
            _ if looks_secret(&value) => SanitizedValue::Redacted,
            TelemetryFieldClass::SafeOperational | TelemetryFieldClass::Identifier => {
                SanitizedValue::Shown(truncate_utf8(&value, MAX_ATTRIBUTE_BYTES))
            }
            TelemetryFieldClass::Sensitive | TelemetryFieldClass::Financial => {
                SanitizedValue::Redacted
            }
            TelemetryFieldClass::PersonallyIdentifiable | TelemetryFieldClass::Secret => {
                SanitizedValue::Omitted
            }
        };
        Ok(Self { key, value })
    }
}

/// Structured event level with stable operational meaning.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EventLevel {
    Error,
    Warn,
    Info,
    Debug,
    Trace,
}

/// Payload-free production log or diagnostic event.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StructuredEvent {
    pub level: EventLevel,
    pub name: String,
    pub component: String,
    pub build_id: String,
    pub fields: Vec<SanitizedField>,
}

impl StructuredEvent {
    /// Validates stable names and the per-event field budget.
    ///
    /// # Errors
    ///
    /// Rejects malformed metadata or too many fields.
    pub fn new(
        level: EventLevel,
        name: impl Into<String>,
        component: impl Into<String>,
        build_id: impl Into<String>,
        fields: Vec<SanitizedField>,
    ) -> Result<Self, ObservabilityError> {
        let name = name.into();
        let component = component.into();
        let build_id = build_id.into();
        if !valid_event_name(&name)
            || !valid_key(&component)
            || !valid_text(&build_id, MAX_ATTRIBUTE_BYTES)
            || fields.len() > MAX_EVENT_FIELDS
        {
            return Err(ObservabilityError::InvalidEvent);
        }
        Ok(Self {
            level,
            name,
            component,
            build_id,
            fields,
        })
    }
}

/// Observability-only trace identity.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TraceId(pub u128);

/// Observability-only span identity.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SpanId(pub u64);

/// Explicitly separated transport, trace, workflow, and durable operation identities.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TelemetryTraceContext {
    pub request_id: RequestId,
    pub trace_id: TraceId,
    pub span_id: SpanId,
    pub correlation_id: CorrelationId,
    pub operation_id: Option<OperationId>,
}

/// Parsed W3C-style trace parent used only as an untrusted telemetry hint.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TraceParent {
    pub trace_id: TraceId,
    pub parent_span_id: SpanId,
    pub sampled: bool,
}

impl TraceParent {
    /// Parses `00-<32 hex trace>-<16 hex span>-<2 hex flags>` without granting authorization.
    ///
    /// # Errors
    ///
    /// Rejects unsupported versions, malformed lengths/hex, and all-zero identities.
    pub fn parse_untrusted(value: &str) -> Result<Self, ObservabilityError> {
        let mut parts = value.split('-');
        let version = parts.next();
        let trace = parts.next();
        let span = parts.next();
        let flags = parts.next();
        if version != Some("00") || parts.next().is_some() {
            return Err(ObservabilityError::InvalidTraceParent);
        }
        let (Some(trace), Some(span), Some(flags)) = (trace, span, flags) else {
            return Err(ObservabilityError::InvalidTraceParent);
        };
        if trace.len() != 32 || span.len() != 16 || flags.len() != 2 {
            return Err(ObservabilityError::InvalidTraceParent);
        }
        let trace_id =
            u128::from_str_radix(trace, 16).map_err(|_| ObservabilityError::InvalidTraceParent)?;
        let parent_span_id =
            u64::from_str_radix(span, 16).map_err(|_| ObservabilityError::InvalidTraceParent)?;
        let flags =
            u8::from_str_radix(flags, 16).map_err(|_| ObservabilityError::InvalidTraceParent)?;
        if trace_id == 0 || parent_span_id == 0 {
            return Err(ObservabilityError::InvalidTraceParent);
        }
        Ok(Self {
            trace_id: TraceId(trace_id),
            parent_span_id: SpanId(parent_span_id),
            sampled: flags & 1 == 1,
        })
    }
}

/// Head-sampling decision inputs. Incoming trace flags are never authorization.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SamplingContext {
    pub trace_id: TraceId,
    pub class: SamplingClass,
}

/// Stable sampling class.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SamplingClass {
    CriticalSecurity,
    Error,
    Slow,
    Normal,
    Sensitive,
}

/// Bounded deterministic head-sampling policy in basis points.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct SamplingPolicy {
    pub error_basis_points: u16,
    pub slow_basis_points: u16,
    pub normal_basis_points: u16,
}

impl SamplingPolicy {
    /// Validates every ratio is at most 100 percent.
    ///
    /// # Errors
    ///
    /// Rejects ratios above 10,000 basis points.
    pub const fn validate(self) -> Result<(), ObservabilityError> {
        if self.error_basis_points > 10_000
            || self.slow_basis_points > 10_000
            || self.normal_basis_points > 10_000
        {
            Err(ObservabilityError::InvalidSampling)
        } else {
            Ok(())
        }
    }

    /// Deterministic sampling decision; critical security is retained and sensitive traces are
    /// never remotely sampled.
    #[must_use]
    pub fn sample(self, context: SamplingContext) -> bool {
        let bucket = u16::try_from(context.trace_id.0 % 10_000).unwrap_or(0);
        match context.class {
            SamplingClass::CriticalSecurity => true,
            SamplingClass::Sensitive => false,
            SamplingClass::Error => bucket < self.error_basis_points,
            SamplingClass::Slow => bucket < self.slow_basis_points,
            SamplingClass::Normal => bucket < self.normal_basis_points,
        }
    }
}

/// Stable operation result class used by metrics and SLI eligibility.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
pub enum OperationOutcomeClass {
    Accepted,
    Duplicate,
    RejectedValidation,
    RejectedAuthorization,
    Conflict,
    BusinessRejection,
    RetryableInfrastructureFailure,
    InternalFailure,
}

impl OperationOutcomeClass {
    /// Whether this result is eligible for server availability calculations.
    #[must_use]
    pub const fn availability_eligible(self) -> bool {
        matches!(
            self,
            Self::Accepted
                | Self::Duplicate
                | Self::RetryableInfrastructureFailure
                | Self::InternalFailure
        )
    }

    /// Whether an eligible request succeeded.
    #[must_use]
    pub const fn availability_success(self) -> bool {
        matches!(self, Self::Accepted | Self::Duplicate)
    }
}

/// SLI represented by a product-owned SLO.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum SliKind {
    SyncAvailability,
    AuthorityCommitLatency,
    BootstrapSuccess,
    BootstrapLatency,
    ClientFleetSync,
}

/// Fully owned and actionable SLO definition.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SloDefinition {
    pub id: String,
    pub sli: SliKind,
    pub owner: String,
    pub measurement_source: String,
    pub window_seconds: u64,
    pub target_basis_points: u16,
    pub exclusions: BTreeSet<OperationOutcomeClass>,
    pub runbook: String,
}

impl SloDefinition {
    /// Checks ownership, target, time window, source, and runbook metadata.
    ///
    /// # Errors
    ///
    /// Rejects incomplete or invalid definitions.
    pub fn validate(&self) -> Result<(), ObservabilityError> {
        if !valid_key(&self.id)
            || !valid_key(&self.owner)
            || !valid_text(&self.measurement_source, MAX_ATTRIBUTE_BYTES)
            || self.window_seconds == 0
            || self.target_basis_points == 0
            || self.target_basis_points > 10_000
            || !valid_path(&self.runbook)
        {
            return Err(ObservabilityError::InvalidSlo);
        }
        Ok(())
    }
}

/// Operational alert response class.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum AlertSeverity {
    Page,
    Ticket,
    Warning,
    SecurityResponse,
}

/// Client process topology used to prevent double-counting one logical operation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ClientTelemetryOwner {
    /// UI and runtime share one process, which owns logical-operation telemetry.
    InProcessRuntime,
    /// Desktop agent owns store coordination and logical-operation telemetry.
    DesktopAgent,
    /// GUI observes an agent-owned store and emits UI telemetry only.
    AgentConnectedGui,
}

impl ClientTelemetryOwner {
    /// Whether this process may count logical local mutations and sync outcomes.
    #[must_use]
    pub const fn owns_logical_operation_metrics(self) -> bool {
        matches!(self, Self::InProcessRuntime | Self::DesktopAgent)
    }
}

/// Outcome of bounded repeated-event coalescing.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RateLimitOutcome {
    /// First event in a new bounded key/window.
    First,
    /// Event remains within the configured burst.
    Emit,
    /// Repeated event was coalesced; the value is the window's suppressed count.
    Suppressed(u64),
    /// A new window should begin with a summary of the preceding suppressed count.
    PeriodicSummary(u64),
    /// Recovery closes the repeated-event interval and reports total suppression.
    Recovery(u64),
    /// The bounded event-key map was full.
    KeyBudgetExceeded,
    /// Lock contention caused best-effort suppression.
    Busy,
}

#[derive(Clone, Copy, Debug)]
struct RateState {
    window_started: Duration,
    emitted: u32,
    suppressed: u64,
}

/// Bounded first/periodic-summary/recovery limiter for repetitive structured events.
pub struct EventRateLimiter {
    maximum_keys: usize,
    burst: u32,
    window: Duration,
    states: Mutex<BTreeMap<String, RateState>>,
}

impl EventRateLimiter {
    /// Creates a limiter with non-zero key, burst, and window bounds.
    ///
    /// # Errors
    ///
    /// Rejects zero or excessive limits.
    pub fn new(
        maximum_keys: usize,
        burst: u32,
        window: Duration,
    ) -> Result<Self, ObservabilityError> {
        if maximum_keys == 0
            || maximum_keys > 4096
            || burst == 0
            || window.is_zero()
            || window > Duration::from_secs(3600)
        {
            return Err(ObservabilityError::InvalidRateLimit);
        }
        Ok(Self {
            maximum_keys,
            burst,
            window,
            states: Mutex::new(BTreeMap::new()),
        })
    }

    /// Observes a stable event name at a monotonic process-relative time.
    #[must_use]
    pub fn observe(&self, event_name: &str, elapsed: Duration) -> RateLimitOutcome {
        if !valid_event_name(event_name) {
            return RateLimitOutcome::KeyBudgetExceeded;
        }
        let Ok(mut states) = self.states.try_lock() else {
            return RateLimitOutcome::Busy;
        };
        if !states.contains_key(event_name) {
            if states.len() >= self.maximum_keys {
                return RateLimitOutcome::KeyBudgetExceeded;
            }
            states.insert(
                event_name.to_owned(),
                RateState {
                    window_started: elapsed,
                    emitted: 1,
                    suppressed: 0,
                },
            );
            return RateLimitOutcome::First;
        }
        let Some(state) = states.get_mut(event_name) else {
            return RateLimitOutcome::KeyBudgetExceeded;
        };
        if elapsed.saturating_sub(state.window_started) >= self.window {
            let suppressed = state.suppressed;
            *state = RateState {
                window_started: elapsed,
                emitted: 1,
                suppressed: 0,
            };
            return RateLimitOutcome::PeriodicSummary(suppressed);
        }
        if state.emitted < self.burst {
            state.emitted += 1;
            RateLimitOutcome::Emit
        } else {
            state.suppressed = state.suppressed.saturating_add(1);
            RateLimitOutcome::Suppressed(state.suppressed)
        }
    }

    /// Closes one repeated-event interval with an explicit recovery event.
    #[must_use]
    pub fn recovery(&self, event_name: &str) -> RateLimitOutcome {
        let Ok(mut states) = self.states.try_lock() else {
            return RateLimitOutcome::Busy;
        };
        states
            .remove(event_name)
            .map_or(RateLimitOutcome::Recovery(0), |state| {
                RateLimitOutcome::Recovery(state.suppressed)
            })
    }
}

/// Stable actionable alert metadata.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct AlertDefinition {
    pub id: String,
    pub severity: AlertSeverity,
    pub owner: String,
    pub condition: String,
    pub runbook: String,
    pub group_by: BTreeSet<AttributeKey>,
}

/// Versioned machine-readable catalog of production alert metadata.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct AlertCatalog {
    pub schema_version: u16,
    pub alerts: Vec<AlertDefinition>,
}

impl AlertCatalog {
    /// Parses and validates a RON alert catalog.
    ///
    /// # Errors
    ///
    /// Rejects malformed RON, unsupported schema, duplicate IDs, or invalid alerts.
    pub fn from_ron(input: &str) -> Result<Self, ObservabilityError> {
        let catalog: Self = ron::from_str(input).map_err(|_| ObservabilityError::Encoding)?;
        if catalog.schema_version != 1 || catalog.alerts.is_empty() {
            return Err(ObservabilityError::InvalidAlert);
        }
        let mut ids = BTreeSet::new();
        for alert in &catalog.alerts {
            alert.validate()?;
            if !ids.insert(alert.id.as_str()) {
                return Err(ObservabilityError::DuplicateIdentity);
            }
        }
        Ok(catalog)
    }
}

/// Versioned machine-readable catalog of product-owned SLOs.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SloCatalog {
    pub schema_version: u16,
    pub objectives: Vec<SloDefinition>,
}

impl SloCatalog {
    /// Parses and validates a RON SLO catalog.
    ///
    /// # Errors
    ///
    /// Rejects malformed RON, unsupported schema, duplicate IDs, or incomplete objectives.
    pub fn from_ron(input: &str) -> Result<Self, ObservabilityError> {
        let catalog: Self = ron::from_str(input).map_err(|_| ObservabilityError::Encoding)?;
        if catalog.schema_version != 1 || catalog.objectives.is_empty() {
            return Err(ObservabilityError::InvalidSlo);
        }
        let mut ids = BTreeSet::new();
        for objective in &catalog.objectives {
            objective.validate()?;
            if !ids.insert(objective.id.as_str()) {
                return Err(ObservabilityError::DuplicateIdentity);
            }
        }
        Ok(catalog)
    }
}

impl AlertDefinition {
    /// Validates stable identity, ownership, actionable condition, and runbook.
    ///
    /// # Errors
    ///
    /// Rejects incomplete, high-cardinality, or malformed alert metadata.
    pub fn validate(&self) -> Result<(), ObservabilityError> {
        if !valid_alert_id(&self.id)
            || !valid_key(&self.owner)
            || !valid_text(&self.condition, 512)
            || !valid_path(&self.runbook)
            || self.group_by.len() > 4
        {
            return Err(ObservabilityError::InvalidAlert);
        }
        Ok(())
    }
}

/// Monotonic latency timer unaffected by wall-clock adjustment.
#[derive(Debug)]
pub struct MonotonicTimer(Instant);

impl MonotonicTimer {
    /// Starts a monotonic interval.
    #[must_use]
    pub fn start() -> Self {
        Self(Instant::now())
    }

    /// Elapsed monotonic duration.
    #[must_use]
    pub fn elapsed(&self) -> Duration {
        self.0.elapsed()
    }
}

/// Vendor-neutral observability configuration with explicit resource budgets.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ObservabilityConfig {
    pub export: ExportMode,
    pub queue_capacity: usize,
    pub batch_size: usize,
    pub maximum_metric_series: usize,
    pub maximum_client_disk_bytes: u64,
    pub sampling: SamplingPolicy,
}

impl Default for ObservabilityConfig {
    fn default() -> Self {
        Self::PRODUCTION
    }
}

impl ObservabilityConfig {
    /// Privacy-safe production defaults.
    pub const PRODUCTION: Self = Self {
        export: ExportMode::BoundedRemote,
        queue_capacity: 4096,
        batch_size: 256,
        maximum_metric_series: 4096,
        maximum_client_disk_bytes: 8 * 1024 * 1024,
        sampling: SamplingPolicy {
            error_basis_points: 10_000,
            slow_basis_points: 10_000,
            normal_basis_points: 500,
        },
    };

    /// Validates all queue, disk, cardinality, and sampling budgets.
    ///
    /// # Errors
    ///
    /// Rejects zero, internally inconsistent, or unbounded settings.
    pub const fn validate(self) -> Result<(), ObservabilityError> {
        if self.queue_capacity == 0
            || self.batch_size == 0
            || self.batch_size > self.queue_capacity
            || self.queue_capacity > 65_536
            || self.maximum_metric_series == 0
            || self.maximum_metric_series > 65_536
            || self.maximum_client_disk_bytes > 64 * 1024 * 1024
        {
            return Err(ObservabilityError::InvalidConfiguration);
        }
        self.sampling.validate()
    }
}

/// Export behavior; local diagnostics and durable audit remain available in every mode.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum ExportMode {
    Disabled,
    LocalOnly,
    BoundedRemote,
}

/// Production observability contract errors.
#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
pub enum ObservabilityError {
    #[error("observability metadata encoding is invalid")]
    Encoding,
    #[error("observability metadata identity is duplicated")]
    DuplicateIdentity,
    #[error("incoming trace parent is malformed or unsupported")]
    InvalidTraceParent,
    #[error("event rate-limit budget is invalid")]
    InvalidRateLimit,
    #[error("metric attribute is unsafe or unbounded")]
    UnsafeAttribute,
    #[error("metric attribute count exceeds its budget")]
    AttributeBudget,
    #[error("metric series capacity must be non-zero")]
    ZeroCapacity,
    #[error("metric series budget exceeded")]
    SeriesBudget,
    #[error("observability state is currently busy")]
    Busy,
    #[error("telemetry queue budget is invalid")]
    InvalidQueueBudget,
    #[error("structured field is invalid or unbounded")]
    UnsafeField,
    #[error("structured event metadata is invalid")]
    InvalidEvent,
    #[error("trace sampling policy is invalid")]
    InvalidSampling,
    #[error("SLO definition is incomplete or invalid")]
    InvalidSlo,
    #[error("alert definition is incomplete or invalid")]
    InvalidAlert,
    #[error("observability configuration is invalid")]
    InvalidConfiguration,
}

fn valid_text(value: &str, maximum: usize) -> bool {
    !value.is_empty() && value.len() <= maximum && !value.chars().any(char::is_control)
}

fn valid_key(value: &str) -> bool {
    valid_text(value, 64)
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
}

fn valid_event_name(value: &str) -> bool {
    valid_text(value, 96)
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
}

fn valid_alert_id(value: &str) -> bool {
    value.starts_with("AEQ-ALERT-")
        && valid_text(value, 64)
        && value
            .bytes()
            .all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit() || byte == b'-')
}

fn valid_path(value: &str) -> bool {
    valid_text(value, 256) && !value.contains("..") && !value.starts_with('/')
}

fn looks_secret(value: &str) -> bool {
    let lowercase = value.to_ascii_lowercase();
    lowercase.contains("secret_do_not_leak")
        || lowercase.contains("bearer ")
        || lowercase.contains("postgres://")
        || lowercase.contains("private_key")
        || lowercase.contains("password=")
}

fn truncate_utf8(value: &str, maximum: usize) -> String {
    if value.len() <= maximum {
        return value.to_owned();
    }
    let mut end = maximum;
    while !value.is_char_boundary(end) {
        end -= 1;
    }
    value[..end].to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stable_metrics_have_prefix_and_correct_types() {
        let metrics = [
            MetricId::HttpRequests,
            MetricId::HttpRequestDuration,
            MetricId::JobsPending,
            MetricId::TelemetryDropped,
            MetricId::BuildInfo,
        ];
        for metric in metrics {
            assert!(metric.name().starts_with("aequora_"));
        }
        assert_eq!(MetricId::HttpRequests.kind(), MetricKind::Counter);
        assert_eq!(MetricId::HttpRequestDuration.kind(), MetricKind::Histogram);
        assert_eq!(MetricId::JobsPending.kind(), MetricKind::Gauge);
    }

    #[test]
    fn identifiers_cannot_be_metric_keys_and_series_are_bounded() {
        let budget = SeriesBudget::new(2).unwrap_or_else(|error| panic!("{error}"));
        let first = Attributes::new([(
            AttributeKey::Region,
            AttributeValue::new("india").unwrap_or_else(|error| panic!("{error}")),
        )])
        .unwrap_or_else(|error| panic!("{error}"));
        let second = Attributes::new([(
            AttributeKey::Region,
            AttributeValue::new("eu").unwrap_or_else(|error| panic!("{error}")),
        )])
        .unwrap_or_else(|error| panic!("{error}"));
        let third = Attributes::new([(
            AttributeKey::Region,
            AttributeValue::new("us").unwrap_or_else(|error| panic!("{error}")),
        )])
        .unwrap_or_else(|error| panic!("{error}"));
        assert!(budget.observe(MetricId::SyncExchanges, &first).is_ok());
        assert!(budget.observe(MetricId::SyncExchanges, &second).is_ok());
        assert_eq!(
            budget.observe(MetricId::SyncExchanges, &third),
            Err(ObservabilityError::SeriesBudget)
        );
    }

    #[test]
    fn secret_sentinel_never_survives_sanitization() {
        for class in [
            TelemetryFieldClass::SafeOperational,
            TelemetryFieldClass::Identifier,
            TelemetryFieldClass::Sensitive,
            TelemetryFieldClass::PersonallyIdentifiable,
            TelemetryFieldClass::Secret,
            TelemetryFieldClass::Financial,
        ] {
            let field = SanitizedField::production(
                "error",
                "SECRET_DO_NOT_LEAK_123 postgres://credential",
                class,
            )
            .unwrap_or_else(|error| panic!("{error}"));
            assert!(!format!("{field:?}").contains("SECRET_DO_NOT_LEAK_123"));
        }
    }

    #[test]
    fn exporter_failure_is_bounded_and_best_effort() {
        struct Failing;
        impl TelemetryExporter<u8> for Failing {
            type Error = ();
            fn export(&self, _batch: &[TelemetryItem<u8>]) -> Result<(), Self::Error> {
                Err(())
            }
        }

        let queue = BoundedTelemetryQueue::new(2, 2).unwrap_or_else(|error| panic!("{error}"));
        assert_eq!(
            queue.try_push(TelemetryItem {
                priority: TelemetryPriority::Debug,
                payload: 1,
            }),
            QueueOutcome::Accepted
        );
        queue.export_once(&Failing);
        assert_eq!(queue.snapshot().export_failures, 1);
        assert_eq!(queue.snapshot().depth, 0);
    }

    #[test]
    fn priority_queue_evicts_debug_before_error() {
        let queue = BoundedTelemetryQueue::new(1, 1).unwrap_or_else(|error| panic!("{error}"));
        let _ = queue.try_push(TelemetryItem {
            priority: TelemetryPriority::Debug,
            payload: 1_u8,
        });
        assert_eq!(
            queue.try_push(TelemetryItem {
                priority: TelemetryPriority::OperationalError,
                payload: 2,
            }),
            QueueOutcome::AcceptedAfterEviction
        );
        assert_eq!(queue.snapshot().dropped, 1);
    }

    #[test]
    fn sampling_retains_errors_and_never_sensitive_traces() {
        let policy = SamplingPolicy {
            error_basis_points: 10_000,
            slow_basis_points: 10_000,
            normal_basis_points: 0,
        };
        let context = |class| SamplingContext {
            trace_id: TraceId(42),
            class,
        };
        assert!(policy.sample(context(SamplingClass::Error)));
        assert!(!policy.sample(context(SamplingClass::Normal)));
        assert!(!policy.sample(context(SamplingClass::Sensitive)));
    }

    #[test]
    fn availability_excludes_expected_client_and_business_outcomes() {
        assert!(OperationOutcomeClass::Accepted.availability_eligible());
        assert!(OperationOutcomeClass::Duplicate.availability_success());
        assert!(!OperationOutcomeClass::Conflict.availability_eligible());
        assert!(!OperationOutcomeClass::RejectedValidation.availability_eligible());
        assert!(OperationOutcomeClass::InternalFailure.availability_eligible());
        assert!(!OperationOutcomeClass::InternalFailure.availability_success());
    }

    #[test]
    fn production_config_and_alert_metadata_are_actionable() {
        assert!(ObservabilityConfig::PRODUCTION.validate().is_ok());
        let alert = AlertDefinition {
            id: "AEQ-ALERT-AUTHORITY-001".to_owned(),
            severity: AlertSeverity::Page,
            owner: "runtime".to_owned(),
            condition: "authority commits fail across the deployment".to_owned(),
            runbook: "deploy/observability/runbooks/authority-commit-failure.md".to_owned(),
            group_by: [AttributeKey::Environment, AttributeKey::Region]
                .into_iter()
                .collect(),
        };
        assert!(alert.validate().is_ok());
    }

    #[test]
    fn checked_in_alerts_and_slos_are_valid() {
        let alerts = include_str!("../../../deploy/observability/alerts/catalog.ron");
        let slos = include_str!("../../../deploy/observability/slos/catalog.ron");
        assert!(AlertCatalog::from_ron(alerts).is_ok());
        assert!(SloCatalog::from_ron(slos).is_ok());
    }

    #[test]
    fn trace_parent_is_untrusted_bounded_and_strict() {
        let parsed =
            TraceParent::parse_untrusted("00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01")
                .unwrap_or_else(|error| panic!("{error}"));
        assert!(parsed.sampled);
        assert_eq!(parsed.parent_span_id, SpanId(0x00f0_67aa_0ba9_02b7));
        assert!(
            TraceParent::parse_untrusted("00-00000000000000000000000000000000-0000000000000000-01")
                .is_err()
        );
    }

    #[test]
    fn repeated_logs_emit_first_summary_and_recovery() {
        let limiter = EventRateLimiter::new(2, 1, Duration::from_secs(10))
            .unwrap_or_else(|error| panic!("{error}"));
        assert_eq!(
            limiter.observe("db.connection.failed", Duration::ZERO),
            RateLimitOutcome::First
        );
        assert_eq!(
            limiter.observe("db.connection.failed", Duration::from_secs(1)),
            RateLimitOutcome::Suppressed(1)
        );
        assert_eq!(
            limiter.observe("db.connection.failed", Duration::from_secs(10)),
            RateLimitOutcome::PeriodicSummary(1)
        );
        assert_eq!(
            limiter.recovery("db.connection.failed"),
            RateLimitOutcome::Recovery(0)
        );
    }

    #[test]
    fn desktop_agent_is_single_logical_metric_owner() {
        assert!(ClientTelemetryOwner::DesktopAgent.owns_logical_operation_metrics());
        assert!(!ClientTelemetryOwner::AgentConnectedGui.owns_logical_operation_metrics());
    }
}
