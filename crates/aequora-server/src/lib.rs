//! Authoritative sync-session orchestration.

/// Stable plug-and-play entry point for constructing an authoritative service.
#[derive(Clone, Copy, Debug, Default)]
pub struct AequoraServer;

impl AequoraServer {
    /// Starts the type-state server builder.
    #[must_use]
    pub fn builder() -> SyncServerBuilder {
        SyncServerBuilder::new()
    }
}

/// Focused imports for application server integrations.
pub mod prelude {
    pub use crate::{
        AdmittedExchangeService, AequoraServer, ExchangeService, ServerBuildError,
        ServerCommandOutcome, ServerConfig, ServerError, ServerRegionalReadGuard, SyncServer,
        SyncServerBuilder,
    };
    pub use aequora_admission::{AdmissionController, AdmissionPolicy, HierarchicalAdmission};
    pub use aequora_authority::{
        AuthorityController, AuthorityPromotionPolicy, AuthorityRole, AuthorityRuntimeMode,
        AuthorityState,
    };
    pub use aequora_clock::{Clock, SystemClock};
    pub use aequora_conflict::{ConflictResolver, RejectConflicts};
    pub use aequora_executor::{
        AuthContext, DerivedEventProvenance, DomainOperation, JobProvenance, OperationExecutor,
        OperationHandler, OperationRegistry, ScopeAuthorizer, TrustedProvenance,
    };
    pub use aequora_region::{
        ReadResponseMetadata, RegionError, RegionalReadRequest, ReplicaObservation,
    };
    pub use aequora_store::{
        AdapterCapabilities, AdapterManifest, AdapterManifestProvider, AdapterRequirements,
        AdapterRole, AdapterTier, AuthoritativeStore, ProductionAdapterPair,
    };
}

use aequora_admission::{
    AdmissionController, AdmissionRejection, CostUnits, RequestShape, ResourceDomain,
    ServerPriorityPolicy, WorkDescriptor,
};
use aequora_authority::{
    AuthorityController, AuthorityError, AuthorityPromotionPolicy, AuthorityRole,
    AuthorityRuntimeMode, AuthorityState, CursorDisposition, validate_cursor,
};
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
    TransactionOutcomeKind,
};
use aequora_performance::PerformancePolicy;
use aequora_protocol::{
    BootstrapRequest, BootstrapResponse, ClientLimits, Conflict, ConflictPolicy, OperationAck,
    OperationEnvelope, OperationRejection, RejectionCode, ResyncReason, SessionMetadata,
    SyncDirective, SyncRequest, SyncResponse,
};
use aequora_region::{
    ReadResponseMetadata, RegionError, RegionalReadRequest, ReplicaObservation, ReplicaReadGuard,
};
use aequora_store::{
    AdapterCompatibilityError, AdapterManifestProvider, AdapterRequirements, AuthoritativeStore,
    CommitOperation, CommitOutcome, StoreError, StoreErrorKind,
};
use aequora_types::{
    AuthorityId, AuthorityInstanceId, EntityVersion, EventId, OperationId, RequestId, Sequence,
    SessionId, SyncScopeId,
};
use aequora_validator::{
    ProtocolLimits, ValidationError, validate_bootstrap_request, validate_request,
};
use async_trait::async_trait;
use rayon::prelude::*;
use std::{
    collections::HashSet,
    sync::{
        Arc,
        atomic::{AtomicU8, Ordering},
    },
    time::Instant,
};
use thiserror::Error;

/// Replica-side defense-in-depth for regional read API handlers.
///
/// Routing metadata can be stale, so every server instance validates its own durable watermark,
/// epoch, projection version, and authorization freshness immediately before reading.
#[derive(Clone, Debug)]
pub struct ServerRegionalReadGuard {
    observation: ReplicaObservation,
}

impl ServerRegionalReadGuard {
    /// Creates a guard from the adapter's latest durable apply observation.
    #[must_use]
    pub const fn new(observation: ReplicaObservation) -> Self {
        Self { observation }
    }

    /// Replaces health and watermark state after an atomic adapter observation refresh.
    pub fn update(&mut self, observation: ReplicaObservation) {
        self.observation = observation;
    }

