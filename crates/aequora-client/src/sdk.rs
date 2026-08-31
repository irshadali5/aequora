//! High-level, misuse-resistant application client SDK.

use aequora_adapter_sdk::{
    AdapterError, AdapterErrorKind, ClientIdentity, ClientStore, ConflictId, ConflictResolution,
    ConflictSummary, CredentialProvider, DomainId, DurableClientStatus, ExchangeContext,
    NoopObservationSink, ObservationSink, OperationSnapshot, SyncTransport,
};
use aequora_operation::{
    EncodedOperation, MutationReceipt, Operation, OperationEncodingError, OperationState,
    OperationUpdate,
};
use aequora_types::{OperationId, SyncScopeId};
use std::{
    fmt,
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};
use thiserror::Error;
use tokio::sync::broadcast;

/// Stable public error code, independent from error text and internal sources.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
#[non_exhaustive]
pub enum AequoraErrorCode {
    /// Durable storage failed.
    StorageUnavailable,
    /// A bounded transport exchange failed.
    TransportUnavailable,
    /// Authentication or credential acquisition failed.
    AuthenticationRequired,
    /// Authorization rejected the requested operation.
    AuthorizationDenied,
    /// A semantic conflict requires explicit handling.
    Conflict,
    /// Public input or configuration is invalid.
    Validation,
    /// A bounded resource limit rejected work.
    Backpressure,
    /// The configured runtime does not provide this optional capability.
    UnsupportedCapability,
    /// Client has begun or completed shutdown.
    Closed,
    /// An internal failure has no more specific stable category.
    Internal,
}

impl AequoraErrorCode {
    /// Stable machine-readable compatibility code.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::StorageUnavailable => "AEQ-SDK-STORAGE-001",
            Self::TransportUnavailable => "AEQ-SDK-TRANSPORT-001",
            Self::AuthenticationRequired => "AEQ-SDK-AUTHN-001",
            Self::AuthorizationDenied => "AEQ-SDK-AUTHZ-001",
            Self::Conflict => "AEQ-SDK-CONFLICT-001",
            Self::Validation => "AEQ-SDK-VALIDATION-001",
            Self::Backpressure => "AEQ-SDK-RESOURCE-001",
            Self::UnsupportedCapability => "AEQ-SDK-CAPABILITY-001",
            Self::Closed => "AEQ-SDK-CLOSED-001",
            Self::Internal => "AEQ-SDK-INTERNAL-001",
        }
    }
}

impl fmt::Display for AequoraErrorCode {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// Stable retry classification for application policy and FFI consumers.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RetryClass {
    /// Retrying without replacing user intent may succeed.
    Retryable,
    /// User action or new semantic intent is required.
    UserActionRequired,
    /// Retrying the same request is not useful.
    NonRetryable,
}

/// One top-level SDK error with semver-stable categories and codes.
#[derive(Clone, Debug, Error, Eq, PartialEq)]
#[non_exhaustive]
pub enum AequoraError {
    /// Stable adapter-boundary failure.
    #[error("{code}: {source}")]
    Adapter {
        /// Stable public code.
        code: AequoraErrorCode,
        /// Redacted adapter source.
        #[source]
        source: AdapterError,
    },
    /// Application operation could not be encoded.
    #[error("{code}: {source}")]
    OperationEncoding {
        /// Stable public code.
        code: AequoraErrorCode,
        /// Redacted operation encoder source.
        #[source]
        source: OperationEncodingError,
    },
    /// Builder is missing required configuration.
    #[error("{code}: required client field `{field}` is missing")]
    MissingConfiguration {
        /// Stable public code.
        code: AequoraErrorCode,
        /// Missing semantic field.
        field: &'static str,
    },
    /// Operation did not reach an authoritative terminal state within policy.
    #[error("{code}: authoritative confirmation timed out")]
    ConfirmationTimeout {
        /// Stable public code.
        code: AequoraErrorCode,
    },
    /// Runtime is closing or closed.
    #[error("{code}: client is closed")]
    Closed {
        /// Stable public code.
        code: AequoraErrorCode,
    },
    /// Another bounded synchronous attempt already owns the single-flight reservation.
    #[error("{code}: a synchronization attempt is already running")]
    SyncInProgress {
        /// Stable public code.
        code: AequoraErrorCode,
    },
}

