//! Payload-free metrics and optional structured tracing hooks.

mod part46;

pub use part46::*;

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

/// Stable anti-entropy verification and repair result category.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum IntegrityOutcomeKind {
    /// Local and authoritative roots matched at the requested boundary.
    Match,
    /// One or more bounded partitions differed and require repair planning.
    Mismatch,
    /// A bounded repair was applied and re-verification succeeded.
    Repaired,
    /// Unsafe divergence was isolated for operator review.
    Quarantined,
    /// Verification or repair failed before a safe conclusion was reached.
    Failed,
}

/// Payload-free durable local leadership lifecycle category.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LocalCoordinationEventKind {
    /// This process atomically acquired a new fencing epoch.
    Acquired,
    /// A heartbeat renewed the current epoch.
    Renewed,
    /// Renewal failed and leader-only work was cancelled.
    Lost,
    /// A stale or expired epoch was rejected by storage.
    StaleFenceRejected,
    /// The current owner released its lease for a graceful handoff.
    Released,
}

/// Payload-free client scheduler lifecycle category.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SchedulerEventKind {
    Selected,
    Deferred,
    BatchIncreased,
    OverloadBackoff,
}

/// Payload-free subscription/scope lifecycle category.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ScopeEventKind {
    Resolved,
    Expanded,
    Contracted,
    Revoked,
    TransitionFailed,
}

/// Payload-free live acceleration and ephemeral-presence lifecycle category.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LiveEventKind {
    Connected,
    Disconnected,
    HintPublished,
    HintReceived,
    HintCoalesced,
    HintDropped,
    BrokerError,
    FallbackPoll,
    PresenceUpdated,
    PresenceExpired,
}

/// Payload-free bulk migration lifecycle category.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ImportEventKind {
    Planned,
    BatchCommitted,
    BatchDuplicate,
    Quarantined,
    Verified,
    CutoverBlocked,
    Completed,
    Failed,
}

/// Payload-free large snapshot bootstrap lifecycle category.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LargeBootstrapEventKind {
    Started,
    Resumed,
    ChunkVerified,
    ChunkRetry,
    ActivationBlocked,
    Activated,
    Completed,
    Failed,
}

/// Payload-free consistency-profile registration and compatibility lifecycle category.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProfileEventKind {
    Registered,
    ManifestVerified,
    CapabilityRejected,
    CompatibilityRejected,
}

/// Payload-free deterministic execution and replay lifecycle category.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ReplayEventKind {
    BundleVerified,
    DecisionVerified,
    DecisionDiverged,
    HandlerVersionMismatch,
    Unsupported,
}

/// Payload-free canonical audit and explainability lifecycle category.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AuditEventKind {
    Committed,
    QueryAuthorized,
    QueryForbidden,
    IntegrityFailure,
    AnchorCreated,
}

/// Payload-free data-governance lifecycle category.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GovernanceEventKind {
    CandidatePlanned,
    Purged,
    Held,
    ErasureBlocked,
    PartiallyCompleted,
    Verified,
    RestoreBlocked,
}

/// Payload-free cryptographic lifecycle and verification category.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CryptoEventKind {
    ArtifactVerified,
    SignatureVerifyFailed,
    DecryptFailed,
    KeyRotated,
    KeyRevoked,
    PolicyRejected,
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
    /// One payload-free anti-entropy verification or repair outcome.
    IntegrityVerification {
        /// Wall duration in microseconds.
        duration_micros: u64,
        /// Number of bounded partitions compared.
        partitions_compared: u64,
        /// Number of partitions whose roots differed.
        mismatching_partitions: u64,
        /// Stable result used by metrics and alerts.
        outcome: IntegrityOutcomeKind,
    },
    /// One bounded local outbox compaction pass.
    QueueCompaction {
        /// Wall duration in microseconds.
        duration_micros: u64,
        /// Active queue rows inspected.
        operations_before: u64,
        /// Active queue rows after the transactional rewrite.
        operations_after: u64,
        /// Approximate serialized active-queue bytes removed.
        bytes_saved: u64,
        /// Whether planning or persistence failed.
        failed: bool,
    },
    /// One local rebase analysis/application pass.
    QueueRebase {
        /// Operations safely rewritten against the newer base.
        rewritten: u64,
        /// Operations left for normal conflict evaluation.
        conflicts: u64,
        /// Whether planning or persistence failed.
        failed: bool,
    },
    /// One durable local coordinator election or fencing event.
    LocalCoordination {
        /// Stable lifecycle category without store or process identifiers.
        kind: LocalCoordinationEventKind,
    },
    /// One scheduler selection, deferral, or controller adaptation.
    Scheduler {
        kind: SchedulerEventKind,
        /// Numeric `QoS` class rank; it is not business data.
        class_rank: u8,
        operations: u64,
        bytes: u64,
    },
    /// One authorized scope resolution or atomic local transition.
    ScopeTransition {
        kind: ScopeEventKind,
        entities: u64,
        active_scopes: u64,
    },
    /// One optional live-delivery or presence event. No tenant/scope/user labels are included.
    Live { kind: LiveEventKind, count: u64 },
    /// One bounded import/export lifecycle event without record values or source credentials.
    Import {
        kind: ImportEventKind,
        records: u64,
        quarantined: u64,
    },
    /// One bounded large-bootstrap lifecycle event without payloads, paths, or signed URLs.
    LargeBootstrap {
        kind: LargeBootstrapEventKind,
        bytes: u64,
        records: u64,
    },
    /// Profile registry or manifest validation without aggregate or operation names.
    Profile { kind: ProfileEventKind, count: u64 },
    /// Replay verification outcome without operation identifiers or domain payloads.
    Replay { kind: ReplayEventKind, count: u64 },
    /// Canonical audit lifecycle outcome without subject, actor, reason, or field labels.
    Audit { kind: AuditEventKind, count: u64 },
    /// Governance outcome without subject, policy, hold, or storage-surface identifiers.
    Governance {
        kind: GovernanceEventKind,
        count: u64,
    },
    /// Cryptographic outcome without key IDs, tenants, artifact names, or payloads.
    Crypto { kind: CryptoEventKind, count: u64 },
    /// Highest locally accepted signed key-registry generation.
    CryptoRegistryGeneration { generation: u64 },
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
    integrity_verifications: AtomicU64,
    integrity_partitions_compared: AtomicU64,
    integrity_mismatching_partitions: AtomicU64,
    integrity_repairs: AtomicU64,
    integrity_quarantines: AtomicU64,
    integrity_failures: AtomicU64,
    integrity_alerts: AtomicU64,
    queue_compactions: AtomicU64,
    queue_operations_removed: AtomicU64,
    queue_bytes_saved: AtomicU64,
    queue_compaction_failures: AtomicU64,
    queue_rebase_successes: AtomicU64,
    queue_rebase_conflicts: AtomicU64,
    local_leadership_acquired: AtomicU64,
    local_leadership_renewed: AtomicU64,
    local_leadership_lost: AtomicU64,
    local_stale_fences_rejected: AtomicU64,
    local_leadership_released: AtomicU64,
    scheduler_work_selected: AtomicU64,
    scheduler_work_deferred: AtomicU64,
    scheduler_batch_operations: AtomicU64,
    scheduler_batch_bytes: AtomicU64,
    scheduler_overload_backoffs: AtomicU64,
    scope_resolutions: AtomicU64,
    scope_expansions: AtomicU64,
    scope_contractions: AtomicU64,
    scope_revocations: AtomicU64,
    scope_transition_failures: AtomicU64,
    scope_transition_entities: AtomicU64,
    active_scope_count: AtomicU64,
    live_connections: AtomicU64,
    live_connects: AtomicU64,
    live_disconnects: AtomicU64,
    live_hints_published: AtomicU64,
    live_hints_received: AtomicU64,
    live_hints_coalesced: AtomicU64,
    live_hints_dropped: AtomicU64,
    live_broker_errors: AtomicU64,
    live_fallback_polls: AtomicU64,
    presence_updates: AtomicU64,
    presence_expired: AtomicU64,
    import_jobs_started: AtomicU64,
    import_jobs_completed: AtomicU64,
    import_jobs_failed: AtomicU64,
    import_batches_committed: AtomicU64,
    import_batches_deduplicated: AtomicU64,
    import_records_committed: AtomicU64,
    import_records_quarantined: AtomicU64,
    import_verifications: AtomicU64,
    import_cutovers_blocked: AtomicU64,
    large_bootstrap_started: AtomicU64,
    large_bootstrap_resumed: AtomicU64,
    large_bootstrap_completed: AtomicU64,
    large_bootstrap_failed: AtomicU64,
    large_bootstrap_chunks_verified: AtomicU64,
    large_bootstrap_chunk_retries: AtomicU64,
    large_bootstrap_bytes: AtomicU64,
    large_bootstrap_records: AtomicU64,
    large_bootstrap_activations_blocked: AtomicU64,
    profile_registrations: AtomicU64,
    profile_manifests_verified: AtomicU64,
    profile_capability_rejections: AtomicU64,
    profile_compatibility_rejections: AtomicU64,
    replay_bundles_verified: AtomicU64,
    replay_decisions_verified: AtomicU64,
    replay_divergences: AtomicU64,
    replay_handler_version_mismatches: AtomicU64,
    replay_unsupported: AtomicU64,
    audit_committed: AtomicU64,
    audit_queries_authorized: AtomicU64,
    audit_queries_forbidden: AtomicU64,
    audit_integrity_failures: AtomicU64,
    audit_anchors_created: AtomicU64,
    governance_candidates_planned: AtomicU64,
    governance_purged: AtomicU64,
    governance_held: AtomicU64,
    governance_erasure_blocked: AtomicU64,
    governance_partially_completed: AtomicU64,
    governance_verified: AtomicU64,
    governance_restore_blocked: AtomicU64,
    crypto_artifacts_verified: AtomicU64,
    crypto_signature_verify_failures: AtomicU64,
    crypto_decrypt_failures: AtomicU64,
    crypto_key_rotations: AtomicU64,
    crypto_key_revocations: AtomicU64,
    crypto_policy_rejections: AtomicU64,
    crypto_registry_generation: AtomicU64,
}

