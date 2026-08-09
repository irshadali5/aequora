//! Payload-free metrics and optional structured tracing hooks.

use std::sync::atomic::{AtomicU64, Ordering};

use aequora_types::{DeviceId, RequestId, SessionId, TenantId};

/// Non-sensitive identifiers that correlate one synchronization request across layers.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TraceContext {
    /// Long-lived synchronization session.
    pub sync_session_id: SessionId,
    /// Unique request attempt.
    pub request_id: RequestId,
    /// Client installation.
    pub device_id: DeviceId,
    /// Authenticated tenant boundary.
    pub tenant_id: TenantId,
}

/// Stable result category for metrics and retry analysis.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OutcomeKind {
    /// Exchange or page completed successfully.
    Success,
    /// Failure may succeed unchanged later.
    TransientFailure,
    /// Failure requires changed input, authorization, or intervention.
    PermanentFailure,
}

/// Timed server pipeline phase.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ServerPhaseKind {
    /// Wire DTO structural and compatibility validation.
    Validation,
    /// Application authorization, execution, dependency, and conflict logic.
    Execution,
    /// Authoritative persistence reads or transactions.
    Database,
}

/// Payload-free synchronization lifecycle event.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MetricEvent {
    /// Point-in-time durable client queue and health gauges.
    ClientState {
        /// Replayable outbox operations.
        outbox_pending: u64,
        /// Age of the oldest replayable operation, when one exists.
        oldest_pending_age_ms: Option<u64>,
        /// Unix timestamp of the latest successful complete drain.
        last_success_unix_ms: Option<u64>,
        /// Conflicts awaiting application resolution.
        conflicts_pending: u64,
    },
    /// One client exchange attempt.
    ClientExchange {
        duration_micros: u64,
        operations: u64,
        changes: u64,
        conflicts: u64,
        rejections: u64,
        outcome: OutcomeKind,
    },
    /// One server exchange request.
    ServerExchange {
        duration_micros: u64,
        operations: u64,
        changes: u64,
        conflicts: u64,
        rejections: u64,
        outcome: OutcomeKind,
    },
    /// Exact framed HTTP bytes observed by a transport boundary.
    TransportBytes {
        /// Request frame bytes sent by the client/received by the server.
        uploaded: u64,
        /// Response frame bytes sent by the server/received by the client.
        downloaded: u64,
    },
    /// Duration of a named server processing phase.
    ServerPhase {
        /// Stable phase category.
        phase: ServerPhaseKind,
        /// Wall duration in microseconds.
        duration_micros: u64,
    },
    /// One bootstrap page attempt.
    BootstrapPage {
        duration_micros: u64,
        entities: u64,
        outcome: OutcomeKind,
    },
    /// Large CPU workload sent to the dedicated compute pool.
    ComputeOffload { items: u64 },
}

/// Application-provided metrics/tracing boundary.
pub trait Observer: Send + Sync {
    /// Records a payload-free event. Implementations should not block request processing.
    fn record(&self, event: MetricEvent);

    /// Records an event with non-sensitive correlation identifiers. Implementations that do not
    /// need dimensional telemetry inherit the payload-free aggregate behavior.
    fn record_with_context(&self, _context: TraceContext, event: MetricEvent) {
        self.record(event);
    }
}

/// Observer used when instrumentation is not configured.
#[derive(Clone, Copy, Debug, Default)]
pub struct NoopObserver;

impl Observer for NoopObserver {
    fn record(&self, _event: MetricEvent) {}
}

/// Lock-free aggregate counters suitable for health/admin endpoints.
#[derive(Default)]
pub struct AtomicMetrics {
    outbox_pending: AtomicU64,
    oldest_pending_age_ms: AtomicU64,
    sync_last_success_unix_ms: AtomicU64,
    conflicts_pending: AtomicU64,
    client_exchanges: AtomicU64,
    server_exchanges: AtomicU64,
    failures: AtomicU64,
    operations: AtomicU64,
    changes: AtomicU64,
    conflicts: AtomicU64,
    rejections: AtomicU64,
    bootstrap_pages: AtomicU64,
    bootstrap_entities: AtomicU64,
    compute_offloads: AtomicU64,
    total_duration_micros: AtomicU64,
    uploaded_bytes: AtomicU64,
    downloaded_bytes: AtomicU64,
    validation_duration_micros: AtomicU64,
    execution_duration_micros: AtomicU64,
    database_duration_micros: AtomicU64,
}