impl AequoraError {
    /// Stable machine-readable code.
    #[must_use]
    pub const fn code(&self) -> AequoraErrorCode {
        match self {
            Self::Adapter { code, .. }
            | Self::OperationEncoding { code, .. }
            | Self::MissingConfiguration { code, .. }
            | Self::ConfirmationTimeout { code }
            | Self::Closed { code }
            | Self::SyncInProgress { code } => *code,
        }
    }

    /// Stable retry classification.
    #[must_use]
    pub const fn retry_class(&self) -> RetryClass {
        match self.code() {
            AequoraErrorCode::StorageUnavailable
            | AequoraErrorCode::TransportUnavailable
            | AequoraErrorCode::Backpressure
            | AequoraErrorCode::Internal => RetryClass::Retryable,
            AequoraErrorCode::AuthenticationRequired
            | AequoraErrorCode::AuthorizationDenied
            | AequoraErrorCode::Conflict => RetryClass::UserActionRequired,
            AequoraErrorCode::Validation
            | AequoraErrorCode::UnsupportedCapability
            | AequoraErrorCode::Closed => RetryClass::NonRetryable,
        }
    }

    fn from_adapter(error: AdapterError) -> Self {
        let code = match error.kind() {
            AdapterErrorKind::Storage => AequoraErrorCode::StorageUnavailable,
            AdapterErrorKind::Transport => AequoraErrorCode::TransportUnavailable,
            AdapterErrorKind::Authentication => AequoraErrorCode::AuthenticationRequired,
            AdapterErrorKind::Validation => AequoraErrorCode::Validation,
            AdapterErrorKind::Backpressure => AequoraErrorCode::Backpressure,
            AdapterErrorKind::UnsupportedCapability => AequoraErrorCode::UnsupportedCapability,
            _ => AequoraErrorCode::Internal,
        };
        Self::Adapter {
            code,
            source: error,
        }
    }

    const fn missing(field: &'static str) -> Self {
        Self::MissingConfiguration {
            code: AequoraErrorCode::Validation,
            field,
        }
    }
}

/// High-level current synchronization state. Internal planner phases are intentionally absent.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum SyncStatus {
    /// Transport is currently unavailable.
    Offline,
    /// No durable work is pending.
    Idle,
    /// Durable operations await authoritative disposition.
    Pending {
        /// Number of pending operations.
        operations: u64,
    },
    /// One bounded exchange is active.
    Syncing,
    /// At least one semantic conflict requires resolution.
    Conflict,
    /// Store requires a fresh snapshot bootstrap.
    NeedsBootstrap,
    /// New credentials or user authentication are required.
    AuthenticationRequired,
    /// Runtime compatibility requires an SDK/application upgrade.
    UpgradeRequired,
    /// Storage pressure currently rejects additional durable work.
    StorageBlocked,
    /// Client is shutting down or closed.
    Closed,
}

/// Next safe action after a bounded synchronization attempt.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum SyncNextAction {
    /// No immediate follow-up is required.
    Idle,
    /// Scheduler may retry after this lower bound.
    RetryAfter(Duration),
    /// A fresh snapshot bootstrap is required.
    NeedsBootstrap,
    /// A compatible application/SDK upgrade is required.
    UpgradeRequired,
    /// New credentials or user authentication are required.
    AuthenticationRequired,
}

/// Stable summary of one bounded synchronization attempt.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SyncResult {
    /// Operations authoritatively disposed.
    pub pushed: u64,
    /// Authoritative changes applied locally.
    pub pulled: u64,
    /// Conflicts created during the attempt.
    pub conflicts: u64,
    /// Next safe scheduler/application action.
    pub next_action: SyncNextAction,
}

/// Bounded scheduler hint. It cannot bypass admission or retry limits.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum SyncReason {
    /// Explicit foreground user action.
    UserInitiated,
    /// Network availability changed.
    ConnectivityChanged,
    /// Application returned to an active lifecycle state.
    AppResumed,
    /// Advisory server hint suggested durable state may have changed.
    ServerHint,
}

/// Redacted advisory data-change notification.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DataChange {
    /// Scope whose durable state may have changed.
    pub scope_id: SyncScopeId,
}

