//! Authoritative sync-session orchestration.

use aequora_clock::Clock;
use aequora_compute::{ComputeError, ComputePool};
use aequora_conflict::{
    ConflictResolver, MergeDecision, MergeError, MergeInput, VersionCheck, check_version,
};
use aequora_executor::{
    AuthContext, DependencyError, DependencyPlan, ExecutionError, IncomingOperation,
    OperationExecutor, plan_dependencies,
};
use aequora_observability::{
    MetricEvent, NoopObserver, Observer, OutcomeKind, ServerPhaseKind, TraceContext,
};
use aequora_protocol::{
    BootstrapRequest, BootstrapResponse, ClientLimits, Conflict, ConflictPolicy, OperationAck,
    OperationEnvelope, OperationRejection, RejectionCode, ResyncReason, SessionMetadata,
    SyncDirective, SyncRequest, SyncResponse,
};
use aequora_store::{
    AuthoritativeStore, CommitOperation, CommitOutcome, StoreError, StoreErrorKind,
};
use aequora_types::{
    Cursor, EntityVersion, OperationId, RequestId, Sequence, SessionId, SyncScopeId,
};
use aequora_validator::{
    ProtocolLimits, ValidationError, validate_bootstrap_request, validate_request,
};
use async_trait::async_trait;
use std::{collections::HashSet, sync::Arc, time::Instant};
use thiserror::Error;

/// Server processing configuration.
#[derive(Clone, Copy, Debug)]
pub struct ServerConfig {
    /// Structural protocol bounds.
    pub limits: ProtocolLimits,
    /// Absolute maximum pull page size.
    pub max_pull_changes: usize,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            limits: ProtocolLimits::default(),
            max_pull_changes: 1_024,
        }
    }
}