impl AtomicMetrics {
    /// Returns a point-in-time relaxed snapshot.
    #[must_use]
    #[allow(clippy::too_many_lines)]
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
            integrity_verifications: self.integrity_verifications.load(Ordering::Relaxed),
            integrity_partitions_compared: self
                .integrity_partitions_compared
                .load(Ordering::Relaxed),
            integrity_mismatching_partitions: self
                .integrity_mismatching_partitions
                .load(Ordering::Relaxed),
            integrity_repairs: self.integrity_repairs.load(Ordering::Relaxed),
            integrity_quarantines: self.integrity_quarantines.load(Ordering::Relaxed),
            integrity_failures: self.integrity_failures.load(Ordering::Relaxed),
            integrity_alerts: self.integrity_alerts.load(Ordering::Relaxed),
            queue_compactions: self.queue_compactions.load(Ordering::Relaxed),
            queue_operations_removed: self.queue_operations_removed.load(Ordering::Relaxed),
            queue_bytes_saved: self.queue_bytes_saved.load(Ordering::Relaxed),
            queue_compaction_failures: self.queue_compaction_failures.load(Ordering::Relaxed),
            queue_rebase_successes: self.queue_rebase_successes.load(Ordering::Relaxed),
            queue_rebase_conflicts: self.queue_rebase_conflicts.load(Ordering::Relaxed),
            local_leadership_acquired: self.local_leadership_acquired.load(Ordering::Relaxed),
            local_leadership_renewed: self.local_leadership_renewed.load(Ordering::Relaxed),
            local_leadership_lost: self.local_leadership_lost.load(Ordering::Relaxed),
            local_stale_fences_rejected: self.local_stale_fences_rejected.load(Ordering::Relaxed),
            local_leadership_released: self.local_leadership_released.load(Ordering::Relaxed),
            scheduler_work_selected: self.scheduler_work_selected.load(Ordering::Relaxed),
            scheduler_work_deferred: self.scheduler_work_deferred.load(Ordering::Relaxed),
            scheduler_batch_operations: self.scheduler_batch_operations.load(Ordering::Relaxed),
            scheduler_batch_bytes: self.scheduler_batch_bytes.load(Ordering::Relaxed),
            scheduler_overload_backoffs: self.scheduler_overload_backoffs.load(Ordering::Relaxed),
            scope_resolutions: self.scope_resolutions.load(Ordering::Relaxed),
            scope_expansions: self.scope_expansions.load(Ordering::Relaxed),
            scope_contractions: self.scope_contractions.load(Ordering::Relaxed),
            scope_revocations: self.scope_revocations.load(Ordering::Relaxed),
            scope_transition_failures: self.scope_transition_failures.load(Ordering::Relaxed),
            scope_transition_entities: self.scope_transition_entities.load(Ordering::Relaxed),
            active_scope_count: self.active_scope_count.load(Ordering::Relaxed),
            live_connections: self.live_connections.load(Ordering::Relaxed),
            live_connects: self.live_connects.load(Ordering::Relaxed),
            live_disconnects: self.live_disconnects.load(Ordering::Relaxed),
            live_hints_published: self.live_hints_published.load(Ordering::Relaxed),
            live_hints_received: self.live_hints_received.load(Ordering::Relaxed),
            live_hints_coalesced: self.live_hints_coalesced.load(Ordering::Relaxed),
            live_hints_dropped: self.live_hints_dropped.load(Ordering::Relaxed),
            live_broker_errors: self.live_broker_errors.load(Ordering::Relaxed),
            live_fallback_polls: self.live_fallback_polls.load(Ordering::Relaxed),
            presence_updates: self.presence_updates.load(Ordering::Relaxed),
            presence_expired: self.presence_expired.load(Ordering::Relaxed),
            import_jobs_started: self.import_jobs_started.load(Ordering::Relaxed),
            import_jobs_completed: self.import_jobs_completed.load(Ordering::Relaxed),
            import_jobs_failed: self.import_jobs_failed.load(Ordering::Relaxed),
            import_batches_committed: self.import_batches_committed.load(Ordering::Relaxed),
            import_batches_deduplicated: self.import_batches_deduplicated.load(Ordering::Relaxed),
            import_records_committed: self.import_records_committed.load(Ordering::Relaxed),
            import_records_quarantined: self.import_records_quarantined.load(Ordering::Relaxed),
            import_verifications: self.import_verifications.load(Ordering::Relaxed),
            import_cutovers_blocked: self.import_cutovers_blocked.load(Ordering::Relaxed),
            large_bootstrap_started: self.large_bootstrap_started.load(Ordering::Relaxed),
            large_bootstrap_resumed: self.large_bootstrap_resumed.load(Ordering::Relaxed),
            large_bootstrap_completed: self.large_bootstrap_completed.load(Ordering::Relaxed),
            large_bootstrap_failed: self.large_bootstrap_failed.load(Ordering::Relaxed),
            large_bootstrap_chunks_verified: self
                .large_bootstrap_chunks_verified
                .load(Ordering::Relaxed),
            large_bootstrap_chunk_retries: self
                .large_bootstrap_chunk_retries
                .load(Ordering::Relaxed),
            large_bootstrap_bytes: self.large_bootstrap_bytes.load(Ordering::Relaxed),
            large_bootstrap_records: self.large_bootstrap_records.load(Ordering::Relaxed),
            large_bootstrap_activations_blocked: self
                .large_bootstrap_activations_blocked
                .load(Ordering::Relaxed),
            profile_registrations: self.profile_registrations.load(Ordering::Relaxed),
            profile_manifests_verified: self.profile_manifests_verified.load(Ordering::Relaxed),
            profile_capability_rejections: self
                .profile_capability_rejections
                .load(Ordering::Relaxed),
            profile_compatibility_rejections: self
                .profile_compatibility_rejections
                .load(Ordering::Relaxed),
            replay_bundles_verified: self.replay_bundles_verified.load(Ordering::Relaxed),
            replay_decisions_verified: self.replay_decisions_verified.load(Ordering::Relaxed),
            replay_divergences: self.replay_divergences.load(Ordering::Relaxed),
            replay_handler_version_mismatches: self
                .replay_handler_version_mismatches
                .load(Ordering::Relaxed),
            replay_unsupported: self.replay_unsupported.load(Ordering::Relaxed),
            audit_committed: self.audit_committed.load(Ordering::Relaxed),
            audit_queries_authorized: self.audit_queries_authorized.load(Ordering::Relaxed),
            audit_queries_forbidden: self.audit_queries_forbidden.load(Ordering::Relaxed),
            audit_integrity_failures: self.audit_integrity_failures.load(Ordering::Relaxed),
            audit_anchors_created: self.audit_anchors_created.load(Ordering::Relaxed),
            governance_candidates_planned: self
                .governance_candidates_planned
                .load(Ordering::Relaxed),
            governance_purged: self.governance_purged.load(Ordering::Relaxed),
            governance_held: self.governance_held.load(Ordering::Relaxed),
            governance_erasure_blocked: self.governance_erasure_blocked.load(Ordering::Relaxed),
            governance_partially_completed: self
                .governance_partially_completed
                .load(Ordering::Relaxed),
            governance_verified: self.governance_verified.load(Ordering::Relaxed),
            governance_restore_blocked: self.governance_restore_blocked.load(Ordering::Relaxed),
            crypto_artifacts_verified: self.crypto_artifacts_verified.load(Ordering::Relaxed),
            crypto_signature_verify_failures: self
                .crypto_signature_verify_failures
                .load(Ordering::Relaxed),
            crypto_decrypt_failures: self.crypto_decrypt_failures.load(Ordering::Relaxed),
            crypto_key_rotations: self.crypto_key_rotations.load(Ordering::Relaxed),
            crypto_key_revocations: self.crypto_key_revocations.load(Ordering::Relaxed),
            crypto_policy_rejections: self.crypto_policy_rejections.load(Ordering::Relaxed),
            crypto_registry_generation: self.crypto_registry_generation.load(Ordering::Relaxed),
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