/// Bounded bootstrap progress hint. Durable bootstrap state remains queryable.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BootstrapProgress {
    /// Completed bounded units.
    pub completed: u64,
    /// Total units when known.
    pub total: Option<u64>,
}

/// Best-effort UI notification. Missing an event never loses durable state.
#[derive(Clone, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum SyncEvent {
    /// High-level status changed.
    StatusChanged(SyncStatus),
    /// Durable data in one scope may have changed.
    DataChanged(DataChange),
    /// A semantic conflict was created.
    ConflictCreated(ConflictId),
    /// Operation lifecycle changed.
    OperationUpdated(OperationUpdate),
    /// Bootstrap made bounded progress.
    BootstrapProgress(BootstrapProgress),
    /// A scheduler hint was accepted.
    SyncRequested(SyncReason),
    /// Graceful shutdown completed.
    Closed,
}

/// Event receiver with explicit best-effort lag semantics.
pub struct EventStream {
    receiver: broadcast::Receiver<SyncEvent>,
}

impl EventStream {
    /// Waits for the next advisory event. Lagged messages are skipped because durable state is
    /// available from [`AequoraClient::status`] and grouped handles.
    #[must_use]
    pub async fn next(&mut self) -> Option<SyncEvent> {
        loop {
            match self.receiver.recv().await {
                Ok(event) => return Some(event),
                Err(broadcast::error::RecvError::Lagged(_)) => {}
                Err(broadcast::error::RecvError::Closed) => return None,
            }
        }
    }
}

/// Bounded high-level client policy.
#[derive(Clone, Copy, Debug)]
pub struct ClientSdkConfig {
    /// Advisory event queue capacity per client runtime.
    pub event_capacity: usize,
    /// Maximum conflicts returned by one list call.
    pub conflict_page_size: usize,
    /// Maximum time an authoritative wait helper may poll.
    pub confirmation_timeout: Duration,
    /// Delay between durable operation-state polls.
    pub confirmation_poll_interval: Duration,
}

impl Default for ClientSdkConfig {
    fn default() -> Self {
        Self {
            event_capacity: 64,
            conflict_page_size: 100,
            confirmation_timeout: Duration::from_secs(30),
            confirmation_poll_interval: Duration::from_millis(250),
        }
    }
}

impl ClientSdkConfig {
    fn validate(self) -> Result<Self, AequoraError> {
        if self.event_capacity == 0 {
            return Err(AequoraError::missing("config.event_capacity"));
        }
        if self.conflict_page_size == 0 {
            return Err(AequoraError::missing("config.conflict_page_size"));
        }
        if self.confirmation_timeout.is_zero() || self.confirmation_poll_interval.is_zero() {
            return Err(AequoraError::missing("config.confirmation_timeout"));
        }
        Ok(self)
    }
}

/// Runtime-validated builder for the stable high-level client.
#[derive(Default)]
pub struct AequoraClientBuilder {
    store: Option<Arc<dyn ClientStore>>,
    transport: Option<Arc<dyn SyncTransport>>,
    credentials: Option<Arc<dyn CredentialProvider>>,
    identity: Option<ClientIdentity>,
    domain: Option<DomainId>,
    observer: Option<Arc<dyn ObservationSink>>,
    config: ClientSdkConfig,
}

impl AequoraClientBuilder {
    /// Installs the cancellation-safe local adapter.
    #[must_use]
    pub fn store(mut self, store: impl ClientStore + 'static) -> Self {
        self.store = Some(Arc::new(store));
        self
    }

    /// Installs a pre-shared local adapter handle.
    #[must_use]
    pub fn shared_store(mut self, store: Arc<dyn ClientStore>) -> Self {
        self.store = Some(store);
        self
    }

    /// Installs the bounded synchronization transport.
    #[must_use]
    pub fn transport(mut self, transport: impl SyncTransport + 'static) -> Self {
        self.transport = Some(Arc::new(transport));
        self
    }

    /// Installs a pre-shared transport handle.
    #[must_use]
    pub fn shared_transport(mut self, transport: Arc<dyn SyncTransport>) -> Self {
        self.transport = Some(transport);
        self
    }

    /// Installs a credential provider. Raw HTTP authorization headers are never exposed.
    #[must_use]
    pub fn credentials(mut self, provider: impl CredentialProvider + 'static) -> Self {
        self.credentials = Some(Arc::new(provider));
        self
    }