impl AtomicMetrics {
    /// Returns a point-in-time relaxed snapshot.
    #[must_use]
    pub fn snapshot(&self) -> MetricsSnapshot {
        MetricsSnapshot {
            outbox_pending: self.outbox_pending.load(Ordering::Relaxed),
            oldest_pending_age_ms: optional_gauge(
                self.oldest_pending_age_ms.load(Ordering::Relaxed),
            ),
            sync_last_success_unix_ms: optional_gauge(
                self.sync_last_success_unix_ms.load(Ordering::Relaxed),
            ),
            conflicts_pending: self.conflicts_pending.load(Ordering::Relaxed),
            client_exchanges: self.client_exchanges.load(Ordering::Relaxed),
            server_exchanges: self.server_exchanges.load(Ordering::Relaxed),
            failures: self.failures.load(Ordering::Relaxed),
            operations: self.operations.load(Ordering::Relaxed),
            changes: self.changes.load(Ordering::Relaxed),
            conflicts: self.conflicts.load(Ordering::Relaxed),
            rejections: self.rejections.load(Ordering::Relaxed),
            bootstrap_pages: self.bootstrap_pages.load(Ordering::Relaxed),
            bootstrap_entities: self.bootstrap_entities.load(Ordering::Relaxed),
            compute_offloads: self.compute_offloads.load(Ordering::Relaxed),
            total_duration_micros: self.total_duration_micros.load(Ordering::Relaxed),
            uploaded_bytes: self.uploaded_bytes.load(Ordering::Relaxed),
            downloaded_bytes: self.downloaded_bytes.load(Ordering::Relaxed),
            validation_duration_micros: self.validation_duration_micros.load(Ordering::Relaxed),
            execution_duration_micros: self.execution_duration_micros.load(Ordering::Relaxed),
            database_duration_micros: self.database_duration_micros.load(Ordering::Relaxed),
        }
    }
}

impl Observer for AtomicMetrics {
    fn record(&self, event: MetricEvent) {
        match event {
            MetricEvent::ClientState {
                outbox_pending,
                oldest_pending_age_ms,
                last_success_unix_ms,
                conflicts_pending,
            } => {
                self.outbox_pending.store(outbox_pending, Ordering::Relaxed);
                self.oldest_pending_age_ms.store(
                    encode_optional_gauge(oldest_pending_age_ms),
                    Ordering::Relaxed,
                );
                self.sync_last_success_unix_ms.store(
                    encode_optional_gauge(last_success_unix_ms),
                    Ordering::Relaxed,
                );
                self.conflicts_pending
                    .store(conflicts_pending, Ordering::Relaxed);
            }
            MetricEvent::ClientExchange {
                duration_micros,
                operations,
                changes,
                conflicts,
                rejections,
                outcome,
            } => {
                self.client_exchanges.fetch_add(1, Ordering::Relaxed);
                self.operations.fetch_add(operations, Ordering::Relaxed);
                self.changes.fetch_add(changes, Ordering::Relaxed);
                self.conflicts.fetch_add(conflicts, Ordering::Relaxed);
                self.rejections.fetch_add(rejections, Ordering::Relaxed);
                self.total_duration_micros
                    .fetch_add(duration_micros, Ordering::Relaxed);
                record_failure(&self.failures, outcome);
            }
            MetricEvent::ServerExchange {
                duration_micros,
                operations,
                changes,
                conflicts,
                rejections,
                outcome,
            } => {
                self.server_exchanges.fetch_add(1, Ordering::Relaxed);
                self.operations.fetch_add(operations, Ordering::Relaxed);
                self.changes.fetch_add(changes, Ordering::Relaxed);
                self.conflicts.fetch_add(conflicts, Ordering::Relaxed);
                self.rejections.fetch_add(rejections, Ordering::Relaxed);
                self.total_duration_micros
                    .fetch_add(duration_micros, Ordering::Relaxed);
                record_failure(&self.failures, outcome);
            }
            MetricEvent::TransportBytes {
                uploaded,
                downloaded,
            } => {
                self.uploaded_bytes.fetch_add(uploaded, Ordering::Relaxed);
                self.downloaded_bytes
                    .fetch_add(downloaded, Ordering::Relaxed);
            }
            MetricEvent::ServerPhase {
                phase,
                duration_micros,
            } => {
                let counter = match phase {
                    ServerPhaseKind::Validation => &self.validation_duration_micros,
                    ServerPhaseKind::Execution => &self.execution_duration_micros,
                    ServerPhaseKind::Database => &self.database_duration_micros,
                };
                counter.fetch_add(duration_micros, Ordering::Relaxed);
            }
            MetricEvent::BootstrapPage {
                duration_micros,
                entities,
                outcome,
            } => {
                self.bootstrap_pages.fetch_add(1, Ordering::Relaxed);
                self.bootstrap_entities
                    .fetch_add(entities, Ordering::Relaxed);
                self.total_duration_micros
                    .fetch_add(duration_micros, Ordering::Relaxed);
                record_failure(&self.failures, outcome);
            }
            MetricEvent::ComputeOffload { .. } => {
                self.compute_offloads.fetch_add(1, Ordering::Relaxed);
            }
        }
    }
}