    fn record_integrity(
        &self,
        duration_micros: u64,
        partitions_compared: u64,
        mismatching_partitions: u64,
        outcome: IntegrityOutcomeKind,
    ) {
        increment(&self.integrity_verifications);
        self.integrity_partitions_compared
            .fetch_add(partitions_compared, Ordering::Relaxed);
        self.integrity_mismatching_partitions
            .fetch_add(mismatching_partitions, Ordering::Relaxed);
        self.total_duration_micros
            .fetch_add(duration_micros, Ordering::Relaxed);
        match outcome {
            IntegrityOutcomeKind::Match | IntegrityOutcomeKind::Mismatch => {}
            IntegrityOutcomeKind::Repaired => increment(&self.integrity_repairs),
            IntegrityOutcomeKind::Quarantined => {
                increment(&self.integrity_quarantines);
                increment(&self.integrity_alerts);
            }
            IntegrityOutcomeKind::Failed => {
                increment(&self.integrity_failures);
                increment(&self.integrity_alerts);
            }
        }
    }

    fn record_integrity_event(&self, event: MetricEvent) {
        if let MetricEvent::IntegrityVerification {
            duration_micros,
            partitions_compared,
            mismatching_partitions,
            outcome,
        } = event
        {
            self.record_integrity(
                duration_micros,
                partitions_compared,
                mismatching_partitions,
                outcome,
            );
        }
    }

    fn record_queue_event(&self, event: MetricEvent) {
        match event {
            MetricEvent::QueueCompaction {
                duration_micros,
                operations_before,
                operations_after,
                bytes_saved,
                failed,
            } => {
                increment(&self.queue_compactions);
                self.queue_operations_removed.fetch_add(
                    operations_before.saturating_sub(operations_after),
                    Ordering::Relaxed,
                );
                self.queue_bytes_saved
                    .fetch_add(bytes_saved, Ordering::Relaxed);
                self.total_duration_micros
                    .fetch_add(duration_micros, Ordering::Relaxed);
                if failed {
                    increment(&self.queue_compaction_failures);
                }
            }
            MetricEvent::QueueRebase {
                rewritten,
                conflicts,
                failed,
            } => {
                self.queue_rebase_successes
                    .fetch_add(rewritten, Ordering::Relaxed);
                self.queue_rebase_conflicts
                    .fetch_add(conflicts, Ordering::Relaxed);
                if failed {
                    increment(&self.queue_compaction_failures);
                }
            }
            _ => {}
        }
    }
}