    /// Installs a pre-shared credential provider.
    #[must_use]
    pub fn shared_credentials(mut self, provider: Arc<dyn CredentialProvider>) -> Self {
        self.credentials = Some(provider);
        self
    }

    /// Sets explicit tenant and device identity.
    #[must_use]
    pub const fn identity(mut self, identity: ClientIdentity) -> Self {
        self.identity = Some(identity);
        self
    }

    /// Sets the application registry/domain identity.
    #[must_use]
    pub fn domain(mut self, domain: DomainId) -> Self {
        self.domain = Some(domain);
        self
    }

    /// Installs a non-blocking observability hook.
    #[must_use]
    pub fn observer(mut self, observer: impl ObservationSink + 'static) -> Self {
        self.observer = Some(Arc::new(observer));
        self
    }

    /// Applies bounded SDK policy.
    #[must_use]
    pub const fn config(mut self, config: ClientSdkConfig) -> Self {
        self.config = config;
        self
    }

    /// Validates required semantic fields and creates one shared runtime.
    ///
    /// # Errors
    ///
    /// Returns a stable validation error naming the first missing or invalid field.
    #[allow(clippy::unused_async)]
    pub async fn build(self) -> Result<AequoraClient, AequoraError> {
        let config = self.config.validate()?;
        let (events, _) = broadcast::channel(config.event_capacity);
        Ok(AequoraClient {
            inner: Arc::new(ClientInner {
                store: self.store.ok_or_else(|| AequoraError::missing("store"))?,
                transport: self
                    .transport
                    .ok_or_else(|| AequoraError::missing("transport"))?,
                credentials: self
                    .credentials
                    .ok_or_else(|| AequoraError::missing("credentials"))?,
                identity: self
                    .identity
                    .ok_or_else(|| AequoraError::missing("identity"))?,
                domain: self.domain.ok_or_else(|| AequoraError::missing("domain"))?,
                observer: self
                    .observer
                    .unwrap_or_else(|| Arc::new(NoopObservationSink)),
                events,
                syncing: AtomicBool::new(false),
                closed: AtomicBool::new(false),
                sync_requested: AtomicBool::new(false),
                config,
            }),
        })
    }
}

struct ClientInner {
    store: Arc<dyn ClientStore>,
    transport: Arc<dyn SyncTransport>,
    credentials: Arc<dyn CredentialProvider>,
    identity: ClientIdentity,
    domain: DomainId,
    observer: Arc<dyn ObservationSink>,
    events: broadcast::Sender<SyncEvent>,
    syncing: AtomicBool,
    closed: AtomicBool,
    sync_requested: AtomicBool,
    config: ClientSdkConfig,
}

impl ClientInner {
    fn ensure_open(&self) -> Result<(), AequoraError> {
        if self.closed.load(Ordering::Acquire) {
            Err(AequoraError::Closed {
                code: AequoraErrorCode::Closed,
            })
        } else {
            Ok(())
        }
    }

    fn notify(&self, event: SyncEvent) {
        let _ignored = self.events.send(event);
    }
}

/// Cheaply clonable reference to one client runtime.
#[derive(Clone)]
pub struct AequoraClient {
    inner: Arc<ClientInner>,
}

impl fmt::Debug for AequoraClient {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("AequoraClient")
            .field("domain", &self.inner.domain)
            .field("closed", &self.inner.closed.load(Ordering::Relaxed))
            .finish_non_exhaustive()
    }
}

impl AequoraClient {
    /// Starts a runtime-validated public SDK builder.
    #[must_use]
    pub fn builder() -> AequoraClientBuilder {
        AequoraClientBuilder::default()
    }

    /// Durably commits application intent locally. The receipt never claims server acceptance.
    ///
    /// # Errors
    ///
    /// Returns a stable encoding, storage, resource, or lifecycle error.
    pub async fn mutate<O: Operation>(
        &self,
        operation: O,
    ) -> Result<MutationReceipt, AequoraError> {
        self.inner.ensure_open()?;
        let encoded = EncodedOperation::from_operation(&operation).map_err(|source| {
            AequoraError::OperationEncoding {
                code: AequoraErrorCode::Validation,
                source,
            }
        })?;
        let operation_id = encoded.operation_id();
        let local_status = self
            .inner
            .store
            .commit_operation(encoded)
            .await
            .map_err(AequoraError::from_adapter)?;
        self.inner
            .notify(SyncEvent::OperationUpdated(OperationUpdate {
                operation_id,
                state: OperationState::Pending,
            }));
        Ok(MutationReceipt::new(operation_id, local_status))
    }