fn record_failure(counter: &AtomicU64, outcome: OutcomeKind) {
    if outcome != OutcomeKind::Success {
        counter.fetch_add(1, Ordering::Relaxed);
    }
}

const fn encode_optional_gauge(value: Option<u64>) -> u64 {
    match value {
        Some(value) => value.saturating_add(1),
        None => 0,
    }
}

const fn optional_gauge(value: u64) -> Option<u64> {
    value.checked_sub(1)
}

/// Point-in-time aggregate values.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct MetricsSnapshot {
    /// Replayable durable outbox depth.
    pub outbox_pending: u64,
    /// Age of the oldest replayable operation.
    pub oldest_pending_age_ms: Option<u64>,
    /// Unix timestamp of the latest successful complete drain.
    pub sync_last_success_unix_ms: Option<u64>,
    /// Durable unresolved-conflict count.
    pub conflicts_pending: u64,
    /// Client-side exchange attempts, including retry attempts.
    pub client_exchanges: u64,
    /// Server-side exchange requests.
    pub server_exchanges: u64,
    /// Transient and permanent failed events.
    pub failures: u64,
    /// Operations submitted across observed exchanges.
    pub operations: u64,
    /// Authoritative changes returned across observed exchanges.
    pub changes: u64,
    /// Conflicts returned across observed exchanges.
    pub conflicts: u64,
    /// Permanent operation rejections returned across observed exchanges.
    pub rejections: u64,
    /// Bootstrap page attempts, including retry attempts.
    pub bootstrap_pages: u64,
    /// Snapshot entities returned by observed pages.
    pub bootstrap_entities: u64,
    /// CPU workloads sent to dedicated compute pools.
    pub compute_offloads: u64,
    /// Aggregate duration for exchange and bootstrap events.
    pub total_duration_micros: u64,
    /// Exact framed request bytes observed at transport boundaries.
    pub uploaded_bytes: u64,
    /// Exact framed response bytes observed at transport boundaries.
    pub downloaded_bytes: u64,
    /// Aggregate structural-validation duration.
    pub validation_duration_micros: u64,
    /// Aggregate application execution duration.
    pub execution_duration_micros: u64,
    /// Aggregate authoritative persistence duration.
    pub database_duration_micros: u64,
}

/// Structured tracing observer that deliberately excludes domain payloads and IDs.
#[cfg(feature = "tracing")]
#[derive(Clone, Copy, Debug, Default)]
pub struct TracingObserver;