impl Observer for AtomicMetrics {
    #[allow(clippy::too_many_lines)]
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
            event @ MetricEvent::IntegrityVerification { .. } => self.record_integrity_event(event),
            event @ (MetricEvent::QueueCompaction { .. } | MetricEvent::QueueRebase { .. }) => {
                self.record_queue_event(event);
            }
            MetricEvent::LocalCoordination { kind } => match kind {
                LocalCoordinationEventKind::Acquired => increment(&self.local_leadership_acquired),
                LocalCoordinationEventKind::Renewed => increment(&self.local_leadership_renewed),
                LocalCoordinationEventKind::Lost => increment(&self.local_leadership_lost),
                LocalCoordinationEventKind::StaleFenceRejected => {
                    increment(&self.local_stale_fences_rejected);
                }
                LocalCoordinationEventKind::Released => increment(&self.local_leadership_released),
            },
            MetricEvent::Scheduler {
                kind,
                operations,
                bytes,
                ..
            } => {
                match kind {
                    SchedulerEventKind::Selected | SchedulerEventKind::BatchIncreased => {
                        increment(&self.scheduler_work_selected);
                    }
                    SchedulerEventKind::Deferred => increment(&self.scheduler_work_deferred),
                    SchedulerEventKind::OverloadBackoff => {
                        increment(&self.scheduler_overload_backoffs);
                    }
                }
                self.scheduler_batch_operations
                    .fetch_add(operations, Ordering::Relaxed);
                self.scheduler_batch_bytes
                    .fetch_add(bytes, Ordering::Relaxed);
            }
            MetricEvent::ScopeTransition {
                kind,
                entities,
                active_scopes,
            } => {
                match kind {
                    ScopeEventKind::Resolved => increment(&self.scope_resolutions),
                    ScopeEventKind::Expanded => increment(&self.scope_expansions),
                    ScopeEventKind::Contracted => increment(&self.scope_contractions),
                    ScopeEventKind::Revoked => increment(&self.scope_revocations),
                    ScopeEventKind::TransitionFailed => {
                        increment(&self.scope_transition_failures);
                    }
                }
                self.scope_transition_entities
                    .fetch_add(entities, Ordering::Relaxed);
                self.active_scope_count
                    .store(active_scopes, Ordering::Relaxed);
            }
            MetricEvent::Live { kind, count } => match kind {
                LiveEventKind::Connected => {
                    self.live_connects.fetch_add(count, Ordering::Relaxed);
                    self.live_connections.fetch_add(count, Ordering::Relaxed);
                }
                LiveEventKind::Disconnected => {
                    self.live_disconnects.fetch_add(count, Ordering::Relaxed);
                    let _updated = self.live_connections.fetch_update(
                        Ordering::Relaxed,
                        Ordering::Relaxed,
                        |current| Some(current.saturating_sub(count)),
                    );
                }
                LiveEventKind::HintPublished => {
                    self.live_hints_published
                        .fetch_add(count, Ordering::Relaxed);
                }
                LiveEventKind::HintReceived => {
                    self.live_hints_received.fetch_add(count, Ordering::Relaxed);
                }
                LiveEventKind::HintCoalesced => {
                    self.live_hints_coalesced
                        .fetch_add(count, Ordering::Relaxed);
                }
                LiveEventKind::HintDropped => {
                    self.live_hints_dropped.fetch_add(count, Ordering::Relaxed);
                }
                LiveEventKind::BrokerError => {
                    self.live_broker_errors.fetch_add(count, Ordering::Relaxed);
                }
                LiveEventKind::FallbackPoll => {
                    self.live_fallback_polls.fetch_add(count, Ordering::Relaxed);
                }
                LiveEventKind::PresenceUpdated => {
                    self.presence_updates.fetch_add(count, Ordering::Relaxed);
                }
                LiveEventKind::PresenceExpired => {
                    self.presence_expired.fetch_add(count, Ordering::Relaxed);
                }
            },
            MetricEvent::Import {
                kind,
                records,
                quarantined,
            } => {
                match kind {
                    ImportEventKind::Planned => increment(&self.import_jobs_started),
                    ImportEventKind::BatchCommitted => {
                        increment(&self.import_batches_committed);
                        self.import_records_committed
                            .fetch_add(records, Ordering::Relaxed);
                    }
                    ImportEventKind::BatchDuplicate => {
                        increment(&self.import_batches_deduplicated);
                    }
                    ImportEventKind::Quarantined => {}
                    ImportEventKind::Verified => increment(&self.import_verifications),
                    ImportEventKind::CutoverBlocked => increment(&self.import_cutovers_blocked),
                    ImportEventKind::Completed => increment(&self.import_jobs_completed),
                    ImportEventKind::Failed => increment(&self.import_jobs_failed),
                }
                self.import_records_quarantined
                    .fetch_add(quarantined, Ordering::Relaxed);
            }
            MetricEvent::LargeBootstrap {
                kind,
                bytes,
                records,
            } => {
                match kind {
                    LargeBootstrapEventKind::Started => increment(&self.large_bootstrap_started),
                    LargeBootstrapEventKind::Resumed => increment(&self.large_bootstrap_resumed),
                    LargeBootstrapEventKind::ChunkVerified => {
                        increment(&self.large_bootstrap_chunks_verified);
                    }
                    LargeBootstrapEventKind::ChunkRetry => {
                        increment(&self.large_bootstrap_chunk_retries);
                    }
                    LargeBootstrapEventKind::ActivationBlocked => {
                        increment(&self.large_bootstrap_activations_blocked);
                    }
                    LargeBootstrapEventKind::Activated => {}
                    LargeBootstrapEventKind::Completed => {
                        increment(&self.large_bootstrap_completed);
                    }
                    LargeBootstrapEventKind::Failed => increment(&self.large_bootstrap_failed),
                }
                self.large_bootstrap_bytes
                    .fetch_add(bytes, Ordering::Relaxed);
                self.large_bootstrap_records
                    .fetch_add(records, Ordering::Relaxed);
            }
            MetricEvent::Profile { kind, count } => match kind {
                ProfileEventKind::Registered => {
                    self.profile_registrations
                        .fetch_add(count, Ordering::Relaxed);
                }
                ProfileEventKind::ManifestVerified => {
                    self.profile_manifests_verified
                        .fetch_add(count, Ordering::Relaxed);
                }
                ProfileEventKind::CapabilityRejected => {
                    self.profile_capability_rejections
                        .fetch_add(count, Ordering::Relaxed);
                }
                ProfileEventKind::CompatibilityRejected => {
                    self.profile_compatibility_rejections
                        .fetch_add(count, Ordering::Relaxed);
                }
            },
            MetricEvent::Replay { kind, count } => match kind {
                ReplayEventKind::BundleVerified => {
                    self.replay_bundles_verified
                        .fetch_add(count, Ordering::Relaxed);
                }
                ReplayEventKind::DecisionVerified => {
                    self.replay_decisions_verified
                        .fetch_add(count, Ordering::Relaxed);
                }
                ReplayEventKind::DecisionDiverged => {
                    self.replay_divergences.fetch_add(count, Ordering::Relaxed);
                }
                ReplayEventKind::HandlerVersionMismatch => {
                    self.replay_handler_version_mismatches
                        .fetch_add(count, Ordering::Relaxed);
                }
                ReplayEventKind::Unsupported => {
                    self.replay_unsupported.fetch_add(count, Ordering::Relaxed);
                }
            },
            MetricEvent::Audit { kind, count } => match kind {
                AuditEventKind::Committed => {
                    self.audit_committed.fetch_add(count, Ordering::Relaxed);
                }
                AuditEventKind::QueryAuthorized => {
                    self.audit_queries_authorized
                        .fetch_add(count, Ordering::Relaxed);
                }
                AuditEventKind::QueryForbidden => {
                    self.audit_queries_forbidden
                        .fetch_add(count, Ordering::Relaxed);
                }
                AuditEventKind::IntegrityFailure => {
                    self.audit_integrity_failures
                        .fetch_add(count, Ordering::Relaxed);
                }
                AuditEventKind::AnchorCreated => {
                    self.audit_anchors_created
                        .fetch_add(count, Ordering::Relaxed);
                }
            },
            MetricEvent::Governance { kind, count } => match kind {
                GovernanceEventKind::CandidatePlanned => {
                    self.governance_candidates_planned
                        .fetch_add(count, Ordering::Relaxed);
                }
                GovernanceEventKind::Purged => {
                    self.governance_purged.fetch_add(count, Ordering::Relaxed);
                }
                GovernanceEventKind::Held => {
                    self.governance_held.fetch_add(count, Ordering::Relaxed);
                }
                GovernanceEventKind::ErasureBlocked => {
                    self.governance_erasure_blocked
                        .fetch_add(count, Ordering::Relaxed);
                }
                GovernanceEventKind::PartiallyCompleted => {
                    self.governance_partially_completed
                        .fetch_add(count, Ordering::Relaxed);
                }
                GovernanceEventKind::Verified => {
                    self.governance_verified.fetch_add(count, Ordering::Relaxed);
                }
                GovernanceEventKind::RestoreBlocked => {
                    self.governance_restore_blocked
                        .fetch_add(count, Ordering::Relaxed);
                }
            },
            MetricEvent::Crypto { kind, count } => match kind {
                CryptoEventKind::ArtifactVerified => {
                    self.crypto_artifacts_verified
                        .fetch_add(count, Ordering::Relaxed);
                }
                CryptoEventKind::SignatureVerifyFailed => {
                    self.crypto_signature_verify_failures
                        .fetch_add(count, Ordering::Relaxed);
                }
                CryptoEventKind::DecryptFailed => {
                    self.crypto_decrypt_failures
                        .fetch_add(count, Ordering::Relaxed);
                }
                CryptoEventKind::KeyRotated => {
                    self.crypto_key_rotations
                        .fetch_add(count, Ordering::Relaxed);
                }
                CryptoEventKind::KeyRevoked => {
                    self.crypto_key_revocations
                        .fetch_add(count, Ordering::Relaxed);
                }
                CryptoEventKind::PolicyRejected => {
                    self.crypto_policy_rejections
                        .fetch_add(count, Ordering::Relaxed);
                }
            },
            MetricEvent::CryptoRegistryGeneration { generation } => {
                self.crypto_registry_generation
                    .fetch_max(generation, Ordering::Relaxed);
            }
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
    /// Anti-entropy verification and repair outcome events.
    pub integrity_verifications: u64,
    /// Bounded partitions compared across integrity events.
    pub integrity_partitions_compared: u64,
    /// Partitions whose roots differed across integrity events.
    pub integrity_mismatching_partitions: u64,
    /// Bounded repairs that completed and re-verified.
    pub integrity_repairs: u64,
    /// Unsafe divergences quarantined for operator review.
    pub integrity_quarantines: u64,
    /// Verification or repair attempts that failed.
    pub integrity_failures: u64,
    /// Alert-worthy quarantine and failure events.
    pub integrity_alerts: u64,
    /// Bounded local queue compaction passes.
    pub queue_compactions: u64,
    /// Local-only operations removed by declared semantic policy.
    pub queue_operations_removed: u64,
    /// Approximate active-queue bytes saved by compaction.
    pub queue_bytes_saved: u64,
    /// Compaction or rebase passes requiring attention.
    pub queue_compaction_failures: u64,
    /// Operations safely rebased against a newer authority version.
    pub queue_rebase_successes: u64,
    /// Operations left for normal conflict handling by rebase analysis.
    pub queue_rebase_conflicts: u64,
    /// Durable local leadership epochs acquired by this process.
    pub local_leadership_acquired: u64,
    /// Successful local leadership heartbeat renewals.
    pub local_leadership_renewed: u64,
    /// Leadership losses that cancelled leader-only work.
    pub local_leadership_lost: u64,
    /// Stale fenced commits rejected before mutation.
    pub local_stale_fences_rejected: u64,
    /// Graceful lease releases.
    pub local_leadership_released: u64,
    /// Eligible work items selected by the local scheduler.
    pub scheduler_work_selected: u64,
    /// Work items retained durably because an eligibility constraint applied.
    pub scheduler_work_deferred: u64,
    /// Aggregate operation capacity selected by scheduler decisions.
    pub scheduler_batch_operations: u64,
    /// Aggregate byte capacity selected by scheduler decisions.
    pub scheduler_batch_bytes: u64,
    /// Overload observations that reduced work and opened backoff.
    pub scheduler_overload_backoffs: u64,
    /// Successful server scope resolutions.
    pub scope_resolutions: u64,
    /// Atomic local scope expansions.
    pub scope_expansions: u64,
    /// Atomic local scope contractions.
    pub scope_contractions: u64,
    /// Logical scope revocations applied locally.
    pub scope_revocations: u64,
    /// Scope transitions that failed before activation.
    pub scope_transition_failures: u64,
    /// Aggregate membership entities affected by transitions.
    pub scope_transition_entities: u64,
    /// Last observed active subscription count.
    pub active_scope_count: u64,
    /// Current node-local live connections.
    pub live_connections: u64,
    /// Accepted live connections.
    pub live_connects: u64,
    /// Closed live connections.
    pub live_disconnects: u64,
    /// Best-effort hints offered to fan-out.
    pub live_hints_published: u64,
    /// Hints accepted by clients.
    pub live_hints_received: u64,
    /// Redundant hints collapsed.
    pub live_hints_coalesced: u64,
    /// Hints dropped within configured bounds.
    pub live_hints_dropped: u64,
    /// Optional broker failures.
    pub live_broker_errors: u64,
    /// Safety polls executed independently of live delivery.
    pub live_fallback_polls: u64,
    /// Ephemeral presence refreshes.
    pub presence_updates: u64,
    /// Ephemeral presence records expired.
    pub presence_expired: u64,
    /// Import jobs planned.
    pub import_jobs_started: u64,
    /// Import jobs completing cutover.
    pub import_jobs_completed: u64,
    /// Import jobs entering a failed terminal attempt.
    pub import_jobs_failed: u64,
    /// Target batches atomically committed with checkpoints.
    pub import_batches_committed: u64,
    /// Retried batches recognized without another effect.
    pub import_batches_deduplicated: u64,
    /// Canonical records committed by import batches.
    pub import_records_committed: u64,
    /// Invalid records durably quarantined.
    pub import_records_quarantined: u64,
    /// Completed global verification passes.
    pub import_verifications: u64,
    /// Cutover attempts blocked by missing evidence.
    pub import_cutovers_blocked: u64,
    /// Large bootstrap jobs started.
    pub large_bootstrap_started: u64,
    /// Large bootstrap jobs resumed from durable progress.
    pub large_bootstrap_resumed: u64,
    /// Large bootstrap jobs completed through delta catch-up.
    pub large_bootstrap_completed: u64,
    /// Large bootstrap jobs failing an attempt.
    pub large_bootstrap_failed: u64,
    /// Chunks whose exact digest and structure were verified.
    pub large_bootstrap_chunks_verified: u64,
    /// Bounded chunk retries.
    pub large_bootstrap_chunk_retries: u64,
    /// Encoded snapshot bytes processed.
    pub large_bootstrap_bytes: u64,
    /// Snapshot records installed.
    pub large_bootstrap_records: u64,
    /// Activation attempts blocked by incomplete or stale evidence.
    pub large_bootstrap_activations_blocked: u64,
    /// Aggregate and operation profile declarations registered.
    pub profile_registrations: u64,
    /// Profile manifests passing structural and digest verification.
    pub profile_manifests_verified: u64,
    /// Startup validations rejected for missing adapter capabilities.
    pub profile_capability_rejections: u64,
    /// Successor manifests rejected for unversioned semantic drift.
    pub profile_compatibility_rejections: u64,
    /// Replay bundles passing integrity and input validation.
    pub replay_bundles_verified: u64,
    /// Handler decisions matching their captured expected plan.
    pub replay_decisions_verified: u64,
    /// Handler decisions diverging from captured semantics.
    pub replay_divergences: u64,
    /// Replays rejected because handler semantic versions differed.
    pub replay_handler_version_mismatches: u64,
    /// Operations explicitly reported as outside replay support.
    pub replay_unsupported: u64,
    pub audit_committed: u64,
    pub audit_queries_authorized: u64,
    pub audit_queries_forbidden: u64,
    pub audit_integrity_failures: u64,
    pub audit_anchors_created: u64,
    pub governance_candidates_planned: u64,
    pub governance_purged: u64,
    pub governance_held: u64,
    pub governance_erasure_blocked: u64,
    pub governance_partially_completed: u64,
    pub governance_verified: u64,
    pub governance_restore_blocked: u64,
    pub crypto_artifacts_verified: u64,
    pub crypto_signature_verify_failures: u64,
    pub crypto_decrypt_failures: u64,
    pub crypto_key_rotations: u64,
    pub crypto_key_revocations: u64,
    pub crypto_policy_rejections: u64,
    pub crypto_registry_generation: u64,
}