    /// Runs and awaits one bounded single-flight synchronization attempt.
    ///
    /// # Errors
    ///
    /// Returns a stable lifecycle, credential, transport, storage, or resource error.
    pub async fn sync_now(&self) -> Result<SyncResult, AequoraError> {
        self.inner.ensure_open()?;
        if self
            .inner
            .syncing
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
        {
            return Err(AequoraError::SyncInProgress {
                code: AequoraErrorCode::Backpressure,
            });
        }
        let _reservation = SyncReservation(&self.inner.syncing);
        self.inner.sync_requested.store(false, Ordering::Release);
        self.inner.observer.on_status("syncing");
        self.inner
            .notify(SyncEvent::StatusChanged(SyncStatus::Syncing));

        let credential = self
            .inner
            .credentials
            .credential()
            .await
            .map_err(AequoraError::from_adapter)?;
        let summary = self
            .inner
            .transport
            .exchange(ExchangeContext {
                identity: self.inner.identity,
                domain: &self.inner.domain,
                credential: &credential,
            })
            .await
            .map_err(AequoraError::from_adapter)?;
        let next_action = if summary.upgrade_required {
            SyncNextAction::UpgradeRequired
        } else if summary.needs_bootstrap {
            SyncNextAction::NeedsBootstrap
        } else if let Some(delay) = summary.retry_after {
            SyncNextAction::RetryAfter(delay)
        } else {
            SyncNextAction::Idle
        };
        let result = SyncResult {
            pushed: summary.pushed,
            pulled: summary.pulled,
            conflicts: summary.conflicts,
            next_action,
        };
        let status = self.status().await?;
        self.inner.observer.on_status("idle");
        self.inner.notify(SyncEvent::StatusChanged(status));
        Ok(result)
    }

    /// Records a bounded scheduler hint without running work inline.
    #[must_use]
    pub fn request_sync(&self, reason: SyncReason) -> bool {
        if self.inner.closed.load(Ordering::Acquire) {
            return false;
        }
        let newly_requested = !self.inner.sync_requested.swap(true, Ordering::AcqRel);
        if newly_requested {
            self.inner.notify(SyncEvent::SyncRequested(reason));
        }
        newly_requested
    }

    /// Queries current state from durable storage; event delivery is not required.
    ///
    /// # Errors
    ///
    /// Returns a stable storage or lifecycle error.
    pub async fn status(&self) -> Result<SyncStatus, AequoraError> {
        if self.inner.closed.load(Ordering::Acquire) {
            return Ok(SyncStatus::Closed);
        }
        if self.inner.syncing.load(Ordering::Acquire) {
            return Ok(SyncStatus::Syncing);
        }
        let durable = self
            .inner
            .store
            .status()
            .await
            .map_err(AequoraError::from_adapter)?;
        Ok(status_from_durable(durable))
    }

    /// Subscribes to bounded best-effort UI events.
    #[must_use]
    pub fn events(&self) -> EventStream {
        EventStream {
            receiver: self.inner.events.subscribe(),
        }
    }

    /// Groups operation inspection and confirmation helpers.
    #[must_use]
    pub fn operations(&self) -> OperationHandle {
        OperationHandle {
            inner: Arc::clone(&self.inner),
        }
    }

    /// Groups semantic conflict APIs.
    #[must_use]
    pub fn conflicts(&self) -> ConflictHandle {
        ConflictHandle {
            inner: Arc::clone(&self.inner),
        }
    }

    /// Groups partial-synchronization scope APIs.
    #[must_use]
    pub fn scopes(&self) -> ScopeHandle {
        ScopeHandle {
            inner: Arc::clone(&self.inner),
        }
    }

    /// Groups optional high-level blob APIs.
    #[must_use]
    pub fn blobs(&self) -> BlobHandle {
        BlobHandle {
            inner: Arc::clone(&self.inner),
        }
    }