#[cfg(feature = "tracing")]
impl Observer for TracingObserver {
    fn record(&self, event: MetricEvent) {
        match event {
            MetricEvent::ClientState {
                outbox_pending,
                oldest_pending_age_ms,
                last_success_unix_ms,
                conflicts_pending,
            } => {
                tracing::info!(target: "aequora", event = "client_state", outbox_pending, ?oldest_pending_age_ms, ?last_success_unix_ms, conflicts_pending);
            }
            MetricEvent::ClientExchange {
                duration_micros,
                operations,
                changes,
                conflicts,
                rejections,
                outcome,
            } => {
                tracing::info!(target: "aequora", event = "client_exchange", duration_micros, operations, changes, conflicts, rejections, ?outcome);
            }
            MetricEvent::ServerExchange {
                duration_micros,
                operations,
                changes,
                conflicts,
                rejections,
                outcome,
            } => {
                tracing::info!(target: "aequora", event = "server_exchange", duration_micros, operations, changes, conflicts, rejections, ?outcome);
            }
            MetricEvent::TransportBytes {
                uploaded,
                downloaded,
            } => {
                tracing::info!(target: "aequora", event = "transport_bytes", uploaded, downloaded);
            }
            MetricEvent::ServerPhase {
                phase,
                duration_micros,
            } => {
                tracing::debug!(target: "aequora", event = "server_phase", ?phase, duration_micros);
            }
            MetricEvent::BootstrapPage {
                duration_micros,
                entities,
                outcome,
            } => {
                tracing::info!(target: "aequora", event = "bootstrap_page", duration_micros, entities, ?outcome);
            }
            MetricEvent::ComputeOffload { items } => {
                tracing::debug!(target: "aequora", event = "compute_offload", items);
            }
        }
    }

    fn record_with_context(&self, context: TraceContext, event: MetricEvent) {
        tracing::info_span!(
            target: "aequora",
            "sync_request",
            sync_session_id = %context.sync_session_id,
            request_id = %context.request_id,
            device_id = %context.device_id,
            tenant_id = %context.tenant_id,
        )
        .in_scope(|| self.record(event));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn atomic_metrics_aggregate_payload_free_events() {
        let metrics = AtomicMetrics::default();
        metrics.record(MetricEvent::ClientExchange {
            duration_micros: 50,
            operations: 2,
            changes: 3,
            conflicts: 1,
            rejections: 1,
            outcome: OutcomeKind::Success,
        });
        metrics.record(MetricEvent::ClientState {
            outbox_pending: 3,
            oldest_pending_age_ms: Some(500),
            last_success_unix_ms: Some(1_000),
            conflicts_pending: 2,
        });
        metrics.record(MetricEvent::BootstrapPage {
            duration_micros: 10,
            entities: 5,
            outcome: OutcomeKind::TransientFailure,
        });
        metrics.record(MetricEvent::TransportBytes {
            uploaded: 100,
            downloaded: 200,
        });
        metrics.record(MetricEvent::ServerPhase {
            phase: ServerPhaseKind::Database,
            duration_micros: 7,
        });
        let snapshot = metrics.snapshot();
        assert_eq!(snapshot.client_exchanges, 1);
        assert_eq!(snapshot.outbox_pending, 3);
        assert_eq!(snapshot.oldest_pending_age_ms, Some(500));
        assert_eq!(snapshot.sync_last_success_unix_ms, Some(1_000));
        assert_eq!(snapshot.conflicts_pending, 2);
        assert_eq!(snapshot.bootstrap_entities, 5);
        assert_eq!(snapshot.conflicts, 1);
        assert_eq!(snapshot.rejections, 1);
        assert_eq!(snapshot.failures, 1);
        assert_eq!(snapshot.total_duration_micros, 60);
        assert_eq!(snapshot.uploaded_bytes, 100);
        assert_eq!(snapshot.downloaded_bytes, 200);
        assert_eq!(snapshot.database_duration_micros, 7);
    }
}