/// Structured tracing observer that deliberately excludes domain payloads and IDs.
#[cfg(feature = "tracing")]
#[derive(Clone, Copy, Debug, Default)]
pub struct TracingObserver;

#[cfg(feature = "tracing")]
fn trace_integrity_event(event: MetricEvent) {
    if let MetricEvent::IntegrityVerification {
        duration_micros,
        partitions_compared,
        mismatching_partitions,
        outcome,
    } = event
    {
        match outcome {
            IntegrityOutcomeKind::Quarantined | IntegrityOutcomeKind::Failed => {
                tracing::warn!(target: "aequora", event = "integrity_verification", duration_micros, partitions_compared, mismatching_partitions, ?outcome);
            }
            _ => {
                tracing::info!(target: "aequora", event = "integrity_verification", duration_micros, partitions_compared, mismatching_partitions, ?outcome);
            }
        }
    }
}

#[cfg(feature = "tracing")]
fn trace_queue_event(event: MetricEvent) {
    match event {
        MetricEvent::QueueCompaction {
            duration_micros,
            operations_before,
            operations_after,
            bytes_saved,
            failed,
        } => {
            tracing::info!(target: "aequora", event = "queue_compaction", duration_micros, operations_before, operations_after, bytes_saved, failed);
        }
        MetricEvent::QueueRebase {
            rewritten,
            conflicts,
            failed,
        } => {
            tracing::info!(target: "aequora", event = "queue_rebase", rewritten, conflicts, failed);
        }
        _ => {}
    }
}

