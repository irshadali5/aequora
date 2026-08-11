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

/// Result of one authoritative operation transaction attempt.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TransactionOutcomeKind {
    /// Entity, journal, operation result, and audit evidence committed together.
    Applied,
    /// A previously committed operation result was reused without another logical mutation.
    Duplicate,
    /// The authoritative version changed and the attempted mutation was rolled back.
    VersionChanged,
    /// Persistence failed; the adapter contract requires that no partial mutation remain.
    Failed,
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
    /// One authoritative operation transaction outcome.
    ServerTransaction {
        /// Payload-free commit, deduplication, rollback, or failure category.
        outcome: TransactionOutcomeKind,
    },
    /// One bootstrap page attempt.
    BootstrapPage {
        duration_micros: u64,
        entities: u64,
        outcome: OutcomeKind,
    },
    /// One bounded retry scheduled by the client.
    ClientRetry { delay_millis: u64 },
    /// Current scoped authoritative journal position distance after a served page.
    ServerJournalLag { sequences: u64 },
    /// Large CPU workload sent to the dedicated compute pool.
    ComputeOffload { items: u64 },
    /// Request rejected before execution because the server admission limit was saturated.
    ServerOverloaded,
    /// Request rejected because its authenticated tenant admission limit was saturated.
    ServerTenantOverloaded,
    /// Request rejected because its authenticated tenant exhausted its request-rate bucket.
    ServerTenantRateLimited,
    /// Admitted request cancelled because its body exceeded the receive deadline.
    ServerBodyReadTimedOut,
    /// Admitted request rejected because its compressed body exceeded the wire-byte limit.
    ServerBodyTooLarge,
    /// Admitted request cancelled after exceeding its configured execution deadline.
    ServerDeadlineExceeded,
    /// One bounded dependency-readiness probe completed.
    ServerReadiness { ready: bool },
    /// Point-in-time graceful-lifecycle state.
    ServerLifecycle { draining: bool, in_flight: u64 },
    /// New synchronization work rejected because graceful draining has begun.
    ServerDrainingRejected,
    /// One bounded graceful-drain attempt completed or reached its deadline.
    ServerDrainOutcome {
        duration_micros: u64,
        remaining: u64,
        timed_out: bool,
    },
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
    retries: AtomicU64,
    journal_lag: AtomicU64,
    compute_offloads: AtomicU64,
    overloaded_requests: AtomicU64,
    tenant_overloaded_requests: AtomicU64,
    tenant_rate_limited_requests: AtomicU64,
    body_read_timeouts: AtomicU64,
    oversized_request_bodies: AtomicU64,
    timed_out_requests: AtomicU64,
    readiness_checks: AtomicU64,
    readiness_failures: AtomicU64,
    server_draining: AtomicU64,
    server_in_flight: AtomicU64,
    draining_rejections: AtomicU64,
    drains_completed: AtomicU64,
    drains_timed_out: AtomicU64,
    drain_remaining: AtomicU64,
    total_duration_micros: AtomicU64,
    uploaded_bytes: AtomicU64,
    downloaded_bytes: AtomicU64,
    validation_duration_micros: AtomicU64,
    execution_duration_micros: AtomicU64,
    database_duration_micros: AtomicU64,
    transaction_commits: AtomicU64,
    transaction_rollbacks: AtomicU64,
    transaction_failures: AtomicU64,
    dedup_hits: AtomicU64,
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
            retries: self.retries.load(Ordering::Relaxed),
            journal_lag: self.journal_lag.load(Ordering::Relaxed),
            compute_offloads: self.compute_offloads.load(Ordering::Relaxed),
            overloaded_requests: self.overloaded_requests.load(Ordering::Relaxed),
            tenant_overloaded_requests: self.tenant_overloaded_requests.load(Ordering::Relaxed),
            tenant_rate_limited_requests: self.tenant_rate_limited_requests.load(Ordering::Relaxed),
            body_read_timeouts: self.body_read_timeouts.load(Ordering::Relaxed),
            oversized_request_bodies: self.oversized_request_bodies.load(Ordering::Relaxed),
            timed_out_requests: self.timed_out_requests.load(Ordering::Relaxed),
            readiness_checks: self.readiness_checks.load(Ordering::Relaxed),
            readiness_failures: self.readiness_failures.load(Ordering::Relaxed),
            server_draining: self.server_draining.load(Ordering::Relaxed) != 0,
            server_in_flight: self.server_in_flight.load(Ordering::Relaxed),
            draining_rejections: self.draining_rejections.load(Ordering::Relaxed),
            drains_completed: self.drains_completed.load(Ordering::Relaxed),
            drains_timed_out: self.drains_timed_out.load(Ordering::Relaxed),
            drain_remaining: self.drain_remaining.load(Ordering::Relaxed),
            total_duration_micros: self.total_duration_micros.load(Ordering::Relaxed),
            uploaded_bytes: self.uploaded_bytes.load(Ordering::Relaxed),
            downloaded_bytes: self.downloaded_bytes.load(Ordering::Relaxed),
            validation_duration_micros: self.validation_duration_micros.load(Ordering::Relaxed),
            execution_duration_micros: self.execution_duration_micros.load(Ordering::Relaxed),
            database_duration_micros: self.database_duration_micros.load(Ordering::Relaxed),
            transaction_commits: self.transaction_commits.load(Ordering::Relaxed),
            transaction_rollbacks: self.transaction_rollbacks.load(Ordering::Relaxed),
            transaction_failures: self.transaction_failures.load(Ordering::Relaxed),
            dedup_hits: self.dedup_hits.load(Ordering::Relaxed),
        }
    }

    fn record_client_state(
        &self,
        outbox_pending: u64,
        oldest_pending_age_ms: Option<u64>,
        last_success_unix_ms: Option<u64>,
        conflicts_pending: u64,
    ) {
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

    fn record_drain_outcome(&self, duration_micros: u64, remaining: u64, timed_out: bool) {
        self.total_duration_micros
            .fetch_add(duration_micros, Ordering::Relaxed);
        self.drain_remaining.store(remaining, Ordering::Relaxed);
        if timed_out {
            self.drains_timed_out.fetch_add(1, Ordering::Relaxed);
        } else {
            self.drains_completed.fetch_add(1, Ordering::Relaxed);
        }
    }

    fn record_exchange(
        &self,
        duration_micros: u64,
        operations: u64,
        changes: u64,
        conflicts: u64,
        rejections: u64,
        outcome: OutcomeKind,
    ) {
        self.operations.fetch_add(operations, Ordering::Relaxed);
        self.changes.fetch_add(changes, Ordering::Relaxed);
        self.conflicts.fetch_add(conflicts, Ordering::Relaxed);
        self.rejections.fetch_add(rejections, Ordering::Relaxed);
        self.total_duration_micros
            .fetch_add(duration_micros, Ordering::Relaxed);
        record_failure(&self.failures, outcome);
    }

    fn record_readiness(&self, ready: bool) {
        self.readiness_checks.fetch_add(1, Ordering::Relaxed);
        if !ready {
            self.readiness_failures.fetch_add(1, Ordering::Relaxed);
        }
    }

    fn record_lifecycle(&self, draining: bool, in_flight: u64) {
        self.server_draining
            .store(u64::from(draining), Ordering::Relaxed);
        self.server_in_flight.store(in_flight, Ordering::Relaxed);
    }

    fn record_server_phase(&self, phase: ServerPhaseKind, duration_micros: u64) {
        let counter = match phase {
            ServerPhaseKind::Validation => &self.validation_duration_micros,
            ServerPhaseKind::Execution => &self.execution_duration_micros,
            ServerPhaseKind::Database => &self.database_duration_micros,
        };
        counter.fetch_add(duration_micros, Ordering::Relaxed);
    }

    fn record_transaction(&self, outcome: TransactionOutcomeKind) {
        match outcome {
            TransactionOutcomeKind::Applied => increment(&self.transaction_commits),
            TransactionOutcomeKind::Duplicate => {
                increment(&self.transaction_commits);
                increment(&self.dedup_hits);
            }
            TransactionOutcomeKind::VersionChanged => increment(&self.transaction_rollbacks),
            TransactionOutcomeKind::Failed => increment(&self.transaction_failures),
        }
    }

    fn record_bootstrap(&self, duration_micros: u64, entities: u64, outcome: OutcomeKind) {
        self.bootstrap_pages.fetch_add(1, Ordering::Relaxed);
        self.bootstrap_entities
            .fetch_add(entities, Ordering::Relaxed);
        self.total_duration_micros
            .fetch_add(duration_micros, Ordering::Relaxed);
        record_failure(&self.failures, outcome);
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
            } => self.record_client_state(
                outbox_pending,
                oldest_pending_age_ms,
                last_success_unix_ms,
                conflicts_pending,
            ),
            MetricEvent::ClientExchange {
                duration_micros,
                operations,
                changes,
                conflicts,
                rejections,
                outcome,
            } => {
                self.client_exchanges.fetch_add(1, Ordering::Relaxed);
                self.record_exchange(
                    duration_micros,
                    operations,
                    changes,
                    conflicts,
                    rejections,
                    outcome,
                );
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
                self.record_exchange(
                    duration_micros,
                    operations,
                    changes,
                    conflicts,
                    rejections,
                    outcome,
                );
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
            } => self.record_server_phase(phase, duration_micros),
            MetricEvent::ServerTransaction { outcome } => self.record_transaction(outcome),
            MetricEvent::BootstrapPage {
                duration_micros,
                entities,
                outcome,
            } => self.record_bootstrap(duration_micros, entities, outcome),
            MetricEvent::ClientRetry { .. } => {
                self.retries.fetch_add(1, Ordering::Relaxed);
            }
            MetricEvent::ServerJournalLag { sequences } => {
                self.journal_lag.store(sequences, Ordering::Relaxed);
            }
            MetricEvent::ComputeOffload { .. } => {
                self.compute_offloads.fetch_add(1, Ordering::Relaxed);
            }
            MetricEvent::ServerOverloaded => increment(&self.overloaded_requests),
            MetricEvent::ServerTenantOverloaded => increment(&self.tenant_overloaded_requests),
            MetricEvent::ServerTenantRateLimited => {
                increment(&self.tenant_rate_limited_requests);
            }
            MetricEvent::ServerBodyReadTimedOut => increment(&self.body_read_timeouts),
            MetricEvent::ServerBodyTooLarge => increment(&self.oversized_request_bodies),
            MetricEvent::ServerDeadlineExceeded => increment(&self.timed_out_requests),
            MetricEvent::ServerReadiness { ready } => self.record_readiness(ready),
            MetricEvent::ServerLifecycle {
                draining,
                in_flight,
            } => self.record_lifecycle(draining, in_flight),
            MetricEvent::ServerDrainingRejected => increment(&self.draining_rejections),
            MetricEvent::ServerDrainOutcome {
                duration_micros,
                remaining,
                timed_out,
            } => self.record_drain_outcome(duration_micros, remaining, timed_out),
        }
    }
}