    /// Authorizes one regional read and returns freshness metadata for its response.
    ///
    /// # Errors
    ///
    /// Fails closed if this node cannot prove the requested consistency and security bounds.
    pub fn authorize(
        &self,
        request: RegionalReadRequest,
    ) -> Result<ReadResponseMetadata, RegionError> {
        ReplicaReadGuard::validate(&self.observation, request)
    }
}

/// Server processing configuration.
#[derive(Clone, Copy, Debug)]
pub struct ServerConfig {
    /// Structural protocol bounds.
    pub limits: ProtocolLimits,
    /// Absolute maximum pull page size.
    pub max_pull_changes: usize,
    /// Cross-layer request, response, snapshot, and CPU memory policy.
    pub performance: PerformancePolicy,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            limits: ProtocolLimits::default(),
            max_pull_changes: 1_024,
            performance: PerformancePolicy::default(),
        }
    }
}

/// A request-level failure. Per-operation business failures are represented in `SyncResponse`.
#[derive(Debug, Error)]
pub enum ServerError {
    /// Cross-transport admission rejected work before authoritative mutation.
    #[error("sync admission rejected work: {0}")]
    Admission(#[from] AdmissionRejection),
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
    /// The client advertised a response ceiling too small for mandatory progress.
    #[error("client response limit is too small to make synchronization progress")]
    ResponseLimit,
    /// A response could not be measured with the production wire codec.
    #[error("sync response encoding failed: {0}")]
    Codec(#[from] aequora_codec::CodecError),
    /// A typed request or response exceeded the configured in-memory domain budget.
    #[error("{domain} memory use {actual} exceeds configured limit {maximum}")]
    MemoryBudget {
        domain: &'static str,
        actual: usize,
        maximum: usize,
    },
    /// An application-registered deterministic merger failed.
    #[error("sync conflict merge failed: {0}")]
    Merge(#[from] MergeError),
    /// Operator-selected maintenance mode currently rejects this synchronization action.
    #[error("sync is unavailable while the server is in {mode:?} maintenance mode")]
    Maintenance {
        /// Active maintenance policy.
        mode: MaintenanceMode,
        /// Retry delay safe to expose to clients and transports.
        retry_after_seconds: u64,
    },
    /// Authority identity, epoch, runtime mode, or write fence rejected the request.
    #[error("authority rejected synchronization: {0}")]
    Authority(#[from] AuthorityError),
}

/// Runtime maintenance policy for an [`ExchangeService`].
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
#[repr(u8)]
pub enum MaintenanceMode {
    /// Bidirectional synchronization and bootstrap are enabled.
    #[default]
    Normal = 0,
    /// Pull and bootstrap remain available, but exchanges containing writes are rejected.
    ReadOnly = 1,
    /// All exchange and bootstrap work is rejected with a retryable typed error.
    SyncPaused = 2,
}

impl MaintenanceMode {
    /// Whether one exchange may proceed in this mode.
    #[must_use]
    pub const fn allows_exchange(self, has_operations: bool) -> bool {
        match self {
            Self::Normal => true,
            Self::ReadOnly => !has_operations,
            Self::SyncPaused => false,
        }
    }

    /// Whether snapshot bootstrap may proceed in this mode.
    #[must_use]
    pub const fn allows_bootstrap(self) -> bool {
        matches!(self, Self::Normal | Self::ReadOnly)
    }
}

/// Cloneable, lock-free maintenance policy handle suitable for an authenticated control plane.
#[derive(Clone, Default)]
pub struct MaintenanceController {
    mode: Arc<AtomicU8>,
}

impl MaintenanceController {
    /// Returns the active mode.
    #[must_use]
    pub fn mode(&self) -> MaintenanceMode {
        match self.mode.load(Ordering::Acquire) {
            1 => MaintenanceMode::ReadOnly,
            2 => MaintenanceMode::SyncPaused,
            _ => MaintenanceMode::Normal,
        }
    }

    /// Atomically changes the active mode and returns the previous value.
    #[must_use]
    pub fn set_mode(&self, mode: MaintenanceMode) -> MaintenanceMode {
        let previous = self.mode.swap(mode as u8, Ordering::AcqRel);
        match previous {
            1 => MaintenanceMode::ReadOnly,
            2 => MaintenanceMode::SyncPaused,
            _ => MaintenanceMode::Normal,
        }
    }
}

/// Maintenance-policy decorator that preserves pending client work by rejecting before execution.
pub struct MaintenanceService<S: ?Sized> {
    inner: Arc<S>,
    controller: MaintenanceController,
    retry_after_seconds: u64,
}

impl<S: ?Sized> MaintenanceService<S> {
    /// Wraps a service with a runtime maintenance gate.
    #[must_use]
    pub fn new(inner: Arc<S>, controller: MaintenanceController, retry_after_seconds: u64) -> Self {
        Self {
            inner,
            controller,
            retry_after_seconds: retry_after_seconds.max(1),
        }
    }

    /// Returns the handle used by the host's authenticated control plane.
    #[must_use]
    pub const fn controller(&self) -> &MaintenanceController {
        &self.controller
    }

    fn rejection(&self, mode: MaintenanceMode) -> ServerError {
        ServerError::Maintenance {
            mode,
            retry_after_seconds: self.retry_after_seconds,
        }
    }
}

#[async_trait]
impl<S> ExchangeService for MaintenanceService<S>
where
    S: ExchangeService + ?Sized,
{
    async fn exchange(
        &self,
        auth: AuthContext,
        request: SyncRequest,
    ) -> Result<SyncResponse, ServerError> {
        let mode = self.controller.mode();
        if mode.allows_exchange(!request.operations.is_empty()) {
            self.inner.exchange(auth, request).await
        } else {
            Err(self.rejection(mode))
        }
    }

    async fn bootstrap(
        &self,
        auth: AuthContext,
        request: BootstrapRequest,
    ) -> Result<BootstrapResponse, ServerError> {
        let mode = self.controller.mode();
        if mode.allows_bootstrap() {
            self.inner.bootstrap(auth, request).await
        } else {
            Err(self.rejection(mode))
        }
    }
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

/// Optional cross-transport admission decorator held across authoritative execution.
pub struct AdmittedExchangeService<S: ?Sized> {
    inner: Arc<S>,
    admission: Arc<dyn AdmissionController>,
    priority: ServerPriorityPolicy,
    tenant_weight: u16,
}

impl<S: ?Sized> AdmittedExchangeService<S> {
    /// Wraps a service with server-derived work classification and hierarchical RAII admission.
    #[must_use]
    pub fn new(inner: Arc<S>, admission: Arc<dyn AdmissionController>) -> Self {
        Self {
            inner,
            admission,
            priority: ServerPriorityPolicy::default(),
            tenant_weight: 1,
        }
    }

    /// Replaces the server-owned work-kind classification policy.
    #[must_use]
    pub fn with_priority_policy(mut self, priority: ServerPriorityPolicy) -> Self {
        self.priority = priority;
        self
    }

    /// Applies a server-derived contractual tenant weight, never a request-body value.
    #[must_use]
    pub fn with_tenant_weight(mut self, tenant_weight: u16) -> Self {
        self.tenant_weight = tenant_weight.max(1);
        self
    }

    fn exchange_work(&self, auth: &AuthContext, request: &SyncRequest) -> WorkDescriptor {
        let operation_count = request.operations.len();
        let dependency_edges = request
            .operations
            .iter()
            .map(|operation| operation.metadata.dependencies.len())
            .sum::<usize>();
        let payload_bytes = request.operations.iter().fold(0u64, |total, operation| {
            total.saturating_add(u64::try_from(operation.payload.len()).unwrap_or(u64::MAX))
        });
        let kind = if request.operations.is_empty() {
            aequora_scheduler::WorkKind::PullChanges
        } else {
            aequora_scheduler::WorkKind::PushOperations
        };
        let class = self.priority.classify(kind, None);
        let mut resources = vec![
            ResourceDomain::SyncExchange,
            ResourceDomain::DatabaseTransaction,
        ];
        if !request.operations.is_empty() {
            resources.push(ResourceDomain::InteractiveCpu);
        }
        WorkDescriptor {
            tenant_id: auth.tenant_id,
            tenant_weight: self.tenant_weight,
            kind,
            class,
            cost: CostUnits::new(
                u32::try_from(
                    operation_count
                        .saturating_add(dependency_edges)
                        .saturating_add(1),
                )
                .unwrap_or(u32::MAX),
            ),
            shape: RequestShape {
                operations: operation_count,
                encoded_bytes: payload_bytes,
                decompressed_bytes: payload_bytes,
                scopes: request.session.partitions.len(),
                dependency_edges,
                dependency_depth: request
                    .operations
                    .iter()
                    .map(|operation| operation.metadata.dependencies.len())
                    .max()
                    .unwrap_or(0),
            },
            scope: Some(request.session.scope_id),
            estimated_response_bytes: u64::from(request.limits.max_response_bytes),
            resources,
        }
    }

    fn bootstrap_work(&self, auth: &AuthContext, request: &BootstrapRequest) -> WorkDescriptor {
        let kind = aequora_scheduler::WorkKind::Bootstrap;
        WorkDescriptor {
            tenant_id: auth.tenant_id,
            tenant_weight: self.tenant_weight,
            kind,
            class: self.priority.classify(kind, None),
            cost: CostUnits::new(request.limits.max_entities.max(1)),
            shape: RequestShape {
                scopes: request.session.partitions.len(),
                ..RequestShape::default()
            },
            scope: Some(request.session.scope_id),
            estimated_response_bytes: u64::from(request.limits.max_payload_bytes),
            resources: vec![
                ResourceDomain::SyncExchange,
                ResourceDomain::DatabaseTransaction,
                ResourceDomain::SnapshotDownload,
            ],
        }
    }
}

#[async_trait]
impl<S> ExchangeService for AdmittedExchangeService<S>
where
    S: ExchangeService + ?Sized,
{
    async fn exchange(
        &self,
        auth: AuthContext,
        request: SyncRequest,
    ) -> Result<SyncResponse, ServerError> {
        let work = self.exchange_work(&auth, &request);
        let _permit = self.admission.admit(&work).await?;
        self.inner.exchange(auth, request).await
    }

    async fn bootstrap(
        &self,
        auth: AuthContext,
        request: BootstrapRequest,
    ) -> Result<BootstrapResponse, ServerError> {
        let work = self.bootstrap_work(&auth, &request);
        let _permit = self.admission.admit(&work).await?;
        self.inner.bootstrap(auth, request).await
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
    authority: AuthorityController,
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
    authority: AuthorityController,
    authority_explicit: bool,
}

/// Production server assembly failure.
#[derive(Clone, Copy, Debug, Error, Eq, PartialEq)]
pub enum ServerBuildError {
    /// Selected authority adapter does not meet production requirements.
    #[error(transparent)]
    Adapter(#[from] AdapterCompatibilityError),
    /// Production assembly requires a deployment-specific authority identity/controller.
    #[error("production server requires an explicit authority controller")]
    AuthorityRequired,
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
            authority: development_authority(),
            authority_explicit: false,
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
            authority: self.authority,
            authority_explicit: self.authority_explicit,
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
            authority: self.authority,
            authority_explicit: self.authority_explicit,
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
            authority: self.authority,
            authority_explicit: self.authority_explicit,
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
            authority: self.authority,
            authority_explicit: self.authority_explicit,
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

    /// Installs authority metadata, fencing, failover, and recovery enforcement.
    #[must_use]
    pub fn authority(mut self, authority: AuthorityController) -> Self {
        self.authority = authority;
        self.authority_explicit = true;
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
            authority: self.authority,
        }
    }
}

impl<S, E, R, C> SyncServerBuilder<Arc<S>, Arc<E>, Arc<R>, Arc<C>>
where
    S: AuthoritativeStore + AdapterManifestProvider + 'static,
    E: OperationExecutor + 'static,
    R: ConflictResolver + 'static,
    C: Clock + 'static,
{
    /// Verifies the authority adapter's production manifest before building the server.
    ///
    /// Reference/test stores should continue to use [`Self::build`].
    ///
    /// # Errors
    ///
    /// Returns a typed adapter compatibility failure before any request is accepted.
    pub fn build_production(self) -> Result<SyncServer<S, E, R, C>, ServerBuildError> {
        if !self.authority_explicit {
            return Err(ServerBuildError::AuthorityRequired);
        }
        AdapterRequirements::PRODUCTION_AUTHORITATIVE.verify(self.store.adapter_manifest())?;
        Ok(self.build())
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
            authority: development_authority(),
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

    /// Installs authority metadata, fencing, failover, and recovery enforcement.
    #[must_use]
    pub fn with_authority(mut self, authority: AuthorityController) -> Self {
        self.authority = authority;
        self
    }

    /// Returns the failover controller used by administrative/control-plane code.
    #[must_use]
    pub const fn authority(&self) -> &AuthorityController {
        &self.authority
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
            .process_operation(
                &auth,
                &operation,
                &HashSet::new(),
                scope_id,
                *blake3::hash(&operation.payload).as_bytes(),
            )
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
        command_digest: [u8; 32],
    ) -> Result<ProcessResult, ServerError> {
        let authority_commit = self.authority.authorize_write()?;
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
        let provenance = authenticated.provenance(auth);

        let database_started = Instant::now();
        let previous = self
            .store
            .operation_result(auth.tenant_id, operation.operation_id)
            .await?;
        self.record_phase(ServerPhaseKind::Database, database_started);
        if let Some(mut previous) = previous {
            let database_started = Instant::now();
            let original_lineage = self
                .store
                .operation_lineage(auth.tenant_id, operation.operation_id)
                .await?;
            self.record_phase(ServerPhaseKind::Database, database_started);
            if original_lineage
                != Some(
                    operation
                        .metadata
                        .lineage
                        .resolved_for_operation(operation.operation_id),
                )
            {
                return Ok(ProcessResult::Rejected(rejection(
                    operation,
                    RejectionCode::InvalidOperation,
                    "retry lineage differs from the original operation",
                )));
            }
            self.observer.record(MetricEvent::ServerTransaction {
                outcome: TransactionOutcomeKind::Duplicate,
            });
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
                    | ConflictPolicy::LastWriterWins
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
        let event = provenance.primary_event(EventId::new());
        let commit = CommitOperation {
            authority: Some(authority_commit),
            operation_id: operation.operation_id,
            event_id: event.event_id,
            operation_lineage: provenance.operation_lineage(),
            actor_id: provenance.actor_id(),
            device_id: provenance.device_id(),
            operation_kind: operation.operation_kind.0,
            tenant_id: provenance.tenant_id(),
            scope_id,
            entity: operation.entity,
            expected_version: current_version,
            next_version,
            payload: mutation.payload,
            change_kind: mutation.change_kind,
            timestamp: self.clock.now(),
            command_digest,
        };
        self.authority.verify_commit_context(authority_commit)?;
        let database_started = Instant::now();
        let outcome = self.store.commit_operation(commit).await;
        self.record_phase(ServerPhaseKind::Database, database_started);
        let outcome = match outcome {
            Ok(outcome) => outcome,
            Err(error) => {
                self.observer.record(MetricEvent::ServerTransaction {
                    outcome: TransactionOutcomeKind::Failed,
                });
                return Err(ServerError::Store(error));
            }
        };
        match outcome {
            CommitOutcome::Applied(ack) => {
                self.observer.record(MetricEvent::ServerTransaction {
                    outcome: TransactionOutcomeKind::Applied,
                });
                Ok(ProcessResult::Acknowledged(ack))
            }
            CommitOutcome::Duplicate(mut ack) => {
                self.observer.record(MetricEvent::ServerTransaction {
                    outcome: TransactionOutcomeKind::Duplicate,
                });
                ack.duplicate = true;
                Ok(ProcessResult::Acknowledged(ack))
            }
            CommitOutcome::VersionChanged { current } => {
                self.observer.record(MetricEvent::ServerTransaction {
                    outcome: TransactionOutcomeKind::VersionChanged,
                });
                Ok(ProcessResult::Conflict(conflict(
                    operation,
                    current,
                    self.conflicts.policy(operation.operation_kind.0),
                    "the entity changed while this operation was being committed",
                )))
            }
            CommitOutcome::Rejected(rejection) => Ok(ProcessResult::Rejected(rejection)),
        }
    }

    #[allow(clippy::too_many_lines)]
    async fn process(
        &self,
        auth: AuthContext,
        request: SyncRequest,
    ) -> Result<SyncResponse, ServerError> {
        let authority = self.authority.state()?;
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
                next_cursor: request
                    .cursor
                    .unwrap_or_else(|| authority.cursor(request.session.scope_id, Sequence(0))),
                has_more: false,
                server_time: self.clock.now(),
            });
        }
        let validation_started = Instant::now();
        let validation = validate_request(request, self.config.limits);
        self.record_phase(ServerPhaseKind::Validation, validation_started);
        let mut request = validation?.into_inner();
        self.validate_typed_request_budget(&request)?;
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

        if let Some(cursor) = request.cursor {
            match validate_cursor(authority, cursor)? {
                CursorDisposition::Continue => {}
                CursorDisposition::AuthorityChanged { previous, current } => {
                    return Ok(SyncResponse {
                        protocol: request.protocol,
                        directive: SyncDirective::AuthorityChanged {
                            authority_id: authority.authority_id,
                            previous_epoch: previous,
                            current_epoch: current,
                        },
                        acknowledged: Vec::new(),
                        rejected: Vec::new(),
                        conflicts: Vec::new(),
                        changes: Vec::new(),
                        next_cursor: authority.cursor(request.session.scope_id, Sequence(0)),
                        has_more: false,
                        server_time: self.clock.now(),
                    });
                }
            }
        }

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
                next_cursor: authority.cursor(request.session.scope_id, minimum_cursor),
                has_more: false,
                server_time: self.clock.now(),
            });
        }

        let operations = std::mem::take(&mut request.operations);
        let (operations, dependency_plan, command_digests) =
            self.prepare_operations(operations).await?;
        let mut acknowledged = Vec::with_capacity(operations.len());
        let mut rejected = Vec::new();
        let mut conflicts = Vec::new();
        let mut completed = HashSet::new();
        for &operation_index in dependency_plan.ordered_indices() {
            let operation = &operations[operation_index];
            match self
                .process_operation(
                    &auth,
                    operation,
                    &completed,
                    request.session.scope_id,
                    command_digests[operation_index],
                )
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
        let journal_head = page.journal_head;
        let mut response = SyncResponse {
            protocol: request.protocol,
            directive: SyncDirective::Continue,
            acknowledged,
            rejected,
            conflicts,
            changes: page.changes,
            next_cursor: authority.cursor(request.session.scope_id, page.next_sequence),
            has_more: page.has_more,
            server_time: self.clock.now(),
        };
        self.fit_response(&mut response, start, request.limits.max_response_bytes)?;
        self.observer.record(MetricEvent::ServerJournalLag {
            sequences: journal_head
                .0
                .saturating_sub(response.next_cursor.sequence.0),
        });
        Ok(response)
    }

    fn fit_response(
        &self,
        response: &mut SyncResponse,
        start: Sequence,
        maximum_bytes: u32,
    ) -> Result<(), ServerError> {
        let maximum = usize::try_from(maximum_bytes)
            .unwrap_or(usize::MAX)
            .min(self.config.performance.memory.response_bytes);
        loop {
            let encoded = aequora_codec::encode(
                response.protocol,
                aequora_codec::MessageKind::SyncResponse,
                response,
            )?;
            if encoded.len() <= maximum {
                return Ok(());
            }
            if response.changes.pop().is_none() {
                return Err(ServerError::ResponseLimit);
            }
            response.has_more = true;
            response.next_cursor.sequence = response
                .changes
                .last()
                .map_or(start, |change| change.sequence);
            if response.changes.is_empty()
                && response.acknowledged.is_empty()
                && response.rejected.is_empty()
                && response.conflicts.is_empty()
            {
                return Err(ServerError::ResponseLimit);
            }
        }
    }

    fn validate_typed_request_budget(&self, request: &SyncRequest) -> Result<(), ServerError> {
        if request.operations.len() > self.config.performance.memory.pending_decode_records {
            return Err(ServerError::MemoryBudget {
                domain: "pending decode records",
                actual: request.operations.len(),
                maximum: self.config.performance.memory.pending_decode_records,
            });
        }
        let bytes = request
            .operations
            .iter()
            .try_fold(0_usize, |total, operation| {
                total.checked_add(operation.payload.len())
            });
        let actual = bytes.unwrap_or(usize::MAX);
        if actual > self.config.performance.memory.decode_bytes {
            return Err(ServerError::MemoryBudget {
                domain: "typed request payload",
                actual,
                maximum: self.config.performance.memory.decode_bytes,
            });
        }
        Ok(())
    }

    async fn prepare_operations(
        &self,
        operations: Vec<OperationEnvelope>,
    ) -> Result<(Vec<OperationEnvelope>, DependencyPlan, Vec<[u8; 32]>), ServerError> {
        let Some(compute) = &self.compute else {
            let plan = plan_dependencies(&operations)?;
            let digests = operations
                .iter()
                .map(|operation| *blake3::hash(&operation.payload).as_bytes())
                .collect();
            return Ok((operations, plan, digests));
        };
        if !compute.should_parallelize(operations.len()) {
            let plan = plan_dependencies(&operations)?;
            let digests = operations
                .iter()
                .map(|operation| *blake3::hash(&operation.payload).as_bytes())
                .collect();
            return Ok((operations, plan, digests));
        }
        self.observer.record(MetricEvent::ComputeOffload {
            items: usize_to_u64(operations.len()),
        });
        Ok(compute
            .run(move || {
                let plan = plan_dependencies(&operations)?;
                let digests = operations
                    .par_iter()
                    .map(|operation| *blake3::hash(&operation.payload).as_bytes())
                    .collect();
                Ok::<_, DependencyError>((operations, plan, digests))
            })
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
        let authority = self.authority.state()?;
        self.authority.validate_startup()?;
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
                .create_snapshot_in_timeline(
                    auth.tenant_id,
                    request.session.scope_id,
                    &request.session.partitions,
                    authority.authority_id,
                    authority.epoch,
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
                usize::try_from(request.limits.max_entities)
                    .unwrap_or(usize::MAX)
                    .min(self.config.performance.snapshot.max_records_per_chunk),
                usize::try_from(request.limits.max_payload_bytes)
                    .unwrap_or(usize::MAX)
                    .min(self.config.performance.snapshot.max_chunk_bytes),
            )
            .await?;
        self.record_phase(ServerPhaseKind::Database, database_started);
        if page.descriptor.cursor.authority_id != AuthorityId::LEGACY_UNBOUND {
            if page.descriptor.cursor.authority_id != authority.authority_id {
                return Err(AuthorityError::AuthorityIdChanged {
                    trusted: authority.authority_id,
                    presented: page.descriptor.cursor.authority_id,
                }
                .into());
            }
            if page.descriptor.cursor.authority_epoch != authority.epoch {
                return Err(AuthorityError::ArtifactEpochMismatch {
                    artifact: page.descriptor.cursor.authority_epoch,
                    current: authority.epoch,
                }
                .into());
            }
        }
        if page.descriptor.cursor.scope != request.session.scope_id {
            return Err(ServerError::IdentityMismatch);
        }
        if page.has_more && page.entities.is_empty() && page.next_offset == request.offset {
            return Err(ServerError::SnapshotNoProgress);
        }
        Ok(BootstrapResponse {
            protocol: request.protocol,
            snapshot_id: page.descriptor.snapshot_id,
            cursor: authority.cursor(request.session.scope_id, page.descriptor.cursor.sequence),
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

fn development_authority() -> AuthorityController {
    let mut state = AuthorityState::new(
        AuthorityId::LOCAL_DEVELOPMENT,
        AuthorityInstanceId::new(),
        AuthorityRole::Primary,
        0,
    );
    state.runtime_mode = AuthorityRuntimeMode::Serving;
    AuthorityController::new(state, AuthorityPromotionPolicy::default())
}

fn server_result_outcome<T>(result: &Result<T, ServerError>) -> OutcomeKind {
    match result {
        Ok(_) => OutcomeKind::Success,
        Err(ServerError::Store(error)) if error.kind == StoreErrorKind::Transient => {
            OutcomeKind::TransientFailure
        }
        Err(ServerError::Maintenance { .. }) => OutcomeKind::TransientFailure,
        Err(ServerError::Admission(rejection)) if rejection.retryable() => {
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

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::*;

    struct CountingService(AtomicUsize);

    #[async_trait]
    impl ExchangeService for CountingService {
        async fn exchange(
            &self,
            _auth: AuthContext,
            _request: SyncRequest,
        ) -> Result<SyncResponse, ServerError> {
            self.0.fetch_add(1, Ordering::Relaxed);
            Err(ServerError::BootstrapUnavailable)
        }
    }

    fn empty_request(auth: AuthContext) -> SyncRequest {
        SyncRequest {
            protocol: aequora_types::ProtocolVersion::V1,
            request_id: aequora_types::RequestId::new(),
            session: SessionMetadata {
                session_id: aequora_types::SessionId::new(),
                device_id: auth.device_id,
                actor_id: auth.actor_id,
                tenant_id: auth.tenant_id,
                scope_id: aequora_types::SyncScopeId::new(),
                partitions: Vec::new(),
            },
            cursor: None,
            operations: Vec::new(),
            limits: ClientLimits::default(),
            capabilities: Vec::new(),
        }
    }

    #[test]
    fn maintenance_controller_transitions_atomically() {
        let controller = MaintenanceController::default();
        assert_eq!(controller.mode(), MaintenanceMode::Normal);
        assert_eq!(
            controller.set_mode(MaintenanceMode::ReadOnly),
            MaintenanceMode::Normal
        );
        assert_eq!(controller.mode(), MaintenanceMode::ReadOnly);
        assert_eq!(
            controller.set_mode(MaintenanceMode::SyncPaused),
            MaintenanceMode::ReadOnly
        );
        assert_eq!(controller.mode(), MaintenanceMode::SyncPaused);
    }

    #[test]
    fn maintenance_policy_preserves_read_only_pull_and_blocks_writes() {
        assert!(MaintenanceMode::Normal.allows_exchange(false));
        assert!(MaintenanceMode::Normal.allows_exchange(true));
        assert!(MaintenanceMode::ReadOnly.allows_exchange(false));
        assert!(!MaintenanceMode::ReadOnly.allows_exchange(true));
        assert!(!MaintenanceMode::SyncPaused.allows_exchange(false));
        assert!(!MaintenanceMode::SyncPaused.allows_exchange(true));
        assert!(MaintenanceMode::Normal.allows_bootstrap());
        assert!(MaintenanceMode::ReadOnly.allows_bootstrap());
        assert!(!MaintenanceMode::SyncPaused.allows_bootstrap());
    }

    #[tokio::test]
    async fn admission_decorator_rejects_before_inner_service_and_releases_with_raii() {
        let mut policy = aequora_admission::AdmissionPolicy::default();
        policy.global.max_in_flight = 1;
        policy.tenant.budget.max_in_flight = 1;
        policy.tenant.max_tracked_tenants = 1;
        policy.critical.reserved_in_flight = 0;
        policy.interactive.reserved_in_flight = 0;
        policy.normal.reserved_in_flight = 0;
        policy.bulk.reserved_in_flight = 0;
        policy.background.reserved_in_flight = 0;
        policy.maintenance.reserved_in_flight = 0;
        let admission = Arc::new(
            aequora_admission::HierarchicalAdmission::new(policy)
                .unwrap_or_else(|error| panic!("{error}")),
        );
        let inner = Arc::new(CountingService(AtomicUsize::new(0)));
        let service = AdmittedExchangeService::new(Arc::clone(&inner), admission.clone());
        let auth = AuthContext {
            actor_id: aequora_types::ActorId::new(),
            tenant_id: aequora_types::TenantId::new(),
            device_id: aequora_types::DeviceId::new(),
        };
        let request = empty_request(auth);
        let blocker = admission
            .try_admit(&service.exchange_work(&auth, &request))
            .unwrap_or_else(|error| panic!("{error}"));
        assert!(matches!(
            service.exchange(auth, request.clone()).await,
            Err(ServerError::Admission(
                AdmissionRejection::ServerBusy { .. }
            ))
        ));
        assert_eq!(inner.0.load(Ordering::Relaxed), 0);
        drop(blocker);
        assert!(matches!(
            service.exchange(auth, request).await,
            Err(ServerError::BootstrapUnavailable)
        ));
        assert_eq!(inner.0.load(Ordering::Relaxed), 1);
    }
}