#[cfg(feature = "tracing")]
fn trace_server_drain_outcome(duration_micros: u64, remaining: u64, timed_out: bool) {
    tracing::info!(target: "aequora", event = "server_drain_outcome", duration_micros, remaining, timed_out);
}

#[cfg(feature = "tracing")]
impl Observer for TracingObserver {
    #[allow(clippy::too_many_lines)]
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
            } => trace_server_drain_outcome(duration_micros, remaining, timed_out),
            event @ MetricEvent::IntegrityVerification { .. } => trace_integrity_event(event),
            event @ (MetricEvent::QueueCompaction { .. } | MetricEvent::QueueRebase { .. }) => {
                trace_queue_event(event);
            }
            MetricEvent::LocalCoordination { kind } => {
                tracing::info!(target: "aequora", event = "local_coordination", ?kind);
            }
            MetricEvent::Scheduler {
                kind,
                class_rank,
                operations,
                bytes,
            } => {
                tracing::info!(target: "aequora", event = "scheduler", ?kind, class_rank, operations, bytes);
            }
            MetricEvent::ScopeTransition {
                kind,
                entities,
                active_scopes,
            } => {
                tracing::info!(target: "aequora", event = "scope_transition", ?kind, entities, active_scopes);
            }
            MetricEvent::Live { kind, count } => {
                tracing::info!(target: "aequora", event = "live", ?kind, count);
            }
            MetricEvent::Import {
                kind,
                records,
                quarantined,
            } => {
                tracing::info!(target: "aequora", event = "import", ?kind, records, quarantined);
            }
            MetricEvent::LargeBootstrap {
                kind,
                bytes,
                records,
            } => {
                tracing::info!(target: "aequora", event = "large_bootstrap", ?kind, bytes, records);
            }
            MetricEvent::Profile { kind, count } => {
                tracing::info!(target: "aequora", event = "profile", ?kind, count);
            }
            MetricEvent::Replay { kind, count } => {
                tracing::info!(target: "aequora", event = "replay", ?kind, count);
            }
            MetricEvent::Audit { kind, count } => {
                tracing::info!(target: "aequora", event = "audit", ?kind, count);
            }
            MetricEvent::Governance { kind, count } => {
                tracing::info!(target: "aequora", event = "governance", ?kind, count);
            }
            MetricEvent::Crypto { kind, count } => match kind {
                CryptoEventKind::SignatureVerifyFailed
                | CryptoEventKind::DecryptFailed
                | CryptoEventKind::PolicyRejected => {
                    tracing::warn!(target: "aequora", event = "crypto", ?kind, count);
                }
                _ => tracing::info!(target: "aequora", event = "crypto", ?kind, count),
            },
            MetricEvent::CryptoRegistryGeneration { generation } => {
                tracing::info!(target: "aequora", event = "crypto_registry", generation);
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

    fn record_quarantined_integrity(metrics: &AtomicMetrics) {
        metrics.record(MetricEvent::IntegrityVerification {
            duration_micros: 13,
            partitions_compared: 32,
            mismatching_partitions: 1,
            outcome: IntegrityOutcomeKind::Quarantined,
        });
    }

    fn assert_integrity_snapshot(snapshot: &MetricsSnapshot) {
        assert_eq!(snapshot.total_duration_micros, 104);
        assert_eq!(snapshot.integrity_verifications, 1);
        assert_eq!(snapshot.integrity_partitions_compared, 32);
        assert_eq!(snapshot.integrity_mismatching_partitions, 1);
        assert_eq!(snapshot.integrity_quarantines, 1);
        assert_eq!(snapshot.integrity_alerts, 1);
    }

    fn record_queue_metrics(metrics: &AtomicMetrics) {
        metrics.record(MetricEvent::QueueCompaction {
            duration_micros: 13,
            operations_before: 8,
            operations_after: 3,
            bytes_saved: 144,
            failed: false,
        });
        metrics.record(MetricEvent::QueueRebase {
            rewritten: 2,
            conflicts: 1,
            failed: true,
        });
    }

    fn assert_queue_snapshot(snapshot: &MetricsSnapshot) {
        assert_eq!(snapshot.queue_compactions, 1);
        assert_eq!(snapshot.queue_operations_removed, 5);
        assert_eq!(snapshot.queue_bytes_saved, 144);
        assert_eq!(snapshot.queue_compaction_failures, 1);
        assert_eq!(snapshot.queue_rebase_successes, 2);
        assert_eq!(snapshot.queue_rebase_conflicts, 1);
    }

    fn record_drain_metrics(metrics: &AtomicMetrics) {
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
    }

    #[test]
    #[allow(clippy::too_many_lines)]
    fn atomic_metrics_aggregate_payload_free_events() {
        let metrics = AtomicMetrics::default();
        metrics.record(MetricEvent::LocalCoordination {
            kind: LocalCoordinationEventKind::Acquired,
        });
        metrics.record(MetricEvent::Scheduler {
            kind: SchedulerEventKind::Selected,
            class_rank: 4,
            operations: 8,
            bytes: 1_024,
        });
        metrics.record(MetricEvent::Scheduler {
            kind: SchedulerEventKind::Deferred,
            class_rank: 1,
            operations: 0,
            bytes: 0,
        });
        metrics.record(MetricEvent::Scheduler {
            kind: SchedulerEventKind::OverloadBackoff,
            class_rank: 3,
            operations: 4,
            bytes: 512,
        });
        metrics.record(MetricEvent::ScopeTransition {
            kind: ScopeEventKind::Contracted,
            entities: 3,
            active_scopes: 2,
        });
        metrics.record(MetricEvent::Live {
            kind: LiveEventKind::Connected,
            count: 2,
        });
        metrics.record(MetricEvent::Live {
            kind: LiveEventKind::HintCoalesced,
            count: 4,
        });
        metrics.record(MetricEvent::Live {
            kind: LiveEventKind::Disconnected,
            count: 1,
        });
        metrics.record(MetricEvent::Live {
            kind: LiveEventKind::PresenceExpired,
            count: 3,
        });
        metrics.record(MetricEvent::Import {
            kind: ImportEventKind::Planned,
            records: 0,
            quarantined: 0,
        });
        metrics.record(MetricEvent::Import {
            kind: ImportEventKind::BatchCommitted,
            records: 50,
            quarantined: 2,
        });
        metrics.record(MetricEvent::Import {
            kind: ImportEventKind::CutoverBlocked,
            records: 0,
            quarantined: 0,
        });
        metrics.record(MetricEvent::Profile {
            kind: ProfileEventKind::Registered,
            count: 3,
        });
        metrics.record(MetricEvent::Profile {
            kind: ProfileEventKind::ManifestVerified,
            count: 1,
        });
        metrics.record(MetricEvent::Profile {
            kind: ProfileEventKind::CapabilityRejected,
            count: 2,
        });
        metrics.record(MetricEvent::Profile {
            kind: ProfileEventKind::CompatibilityRejected,
            count: 1,
        });
        metrics.record(MetricEvent::Replay {
            kind: ReplayEventKind::BundleVerified,
            count: 2,
        });
        metrics.record(MetricEvent::Replay {
            kind: ReplayEventKind::DecisionVerified,
            count: 1,
        });
        metrics.record(MetricEvent::Replay {
            kind: ReplayEventKind::DecisionDiverged,
            count: 1,
        });
        metrics.record(MetricEvent::Replay {
            kind: ReplayEventKind::HandlerVersionMismatch,
            count: 1,
        });
        metrics.record(MetricEvent::Audit {
            kind: AuditEventKind::Committed,
            count: 3,
        });
        metrics.record(MetricEvent::Audit {
            kind: AuditEventKind::QueryAuthorized,
            count: 2,
        });
        metrics.record(MetricEvent::Audit {
            kind: AuditEventKind::QueryForbidden,
            count: 1,
        });
        metrics.record(MetricEvent::Audit {
            kind: AuditEventKind::IntegrityFailure,
            count: 1,
        });
        metrics.record(MetricEvent::Audit {
            kind: AuditEventKind::AnchorCreated,
            count: 1,
        });
        metrics.record(MetricEvent::Governance {
            kind: GovernanceEventKind::CandidatePlanned,
            count: 7,
        });
        metrics.record(MetricEvent::Governance {
            kind: GovernanceEventKind::Purged,
            count: 5,
        });
        metrics.record(MetricEvent::Governance {
            kind: GovernanceEventKind::Held,
            count: 2,
        });
        metrics.record(MetricEvent::Governance {
            kind: GovernanceEventKind::ErasureBlocked,
            count: 1,
        });
        metrics.record(MetricEvent::Governance {
            kind: GovernanceEventKind::PartiallyCompleted,
            count: 1,
        });
        metrics.record(MetricEvent::Governance {
            kind: GovernanceEventKind::Verified,
            count: 4,
        });
        metrics.record(MetricEvent::Governance {
            kind: GovernanceEventKind::RestoreBlocked,
            count: 1,
        });
        metrics.record(MetricEvent::Crypto {
            kind: CryptoEventKind::ArtifactVerified,
            count: 3,
        });
        metrics.record(MetricEvent::Crypto {
            kind: CryptoEventKind::SignatureVerifyFailed,
            count: 2,
        });
        metrics.record(MetricEvent::Crypto {
            kind: CryptoEventKind::DecryptFailed,
            count: 1,
        });
        metrics.record(MetricEvent::Crypto {
            kind: CryptoEventKind::KeyRotated,
            count: 1,
        });
        metrics.record(MetricEvent::Crypto {
            kind: CryptoEventKind::KeyRevoked,
            count: 1,
        });
        metrics.record(MetricEvent::Crypto {
            kind: CryptoEventKind::PolicyRejected,
            count: 1,
        });
        metrics.record(MetricEvent::CryptoRegistryGeneration { generation: 8 });
        metrics.record(MetricEvent::LocalCoordination {
            kind: LocalCoordinationEventKind::Renewed,
        });
        metrics.record(MetricEvent::LocalCoordination {
            kind: LocalCoordinationEventKind::Lost,
        });
        metrics.record(MetricEvent::LocalCoordination {
            kind: LocalCoordinationEventKind::StaleFenceRejected,
        });
        metrics.record(MetricEvent::LocalCoordination {
            kind: LocalCoordinationEventKind::Released,
        });
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
        record_drain_metrics(&metrics);
        record_queue_metrics(&metrics);
        record_quarantined_integrity(&metrics);
        let snapshot = metrics.snapshot();
        assert_eq!(snapshot.local_leadership_acquired, 1);
        assert_eq!(snapshot.local_leadership_renewed, 1);
        assert_eq!(snapshot.local_leadership_lost, 1);
        assert_eq!(snapshot.local_stale_fences_rejected, 1);
        assert_eq!(snapshot.local_leadership_released, 1);
        assert_eq!(snapshot.scheduler_work_selected, 1);
        assert_eq!(snapshot.scheduler_work_deferred, 1);
        assert_eq!(snapshot.scheduler_batch_operations, 12);
        assert_eq!(snapshot.scheduler_batch_bytes, 1_536);
        assert_eq!(snapshot.scheduler_overload_backoffs, 1);
        assert_eq!(snapshot.scope_contractions, 1);
        assert_eq!(snapshot.scope_transition_entities, 3);
        assert_eq!(snapshot.active_scope_count, 2);
        assert_eq!(snapshot.live_connections, 1);
        assert_eq!(snapshot.live_connects, 2);
        assert_eq!(snapshot.live_disconnects, 1);
        assert_eq!(snapshot.live_hints_coalesced, 4);
        assert_eq!(snapshot.presence_expired, 3);
        assert_eq!(snapshot.import_jobs_started, 1);
        assert_eq!(snapshot.import_batches_committed, 1);
        assert_eq!(snapshot.import_records_committed, 50);
        assert_eq!(snapshot.import_records_quarantined, 2);
        assert_eq!(snapshot.import_cutovers_blocked, 1);
        assert_eq!(snapshot.profile_registrations, 3);
        assert_eq!(snapshot.profile_manifests_verified, 1);
        assert_eq!(snapshot.profile_capability_rejections, 2);
        assert_eq!(snapshot.profile_compatibility_rejections, 1);
        assert_eq!(snapshot.replay_bundles_verified, 2);
        assert_eq!(snapshot.replay_decisions_verified, 1);
        assert_eq!(snapshot.replay_divergences, 1);
        assert_eq!(snapshot.replay_handler_version_mismatches, 1);
        assert_eq!(snapshot.audit_committed, 3);
        assert_eq!(snapshot.audit_queries_authorized, 2);
        assert_eq!(snapshot.audit_queries_forbidden, 1);
        assert_eq!(snapshot.audit_integrity_failures, 1);
        assert_eq!(snapshot.audit_anchors_created, 1);
        assert_eq!(snapshot.governance_candidates_planned, 7);
        assert_eq!(snapshot.governance_purged, 5);
        assert_eq!(snapshot.governance_held, 2);
        assert_eq!(snapshot.governance_erasure_blocked, 1);
        assert_eq!(snapshot.governance_partially_completed, 1);
        assert_eq!(snapshot.governance_verified, 4);
        assert_eq!(snapshot.governance_restore_blocked, 1);
        assert_eq!(snapshot.crypto_artifacts_verified, 3);
        assert_eq!(snapshot.crypto_signature_verify_failures, 2);
        assert_eq!(snapshot.crypto_decrypt_failures, 1);
        assert_eq!(snapshot.crypto_key_rotations, 1);
        assert_eq!(snapshot.crypto_key_revocations, 1);
        assert_eq!(snapshot.crypto_policy_rejections, 1);
        assert_eq!(snapshot.crypto_registry_generation, 8);
        assert_eq!(snapshot.client_exchanges, 1);
        assert_eq!(snapshot.outbox_pending, 3);
        assert_eq!(snapshot.oldest_pending_age_ms, Some(500));
        assert_eq!(snapshot.sync_last_success_unix_ms, Some(1_000));
        assert_eq!(snapshot.conflicts_pending, 2);
        assert_eq!(snapshot.bootstrap_entities, 5);
        assert_eq!(snapshot.conflicts, 1);
        assert_eq!(snapshot.rejections, 1);
        assert_eq!(snapshot.failures, 1);
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
        assert_queue_snapshot(&snapshot);
        assert_integrity_snapshot(&snapshot);
    }
}