fn increment(counter: &AtomicU64) {
    counter.fetch_add(1, Ordering::Relaxed);
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
    /// Client retries scheduled after typed transient failures.
    pub retries: u64,
    /// Latest observed distance between a served scoped cursor and its journal head.
    pub journal_lag: u64,
    /// CPU workloads sent to dedicated compute pools.
    pub compute_offloads: u64,
    /// Requests rejected because all server admission permits were occupied.
    pub overloaded_requests: u64,
    /// Requests rejected because one tenant's admission permits were occupied.
    pub tenant_overloaded_requests: u64,
    /// Requests rejected because one tenant exhausted its request-rate bucket.
    pub tenant_rate_limited_requests: u64,
    /// Admitted requests cancelled after exceeding the body receive deadline.
    pub body_read_timeouts: u64,
    /// Admitted requests rejected after exceeding the compressed wire-byte limit.
    pub oversized_request_bodies: u64,
    /// Admitted requests cancelled after exceeding their execution deadline.
    pub timed_out_requests: u64,
    /// Dependency-readiness probes executed.
    pub readiness_checks: u64,
    /// Dependency-readiness probes that failed or exceeded their deadline.
    pub readiness_failures: u64,
    /// Whether the attached server lifecycle is draining.
    pub server_draining: bool,
    /// Requests currently admitted by the attached server lifecycle.
    pub server_in_flight: u64,
    /// New requests rejected after graceful draining began.
    pub draining_rejections: u64,
    /// Graceful drains that reached zero admitted requests.
    pub drains_completed: u64,
    /// Graceful drains that reached their deadline with work remaining.
    pub drains_timed_out: u64,
    /// Remaining admitted requests at the latest drain outcome.
    pub drain_remaining: u64,
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
    /// Authoritative operation transactions committed, including durable dedup lookups.
    pub transaction_commits: u64,
    /// Authoritative operation transactions rolled back after a version race.
    pub transaction_rollbacks: u64,
    /// Authoritative persistence attempts that failed and must leave no partial state.
    pub transaction_failures: u64,
    /// Previously committed operation results reused for at-least-once delivery.
    pub dedup_hits: u64,
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
            MetricEvent::ServerTransaction { outcome } => {
                tracing::debug!(target: "aequora", event = "server_transaction", ?outcome);
            }
            MetricEvent::BootstrapPage {
                duration_micros,
                entities,
                outcome,
            } => {
                tracing::info!(target: "aequora", event = "bootstrap_page", duration_micros, entities, ?outcome);
            }
            MetricEvent::ClientRetry { delay_millis } => {
                tracing::info!(target: "aequora", event = "client_retry", delay_millis);
            }
            MetricEvent::ServerJournalLag { sequences } => {
                tracing::debug!(target: "aequora", event = "server_journal_lag", sequences);
            }
            MetricEvent::ComputeOffload { items } => {
                tracing::debug!(target: "aequora", event = "compute_offload", items);
            }
            MetricEvent::ServerOverloaded => {
                tracing::warn!(target: "aequora", event = "server_overloaded");
            }
            MetricEvent::ServerTenantOverloaded => {
                tracing::warn!(target: "aequora", event = "server_tenant_overloaded");
            }
            MetricEvent::ServerTenantRateLimited => {
                tracing::warn!(target: "aequora", event = "server_tenant_rate_limited");
            }
            MetricEvent::ServerBodyReadTimedOut => {
                tracing::warn!(target: "aequora", event = "server_body_read_timed_out");
            }
            MetricEvent::ServerBodyTooLarge => {
                tracing::warn!(target: "aequora", event = "server_body_too_large");
            }
            MetricEvent::ServerDeadlineExceeded => {
                tracing::warn!(target: "aequora", event = "server_deadline_exceeded");
            }
            MetricEvent::ServerReadiness { ready } => {
                tracing::debug!(target: "aequora", event = "server_readiness", ready);
            }
            MetricEvent::ServerLifecycle {
                draining,
                in_flight,
            } => {
                tracing::debug!(target: "aequora", event = "server_lifecycle", draining, in_flight);
            }
            MetricEvent::ServerDrainingRejected => {
                tracing::info!(target: "aequora", event = "server_draining_rejected");
            }
            MetricEvent::ServerDrainOutcome {
                duration_micros,
                remaining,
                timed_out,
            } => {
                tracing::info!(target: "aequora", event = "server_drain_outcome", duration_micros, remaining, timed_out);
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
        metrics.record(MetricEvent::ServerTransaction {
            outcome: TransactionOutcomeKind::Applied,
        });
        metrics.record(MetricEvent::ServerTransaction {
            outcome: TransactionOutcomeKind::Duplicate,
        });
        metrics.record(MetricEvent::ServerTransaction {
            outcome: TransactionOutcomeKind::VersionChanged,
        });
        metrics.record(MetricEvent::ServerTransaction {
            outcome: TransactionOutcomeKind::Failed,
        });
        metrics.record(MetricEvent::ClientRetry { delay_millis: 500 });
        metrics.record(MetricEvent::ServerJournalLag { sequences: 9 });
        metrics.record(MetricEvent::ServerOverloaded);
        metrics.record(MetricEvent::ServerTenantOverloaded);
        metrics.record(MetricEvent::ServerTenantRateLimited);
        metrics.record(MetricEvent::ServerBodyReadTimedOut);
        metrics.record(MetricEvent::ServerBodyTooLarge);
        metrics.record(MetricEvent::ServerDeadlineExceeded);
        metrics.record(MetricEvent::ServerReadiness { ready: true });
        metrics.record(MetricEvent::ServerReadiness { ready: false });
        metrics.record(MetricEvent::ServerLifecycle {
            draining: true,
            in_flight: 2,
        });
        metrics.record(MetricEvent::ServerDrainingRejected);
        metrics.record(MetricEvent::ServerDrainOutcome {
            duration_micros: 11,
            remaining: 2,
            timed_out: true,
        });
        metrics.record(MetricEvent::ServerDrainOutcome {
            duration_micros: 7,
            remaining: 0,
            timed_out: false,
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
        assert_eq!(snapshot.total_duration_micros, 78);
        assert_eq!(snapshot.uploaded_bytes, 100);
        assert_eq!(snapshot.downloaded_bytes, 200);
        assert_eq!(snapshot.database_duration_micros, 7);
        assert_eq!(snapshot.transaction_commits, 2);
        assert_eq!(snapshot.transaction_rollbacks, 1);
        assert_eq!(snapshot.transaction_failures, 1);
        assert_eq!(snapshot.dedup_hits, 1);
        assert_eq!(snapshot.retries, 1);
        assert_eq!(snapshot.journal_lag, 9);
        assert_eq!(snapshot.overloaded_requests, 1);
        assert_eq!(snapshot.tenant_overloaded_requests, 1);
        assert_eq!(snapshot.tenant_rate_limited_requests, 1);
        assert_eq!(snapshot.body_read_timeouts, 1);
        assert_eq!(snapshot.oversized_request_bodies, 1);
        assert_eq!(snapshot.timed_out_requests, 1);
        assert_eq!(snapshot.readiness_checks, 2);
        assert_eq!(snapshot.readiness_failures, 1);
        assert!(snapshot.server_draining);
        assert_eq!(snapshot.server_in_flight, 2);
        assert_eq!(snapshot.draining_rejections, 1);
        assert_eq!(snapshot.drains_timed_out, 1);
        assert_eq!(snapshot.drains_completed, 1);
        assert_eq!(snapshot.drain_remaining, 0);
    }
}