    /// Groups bounded, redacted diagnostics APIs.
    #[must_use]
    pub fn diagnostics(&self) -> DiagnosticsHandle {
        DiagnosticsHandle {
            inner: Arc::clone(&self.inner),
        }
    }

    /// Requests graceful shutdown of this shared runtime.
    ///
    /// # Errors
    ///
    /// Currently infallible; the `Result` permits future bounded shutdown providers without a
    /// breaking signature change.
    #[allow(clippy::unused_async)]
    pub async fn shutdown(&self) -> Result<(), AequoraError> {
        if !self.inner.closed.swap(true, Ordering::AcqRel) {
            self.inner
                .notify(SyncEvent::StatusChanged(SyncStatus::Closed));
            self.inner.notify(SyncEvent::Closed);
            self.inner.observer.on_status("closed");
        }
        Ok(())
    }
}

struct SyncReservation<'a>(&'a AtomicBool);

impl Drop for SyncReservation<'_> {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Release);
    }
}

const fn status_from_durable(status: DurableClientStatus) -> SyncStatus {
    if status.storage_blocked {
        SyncStatus::StorageBlocked
    } else if status.needs_bootstrap {
        SyncStatus::NeedsBootstrap
    } else if status.conflicts > 0 {
        SyncStatus::Conflict
    } else if status.pending_operations > 0 {
        SyncStatus::Pending {
            operations: status.pending_operations,
        }
    } else {
        SyncStatus::Idle
    }
}

/// Operation inspection namespace.
#[derive(Clone)]
pub struct OperationHandle {
    inner: Arc<ClientInner>,
}

impl OperationHandle {
    /// Queries one durable operation.
    ///
    /// # Errors
    ///
    /// Returns a stable storage or lifecycle error.
    pub async fn get(
        &self,
        operation_id: OperationId,
    ) -> Result<Option<OperationRef>, AequoraError> {
        self.inner.ensure_open()?;
        self.inner
            .store
            .operation(operation_id)
            .await
            .map(|snapshot| {
                snapshot.map(|snapshot| OperationRef {
                    inner: Arc::clone(&self.inner),
                    snapshot,
                })
            })
            .map_err(AequoraError::from_adapter)
    }
}

/// One operation plus bounded authoritative-confirmation helper.
#[derive(Clone)]
pub struct OperationRef {
    inner: Arc<ClientInner>,
    snapshot: OperationSnapshot,
}

impl OperationRef {
    /// Latest durable snapshot observed when this value was created.
    #[must_use]
    pub const fn snapshot(&self) -> OperationSnapshot {
        self.snapshot
    }

    /// Waits until the operation reaches an authoritative terminal state or policy expires.
    ///
    /// # Errors
    ///
    /// Returns a stable storage, lifecycle, or timeout error.
    pub async fn await_authoritative(self) -> Result<OperationSnapshot, AequoraError> {
        let started = Instant::now();
        loop {
            self.inner.ensure_open()?;
            let snapshot = self
                .inner
                .store
                .operation(self.snapshot.operation_id)
                .await
                .map_err(AequoraError::from_adapter)?
                .ok_or_else(|| {
                    AequoraError::from_adapter(AdapterError::new(
                        AdapterErrorKind::Storage,
                        "durable operation disappeared",
                    ))
                })?;
            if matches!(
                snapshot.state,
                OperationState::AuthoritativeAccepted
                    | OperationState::Rejected
                    | OperationState::Conflict
                    | OperationState::Superseded
            ) {
                return Ok(snapshot);
            }
            if started.elapsed() >= self.inner.config.confirmation_timeout {
                return Err(AequoraError::ConfirmationTimeout {
                    code: AequoraErrorCode::TransportUnavailable,
                });
            }
            tokio::time::sleep(self.inner.config.confirmation_poll_interval).await;
        }
    }
}

/// Semantic conflict namespace.
#[derive(Clone)]
pub struct ConflictHandle {
    inner: Arc<ClientInner>,
}

impl ConflictHandle {
    /// Lists a bounded page of unresolved conflicts.
    ///
    /// # Errors
    ///
    /// Returns a stable storage or lifecycle error.
    pub async fn list(&self) -> Result<Vec<ConflictSummary>, AequoraError> {
        self.inner.ensure_open()?;
        self.inner
            .store
            .conflicts(self.inner.config.conflict_page_size)
            .await
            .map_err(AequoraError::from_adapter)
    }