/// A request-level failure. Per-operation business failures are represented in `SyncResponse`.
#[derive(Debug, Error)]
pub enum ServerError {
    /// Request failed structural validation.
    #[error("invalid sync request: {0}")]
    Validation(#[from] ValidationError),
    /// Session identity did not match authenticated identity.
    #[error("sync session identity does not match authenticated identity")]
    IdentityMismatch,
    /// Persistence failed before a safe response could be produced.
    #[error("sync persistence failed: {0}")]
    Store(#[from] StoreError),
    /// Entity version exhausted its integer representation.
    #[error("entity version overflow")]
    VersionOverflow,
    /// The operation dependency graph was invalid.
    #[error("invalid operation dependency graph: {0}")]
    Dependency(#[from] DependencyError),
    /// The application rejected access to the requested partial sync scope.
    #[error("sync scope is not authorized: {0}")]
    ScopeAuthorization(ExecutionError),
    /// Snapshot page limits were too small to return even one remaining entity.
    #[error("snapshot page limits are too small to make progress")]
    SnapshotNoProgress,
    /// This service implementation does not provide bootstrap snapshots.
    #[error("snapshot bootstrap is not available")]
    BootstrapUnavailable,
    /// Dedicated CPU work could not complete.
    #[error("sync compute work failed: {0}")]
    Compute(#[from] ComputeError),
    /// An application-registered deterministic merger failed.
    #[error("sync conflict merge failed: {0}")]
    Merge(#[from] MergeError),
}

/// Result of a server-originated command executed through the same authoritative pipeline as sync.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ServerCommandOutcome {
    /// The command committed or replayed its durable ledger result.
    Acknowledged(OperationAck),
    /// Application authorization or business validation rejected the command.
    Rejected(OperationRejection),
    /// The command requires conflict handling.
    Conflict(Conflict),
}

/// Object-safe server boundary used by Axum and in-process transports.
#[async_trait]
pub trait ExchangeService: Send + Sync {
    /// Processes one authenticated bidirectional exchange.
    async fn exchange(
        &self,
        auth: AuthContext,
        request: SyncRequest,
    ) -> Result<SyncResponse, ServerError>;

    /// Begins or resumes one authenticated consistent snapshot.
    async fn bootstrap(
        &self,
        _auth: AuthContext,
        _request: BootstrapRequest,
    ) -> Result<BootstrapResponse, ServerError> {
        Err(ServerError::BootstrapUnavailable)
    }
}

/// Server-authoritative synchronization service.
pub struct SyncServer<S, E, R, C> {
    store: Arc<S>,
    executor: Arc<E>,
    conflicts: Arc<R>,
    clock: Arc<C>,
    config: ServerConfig,
    compute: Option<Arc<ComputePool>>,
    observer: Arc<dyn Observer>,
}

/// Marker used only before a server builder receives its authoritative store.
#[doc(hidden)]
#[derive(Clone, Copy, Debug, Default)]
pub struct MissingServerStore;

/// Marker used only before a server builder receives its operation executor.
#[doc(hidden)]
#[derive(Clone, Copy, Debug, Default)]
pub struct MissingServerExecutor;

/// Marker used only before a server builder receives its conflict resolver.
#[doc(hidden)]
#[derive(Clone, Copy, Debug, Default)]
pub struct MissingServerConflicts;

/// Marker used only before a server builder receives its clock.
#[doc(hidden)]
#[derive(Clone, Copy, Debug, Default)]
pub struct MissingServerClock;

/// Fluent type-state builder for an authoritative synchronization service.
pub struct SyncServerBuilder<
    S = MissingServerStore,
    E = MissingServerExecutor,
    R = MissingServerConflicts,
    C = MissingServerClock,
> {
    store: S,
    executor: E,
    conflicts: R,
    clock: C,
    config: ServerConfig,
    compute: Option<Arc<ComputePool>>,
    observer: Arc<dyn Observer>,
}

impl Default for SyncServerBuilder {
    fn default() -> Self {
        Self {
            store: MissingServerStore,
            executor: MissingServerExecutor,
            conflicts: MissingServerConflicts,
            clock: MissingServerClock,
            config: ServerConfig::default(),
            compute: None,
            observer: Arc::new(NoopObserver),
        }
    }
}

impl SyncServerBuilder {
    /// Starts an empty type-state builder.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }
}

impl<S, E, R, C> SyncServerBuilder<S, E, R, C> {
    /// Installs or replaces the authoritative store.
    #[must_use]
    pub fn store<S2>(self, store: Arc<S2>) -> SyncServerBuilder<Arc<S2>, E, R, C> {
        SyncServerBuilder {
            store,
            executor: self.executor,
            conflicts: self.conflicts,
            clock: self.clock,
            config: self.config,
            compute: self.compute,
            observer: self.observer,
        }
    }

    /// Installs or replaces the typed operation executor or registry.
    #[must_use]
    pub fn executor<E2>(self, executor: Arc<E2>) -> SyncServerBuilder<S, Arc<E2>, R, C> {
        SyncServerBuilder {
            store: self.store,
            executor,
            conflicts: self.conflicts,
            clock: self.clock,
            config: self.config,
            compute: self.compute,
            observer: self.observer,
        }
    }

    /// Installs or replaces the conflict-policy resolver.
    #[must_use]
    pub fn conflicts<R2>(self, conflicts: Arc<R2>) -> SyncServerBuilder<S, E, Arc<R2>, C> {
        SyncServerBuilder {
            store: self.store,
            executor: self.executor,
            conflicts,
            clock: self.clock,
            config: self.config,
            compute: self.compute,
            observer: self.observer,
        }
    }

    /// Installs or replaces the hybrid timestamp clock.
    #[must_use]
    pub fn clock<C2>(self, clock: Arc<C2>) -> SyncServerBuilder<S, E, R, Arc<C2>> {
        SyncServerBuilder {
            store: self.store,
            executor: self.executor,
            conflicts: self.conflicts,
            clock,
            config: self.config,
            compute: self.compute,
            observer: self.observer,
        }
    }

    /// Applies request-validation and pull-page settings.
    #[must_use]
    pub const fn config(mut self, config: ServerConfig) -> Self {
        self.config = config;
        self
    }

    /// Installs a dedicated pool for CPU-heavy planning.
    #[must_use]
    pub fn compute_pool(mut self, compute: Arc<ComputePool>) -> Self {
        self.compute = Some(compute);
        self
    }

    /// Installs a non-blocking, payload-free observer.
    #[must_use]
    pub fn observer(mut self, observer: Arc<dyn Observer>) -> Self {
        self.observer = observer;
        self
    }
}

impl<S, E, R, C> SyncServerBuilder<Arc<S>, Arc<E>, Arc<R>, Arc<C>>
where
    S: AuthoritativeStore + 'static,
    E: OperationExecutor + 'static,
    R: ConflictResolver + 'static,
    C: Clock + 'static,
{
    /// Builds the service. This method only exists once every required component is present.
    #[must_use]
    pub fn build(self) -> SyncServer<S, E, R, C> {
        SyncServer {
            store: self.store,
            executor: self.executor,
            conflicts: self.conflicts,
            clock: self.clock,
            config: self.config,
            compute: self.compute,
            observer: self.observer,
        }
    }
}

impl<S, E, R, C> SyncServer<S, E, R, C> {
    /// Creates a server with explicit outer-layer components.
    #[must_use]
    pub fn new(store: Arc<S>, executor: Arc<E>, conflicts: Arc<R>, clock: Arc<C>) -> Self {
        Self {
            store,
            executor,
            conflicts,
            clock,
            config: ServerConfig::default(),
            compute: None,
            observer: Arc::new(NoopObserver),
        }
    }

    /// Applies request limits and pull configuration.
    #[must_use]
    pub const fn with_config(mut self, config: ServerConfig) -> Self {
        self.config = config;
        self
    }

    /// Installs a dedicated Rayon pool for large CPU-bound planning workloads.
    #[must_use]
    pub fn with_compute_pool(mut self, compute: Arc<ComputePool>) -> Self {
        self.compute = Some(compute);
        self
    }

    /// Installs a non-blocking payload-free metrics and tracing observer.
    #[must_use]
    pub fn with_observer(mut self, observer: Arc<dyn Observer>) -> Self {
        self.observer = observer;
        self
    }
}

impl<S, E, R, C> SyncServer<S, E, R, C>
where
    S: AuthoritativeStore + 'static,
    E: OperationExecutor + 'static,
    R: ConflictResolver + 'static,
    C: Clock + 'static,
{
    /// Executes an admin/API/background-job command through validation, authorization, domain
    /// execution, and the atomic entity/journal/ledger/audit transaction.
    ///
    /// This prevents server-side callers from bypassing the synchronization journal while still
    /// reusing the application's registered command handlers.
    ///
    /// # Errors
    ///
    /// Returns [`ServerError`] for structural, scope-authorization, conflict-merge, execution, or
    /// persistence failures.
    pub async fn execute_server_command(
        &self,
        auth: AuthContext,
        scope_id: SyncScopeId,
        operation: OperationEnvelope,
    ) -> Result<ServerCommandOutcome, ServerError> {
        let session = SessionMetadata {
            session_id: SessionId::new(),
            device_id: auth.device_id,
            actor_id: auth.actor_id,
            tenant_id: auth.tenant_id,
            scope_id,
            partitions: Vec::new(),
        };
        let validation_started = Instant::now();
        let validation = validate_request(
            SyncRequest {
                protocol: operation.protocol_version,
                request_id: RequestId::new(),
                session: session.clone(),
                cursor: None,
                operations: vec![operation.clone()],
                limits: ClientLimits {
                    max_changes: 0,
                    max_response_bytes: 1,
                },
                capabilities: Vec::new(),
            },
            self.config.limits,
        );
        self.record_phase(ServerPhaseKind::Validation, validation_started);
        validation?;
        self.executor
            .authorize_scope(&auth, &session)
            .await
            .map_err(ServerError::ScopeAuthorization)?;
        let outcome = self
            .process_operation(&auth, &operation, &HashSet::new(), scope_id)
            .await?;
        Ok(match outcome {
            ProcessResult::Acknowledged(acknowledgement) => {
                ServerCommandOutcome::Acknowledged(acknowledgement)
            }
            ProcessResult::Rejected(rejection) => ServerCommandOutcome::Rejected(rejection),
            ProcessResult::Conflict(conflict) => ServerCommandOutcome::Conflict(conflict),
        })
    }

    #[allow(clippy::too_many_lines)]
    async fn process_operation(
        &self,
        auth: &AuthContext,
        operation: &OperationEnvelope,
        completed: &HashSet<OperationId>,
        scope_id: SyncScopeId,
    ) -> Result<ProcessResult, ServerError> {
        let authenticated = match IncomingOperation::new(operation).authenticate(auth) {
            Ok(authenticated) => authenticated,
            Err(error) => {
                return Ok(ProcessResult::Rejected(OperationRejection {
                    operation_id: operation.operation_id,
                    code: error.code,
                    message: error.message,
                }));
            }
        };

        let database_started = Instant::now();
        let previous = self
            .store
            .operation_result(auth.tenant_id, operation.operation_id)
            .await?;
        self.record_phase(ServerPhaseKind::Database, database_started);
        if let Some(mut previous) = previous {
            previous.duplicate = true;
            return Ok(ProcessResult::Acknowledged(previous));
        }

        for dependency in &operation.metadata.dependencies {
            if completed.contains(dependency) {
                continue;
            }
            let database_started = Instant::now();
            let dependency_result = self
                .store
                .operation_result(auth.tenant_id, *dependency)
                .await?;
            self.record_phase(ServerPhaseKind::Database, database_started);
            if dependency_result.is_none() {
                return Ok(ProcessResult::Rejected(rejection(
                    operation,
                    RejectionCode::Dependency,
                    "a required operation has not completed",
                )));
            }
        }

        let execution_started = Instant::now();
        let authorization = self.executor.authorize(auth, authenticated).await;
        self.record_phase(ServerPhaseKind::Execution, execution_started);
        let authorized = match authorization {
            Ok(authorized) => authorized,
            Err(error) => {
                return Ok(ProcessResult::Rejected(OperationRejection {
                    operation_id: operation.operation_id,
                    code: error.code,
                    message: error.message,
                }));
            }
        };

        let database_started = Instant::now();
        let snapshot = self
            .store
            .read_entity(auth.tenant_id, operation.entity)
            .await?;
        self.record_phase(ServerPhaseKind::Database, database_started);
        let current = snapshot.as_ref().map(|value| &value.current);
        let current_version = current.map(|value| value.version);
        let merge_policy = match check_version(operation.base_version, current_version) {
            VersionCheck::Current => None,
            VersionCheck::Missing => {
                return Ok(ProcessResult::Conflict(conflict(
                    operation,
                    current_version,
                    self.conflicts.policy(operation.operation_kind.0),
                    "the entity no longer exists",
                )));
            }
            VersionCheck::Diverged { .. } => {
                let policy = self.conflicts.policy(operation.operation_kind.0);
                match policy {
                    ConflictPolicy::ClientWins | ConflictPolicy::CommutativeOperation => None,
                    ConflictPolicy::FieldMerge
                    | ConflictPolicy::CustomMerge
                    | ConflictPolicy::Crdt => Some(policy),
                    _ => {
                        return Ok(ProcessResult::Conflict(conflict(
                            operation,
                            current_version,
                            policy,
                            "the operation is based on a stale entity version",
                        )));
                    }
                }
            }
        };

        let executable = authorized.validate().executable();
        let execution_started = Instant::now();
        let execution = self.executor.execute(auth, executable, current).await;
        self.record_phase(ServerPhaseKind::Execution, execution_started);
        let mut mutation = match execution {
            Ok(mutation) => mutation,
            Err(error) => {
                return Ok(ProcessResult::Rejected(OperationRejection {
                    operation_id: operation.operation_id,
                    code: error.code,
                    message: error.message,
                }));
            }
        };
        if let Some(policy) = merge_policy {
            let Some(current) = current else {
                return Ok(ProcessResult::Conflict(conflict(
                    operation,
                    current_version,
                    policy,
                    "the authoritative entity is unavailable for merge",
                )));
            };
            match self.conflicts.merge(MergeInput {
                operation,
                current_payload: &current.payload,
                current_tombstone: current.tombstone,
                candidate_payload: &mutation.payload,
                candidate_kind: mutation.change_kind,
            })? {
                MergeDecision::Merged {
                    payload,
                    change_kind,
                } => {
                    mutation.payload = payload;
                    mutation.change_kind = change_kind;
                }
                MergeDecision::Unresolved { message } => {
                    return Ok(ProcessResult::Conflict(conflict(
                        operation,
                        current_version,
                        policy,
                        &message,
                    )));
                }
            }
        }
        let next_version = match current_version {
            Some(version) => version.checked_next().ok_or(ServerError::VersionOverflow)?,
            None => EntityVersion::INITIAL,
        };
        let commit = CommitOperation {
            operation_id: operation.operation_id,
            actor_id: auth.actor_id,
            device_id: auth.device_id,
            operation_kind: operation.operation_kind.0,
            tenant_id: auth.tenant_id,
            scope_id,
            entity: operation.entity,
            expected_version: current_version,
            next_version,
            payload: mutation.payload,
            change_kind: mutation.change_kind,
            timestamp: self.clock.now(),
            command_digest: *blake3::hash(&operation.payload).as_bytes(),
        };
        let database_started = Instant::now();
        let outcome = self.store.commit_operation(commit).await?;
        self.record_phase(ServerPhaseKind::Database, database_started);
        match outcome {
            CommitOutcome::Applied(ack) => Ok(ProcessResult::Acknowledged(ack)),
            CommitOutcome::Duplicate(mut ack) => {
                ack.duplicate = true;
                Ok(ProcessResult::Acknowledged(ack))
            }
            CommitOutcome::VersionChanged { current } => Ok(ProcessResult::Conflict(conflict(
                operation,
                current,
                self.conflicts.policy(operation.operation_kind.0),
                "the entity changed while this operation was being committed",
            ))),
        }
    }

    #[allow(clippy::too_many_lines)]
    async fn process(
        &self,
        auth: AuthContext,
        request: SyncRequest,
    ) -> Result<SyncResponse, ServerError> {
        if request.protocol < self.config.limits.minimum_protocol
            || request.protocol > self.config.limits.current_protocol
        {
            return Ok(SyncResponse {
                protocol: self.config.limits.current_protocol,
                directive: SyncDirective::UpgradeRequired {
                    minimum: self.config.limits.minimum_protocol,
                    current: self.config.limits.current_protocol,
                },
                acknowledged: Vec::new(),
                rejected: Vec::new(),
                conflicts: Vec::new(),
                changes: Vec::new(),
                next_cursor: request.cursor.unwrap_or(Cursor {
                    scope: request.session.scope_id,
                    sequence: Sequence(0),
                }),
                has_more: false,
                server_time: self.clock.now(),
            });
        }
        let validation_started = Instant::now();
        let validation = validate_request(request, self.config.limits);
        self.record_phase(ServerPhaseKind::Validation, validation_started);
        let request = validation?.into_inner();
        if request.session.tenant_id != auth.tenant_id
            || request.session.actor_id != auth.actor_id
            || request.session.device_id != auth.device_id
        {
            return Err(ServerError::IdentityMismatch);
        }
        self.executor
            .authorize_scope(&auth, &request.session)
            .await
            .map_err(ServerError::ScopeAuthorization)?;

        let start = request.cursor.map_or(Sequence(0), |cursor| cursor.sequence);
        let database_started = Instant::now();
        let minimum_cursor = self
            .store
            .minimum_retained_cursor(auth.tenant_id, request.session.scope_id)
            .await?;
        self.record_phase(ServerPhaseKind::Database, database_started);
        if start < minimum_cursor {
            return Ok(SyncResponse {
                protocol: request.protocol,
                directive: SyncDirective::ResyncRequired {
                    reason: ResyncReason::CursorExpired,
                },
                acknowledged: Vec::new(),
                rejected: Vec::new(),
                conflicts: Vec::new(),
                changes: Vec::new(),
                next_cursor: Cursor {
                    scope: request.session.scope_id,
                    sequence: minimum_cursor,
                },
                has_more: false,
                server_time: self.clock.now(),
            });
        }

        let mut acknowledged = Vec::with_capacity(request.operations.len());
        let mut rejected = Vec::new();
        let mut conflicts = Vec::new();
        let mut completed = HashSet::new();
        let dependency_plan = self.plan_dependencies(&request.operations).await?;
        for &operation_index in dependency_plan.ordered_indices() {
            let operation = &request.operations[operation_index];
            match self
                .process_operation(&auth, operation, &completed, request.session.scope_id)
                .await?
            {
                ProcessResult::Acknowledged(ack) => {
                    completed.insert(ack.operation_id);
                    acknowledged.push(ack);
                }
                ProcessResult::Rejected(rejection) => rejected.push(rejection),
                ProcessResult::Conflict(conflict) => conflicts.push(conflict),
            }
        }

        let requested_limit = usize::try_from(request.limits.max_changes).unwrap_or(usize::MAX);
        let database_started = Instant::now();
        let page = self
            .store
            .read_changes_after(
                auth.tenant_id,
                request.session.scope_id,
                start,
                requested_limit.min(self.config.max_pull_changes),
                usize::try_from(request.limits.max_response_bytes).unwrap_or(usize::MAX),
            )
            .await?;
        self.record_phase(ServerPhaseKind::Database, database_started);
        Ok(SyncResponse {
            protocol: request.protocol,
            directive: SyncDirective::Continue,
            acknowledged,
            rejected,
            conflicts,
            changes: page.changes,
            next_cursor: Cursor {
                scope: request.session.scope_id,
                sequence: page.next_sequence,
            },
            has_more: page.has_more,
            server_time: self.clock.now(),
        })
    }

    async fn plan_dependencies(
        &self,
        operations: &[OperationEnvelope],
    ) -> Result<DependencyPlan, ServerError> {
        let Some(compute) = &self.compute else {
            return Ok(plan_dependencies(operations)?);
        };
        if !compute.should_parallelize(operations.len()) {
            return Ok(plan_dependencies(operations)?);
        }
        self.observer.record(MetricEvent::ComputeOffload {
            items: usize_to_u64(operations.len()),
        });
        let operations = operations.to_vec();
        Ok(compute
            .run(move || plan_dependencies(&operations))
            .await??)
    }

    fn record_phase(&self, phase: ServerPhaseKind, started: Instant) {
        self.observer.record(MetricEvent::ServerPhase {
            phase,
            duration_micros: duration_micros(started.elapsed()),
        });
    }

    async fn process_bootstrap(
        &self,
        auth: AuthContext,
        request: BootstrapRequest,
    ) -> Result<BootstrapResponse, ServerError> {
        let validation_started = Instant::now();
        let validation = validate_bootstrap_request(&request, self.config.limits);
        self.record_phase(ServerPhaseKind::Validation, validation_started);
        validation?;
        if request.session.tenant_id != auth.tenant_id
            || request.session.actor_id != auth.actor_id
            || request.session.device_id != auth.device_id
        {
            return Err(ServerError::IdentityMismatch);
        }
        self.executor
            .authorize_scope(&auth, &request.session)
            .await
            .map_err(ServerError::ScopeAuthorization)?;
        let snapshot_id = if let Some(snapshot_id) = request.snapshot_id {
            snapshot_id
        } else {
            let database_started = Instant::now();
            let descriptor = self
                .store
                .create_snapshot(
                    auth.tenant_id,
                    request.session.scope_id,
                    &request.session.partitions,
                )
                .await?;
            self.record_phase(ServerPhaseKind::Database, database_started);
            descriptor.snapshot_id
        };
        let database_started = Instant::now();
        let page = self
            .store
            .read_snapshot(
                auth.tenant_id,
                snapshot_id,
                request.offset,
                usize::try_from(request.limits.max_entities).unwrap_or(usize::MAX),
                usize::try_from(request.limits.max_payload_bytes).unwrap_or(usize::MAX),
            )
            .await?;
        self.record_phase(ServerPhaseKind::Database, database_started);
        if page.descriptor.cursor.scope != request.session.scope_id {
            return Err(ServerError::IdentityMismatch);
        }
        if page.has_more && page.entities.is_empty() && page.next_offset == request.offset {
            return Err(ServerError::SnapshotNoProgress);
        }
        Ok(BootstrapResponse {
            protocol: request.protocol,
            snapshot_id: page.descriptor.snapshot_id,
            cursor: page.descriptor.cursor,
            offset: request.offset,
            entities: page.entities,
            next_offset: page.next_offset,
            has_more: page.has_more,
            server_time: self.clock.now(),
        })
    }
}

#[async_trait]
impl<S, E, R, C> ExchangeService for SyncServer<S, E, R, C>
where
    S: AuthoritativeStore + 'static,
    E: OperationExecutor + 'static,
    R: ConflictResolver + 'static,
    C: Clock + 'static,
{
    async fn exchange(
        &self,
        auth: AuthContext,
        request: SyncRequest,
    ) -> Result<SyncResponse, ServerError> {
        let operations = request.operations.len();
        let trace = TraceContext {
            sync_session_id: request.session.session_id,
            request_id: request.request_id,
            device_id: request.session.device_id,
            tenant_id: request.session.tenant_id,
        };
        let started = Instant::now();
        let result = self.process(auth, request).await;
        let (changes, conflicts, rejections) = result.as_ref().map_or((0, 0, 0), |response| {
            (
                response.changes.len(),
                response.conflicts.len(),
                response.rejected.len(),
            )
        });
        self.observer.record_with_context(
            trace,
            MetricEvent::ServerExchange {
                duration_micros: duration_micros(started.elapsed()),
                operations: usize_to_u64(operations),
                changes: usize_to_u64(changes),
                conflicts: usize_to_u64(conflicts),
                rejections: usize_to_u64(rejections),
                outcome: server_result_outcome(&result),
            },
        );
        result
    }

    async fn bootstrap(
        &self,
        auth: AuthContext,
        request: BootstrapRequest,
    ) -> Result<BootstrapResponse, ServerError> {
        let trace = TraceContext {
            sync_session_id: request.session.session_id,
            request_id: request.request_id,
            device_id: request.session.device_id,
            tenant_id: request.session.tenant_id,
        };
        let started = Instant::now();
        let result = self.process_bootstrap(auth, request).await;
        self.observer.record_with_context(
            trace,
            MetricEvent::BootstrapPage {
                duration_micros: duration_micros(started.elapsed()),
                entities: result
                    .as_ref()
                    .map_or(0, |response| usize_to_u64(response.entities.len())),
                outcome: server_result_outcome(&result),
            },
        );
        result
    }
}

fn duration_micros(duration: std::time::Duration) -> u64 {
    u64::try_from(duration.as_micros()).unwrap_or(u64::MAX)
}

fn usize_to_u64(value: usize) -> u64 {
    u64::try_from(value).unwrap_or(u64::MAX)
}

fn server_result_outcome<T>(result: &Result<T, ServerError>) -> OutcomeKind {
    match result {
        Ok(_) => OutcomeKind::Success,
        Err(ServerError::Store(error)) if error.kind == StoreErrorKind::Transient => {
            OutcomeKind::TransientFailure
        }
        Err(_) => OutcomeKind::PermanentFailure,
    }
}

enum ProcessResult {
    Acknowledged(OperationAck),
    Rejected(OperationRejection),
    Conflict(Conflict),
}

fn rejection(
    operation: &OperationEnvelope,
    code: RejectionCode,
    message: &str,
) -> OperationRejection {
    OperationRejection {
        operation_id: operation.operation_id,
        code,
        message: message.to_owned(),
    }
}

fn conflict(
    operation: &OperationEnvelope,
    current: Option<EntityVersion>,
    policy: ConflictPolicy,
    message: &str,
) -> Conflict {
    Conflict {
        operation_id: operation.operation_id,
        entity: operation.entity,
        client_base: operation.base_version,
        server_version: current,
        policy,
        message: message.to_owned(),
    }
}