    /// Loads one unresolved conflict.
    ///
    /// # Errors
    ///
    /// Returns a stable storage or lifecycle error.
    pub async fn get(&self, id: ConflictId) -> Result<Option<ConflictSummary>, AequoraError> {
        self.inner.ensure_open()?;
        self.inner
            .store
            .conflict(id)
            .await
            .map_err(AequoraError::from_adapter)
    }

    /// Persists semantic resolution intent without editing internal conflict or ledger rows.
    ///
    /// # Errors
    ///
    /// Returns a stable storage, validation, resource, or lifecycle error.
    pub async fn resolve(
        &self,
        id: ConflictId,
        resolution: ConflictResolution,
    ) -> Result<OperationId, AequoraError> {
        self.inner.ensure_open()?;
        self.inner
            .store
            .resolve_conflict(id, resolution)
            .await
            .map_err(AequoraError::from_adapter)
    }
}

/// Public result of a scope subscription request.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum ScopeSubscriptionState {
    /// Durable request accepted and awaiting scheduler work.
    Requested,
    /// Snapshot/bootstrap work is active.
    Bootstrapping,
    /// Scope is active.
    Active,
}

/// Partial-synchronization scope namespace.
#[derive(Clone)]
pub struct ScopeHandle {
    inner: Arc<ClientInner>,
}

impl ScopeHandle {
    /// Persists a semantic subscription request.
    ///
    /// # Errors
    ///
    /// Returns a stable storage, authorization, resource, or lifecycle error.
    pub async fn subscribe(
        &self,
        scope: SyncScopeId,
    ) -> Result<ScopeSubscriptionState, AequoraError> {
        self.inner.ensure_open()?;
        self.inner
            .store
            .subscribe_scope(scope)
            .await
            .map_err(AequoraError::from_adapter)?;
        Ok(ScopeSubscriptionState::Requested)
    }

    /// Persists semantic unsubscription without asserting domain deletion.
    ///
    /// # Errors
    ///
    /// Returns a stable storage, authorization, resource, or lifecycle error.
    pub async fn unsubscribe(&self, scope: SyncScopeId) -> Result<(), AequoraError> {
        self.inner.ensure_open()?;
        self.inner
            .store
            .unsubscribe_scope(scope)
            .await
            .map_err(AequoraError::from_adapter)
    }
}

/// Optional blob namespace. Raw storage handles are never exposed.
#[derive(Clone)]
pub struct BlobHandle {
    inner: Arc<ClientInner>,
}

impl BlobHandle {
    /// Imports a path through the configured blob capability.
    ///
    /// # Errors
    ///
    /// Returns [`AequoraErrorCode::UnsupportedCapability`] until a blob provider is installed.
    #[allow(clippy::unused_async)]
    pub async fn import(&self, path: impl AsRef<Path>) -> Result<PathBuf, AequoraError> {
        self.inner.ensure_open()?;
        let _path = path.as_ref();
        Err(AequoraError::from_adapter(AdapterError::new(
            AdapterErrorKind::UnsupportedCapability,
            "blob provider is not configured",
        )))
    }
}

/// Bounded diagnostics namespace.
#[derive(Clone)]
pub struct DiagnosticsHandle {
    inner: Arc<ClientInner>,
}

impl DiagnosticsHandle {
    /// Returns a redacted bounded summary.
    ///
    /// # Errors
    ///
    /// Returns a stable storage or lifecycle error.
    pub async fn summary(&self) -> Result<Arc<str>, AequoraError> {
        self.inner.ensure_open()?;
        self.inner
            .store
            .diagnostic_summary()
            .await
            .map_err(AequoraError::from_adapter)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aequora_adapter_sdk::{Credential, ExchangeSummary};
    use aequora_operation::{LocalCommitStatus, OperationKind, OperationSchemaVersion};
    use aequora_types::{DeviceId, TenantId};
    use async_trait::async_trait;
    use std::{collections::BTreeMap, sync::Mutex};

    #[derive(Default)]
    struct MemoryStore {
        operations: Mutex<BTreeMap<OperationId, OperationState>>,
    }

    #[async_trait]
    impl ClientStore for MemoryStore {
        async fn commit_operation(
            &self,
            operation: EncodedOperation,
        ) -> Result<LocalCommitStatus, AdapterError> {
            self.operations
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .insert(operation.operation_id(), OperationState::Pending);
            Ok(LocalCommitStatus::SavedLocally)
        }

        async fn operation(
            &self,
            operation_id: OperationId,
        ) -> Result<Option<OperationSnapshot>, AdapterError> {
            Ok(self
                .operations
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .get(&operation_id)
                .copied()
                .map(|state| OperationSnapshot {
                    operation_id,
                    state,
                }))
        }

        async fn status(&self) -> Result<DurableClientStatus, AdapterError> {
            Ok(DurableClientStatus {
                pending_operations: self
                    .operations
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .len() as u64,
                ..DurableClientStatus::default()
            })
        }

        async fn conflicts(&self, _limit: usize) -> Result<Vec<ConflictSummary>, AdapterError> {
            Ok(Vec::new())
        }

        async fn conflict(
            &self,
            _conflict_id: ConflictId,
        ) -> Result<Option<ConflictSummary>, AdapterError> {
            Ok(None)
        }

        async fn resolve_conflict(
            &self,
            _conflict_id: ConflictId,
            _resolution: ConflictResolution,
        ) -> Result<OperationId, AdapterError> {
            Err(AdapterError::new(
                AdapterErrorKind::UnsupportedCapability,
                "no conflicts",
            ))
        }

        async fn subscribe_scope(&self, _scope: SyncScopeId) -> Result<(), AdapterError> {
            Ok(())
        }

        async fn unsubscribe_scope(&self, _scope: SyncScopeId) -> Result<(), AdapterError> {
            Ok(())
        }

        async fn diagnostic_summary(&self) -> Result<Arc<str>, AdapterError> {
            Ok(Arc::from("healthy"))
        }
    }

    struct Transport;

    #[async_trait]
    impl SyncTransport for Transport {
        async fn exchange(
            &self,
            _context: ExchangeContext<'_>,
        ) -> Result<ExchangeSummary, AdapterError> {
            Ok(ExchangeSummary::default())
        }
    }

    struct Credentials;

    #[async_trait]
    impl CredentialProvider for Credentials {
        async fn credential(&self) -> Result<Credential, AdapterError> {
            Ok(Credential::new("test-only"))
        }
    }

    struct CreateStudent;

    impl Operation for CreateStudent {
        type Outcome = ();

        const KIND: OperationKind = OperationKind::from_static(1001);
        const SCHEMA_VERSION: OperationSchemaVersion = OperationSchemaVersion::from_static(1);

        fn encode(&self) -> Result<Vec<u8>, OperationEncodingError> {
            Ok(vec![1])
        }
    }

    async fn client() -> AequoraClient {
        AequoraClient::builder()
            .store(MemoryStore::default())
            .transport(Transport)
            .credentials(Credentials)
            .identity(ClientIdentity {
                tenant_id: TenantId::new(),
                device_id: DeviceId::new(),
            })
            .domain(DomainId::new("school").unwrap_or_else(|error| {
                panic!("domain should be valid: {error}");
            }))
            .build()
            .await
            .unwrap_or_else(|error| panic!("client should build: {error}"))
    }

    #[tokio::test]
    async fn mutation_receipt_is_local_only_and_state_remains_queryable() {
        let client = client().await;
        let mut events = client.events();
        let receipt = client
            .mutate(CreateStudent)
            .await
            .unwrap_or_else(|error| panic!("mutation should persist: {error}"));
        assert_eq!(receipt.local_status(), LocalCommitStatus::SavedLocally);
        assert_eq!(
            client.status().await,
            Ok(SyncStatus::Pending { operations: 1 })
        );
        assert!(matches!(
            events.next().await,
            Some(SyncEvent::OperationUpdated(_))
        ));
    }

    #[tokio::test]
    async fn cloned_handle_shares_runtime_and_shutdown_is_explicit() {
        let client = client().await;
        let clone = client.clone();
        clone
            .shutdown()
            .await
            .unwrap_or_else(|error| panic!("shutdown should succeed: {error}"));
        assert_eq!(client.status().await, Ok(SyncStatus::Closed));
        assert!(matches!(
            client.mutate(CreateStudent).await,
            Err(AequoraError::Closed { .. })
        ));
    }
}
