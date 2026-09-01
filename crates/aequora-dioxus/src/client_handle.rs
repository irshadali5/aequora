use crate::{BootstrapState, Connectivity, LifecycleState, PlatformEvent, QueryKey, SyncStatus};
use aequora_client::{AequoraClient, AequoraError, SyncEvent, SyncReason};
use aequora_types::SyncScopeId;
use std::{fmt, sync::Arc};
use tokio::sync::watch;

const INITIAL_GENERATION: u64 = 1;

/// Unique identity of one active local-store/query-cache boundary.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct QueryNamespace(Arc<str>);

impl QueryNamespace {
    #[must_use]
    pub fn new(value: impl Into<Arc<str>>) -> Self {
        Self(value.into())
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Bounded advisory reason for rereading durable local state.
#[derive(Clone, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum Invalidation {
    All,
    Key(QueryKey),
    Scope(SyncScopeId),
    Conflicts,
    Diagnostics,
}

impl Invalidation {
    fn matches(&self, key: &QueryKey) -> bool {
        match self {
            Self::All => true,
            Self::Key(invalidated) => invalidated == key,
            Self::Scope(scope_id) => key.scope_id() == Some(*scope_id),
            Self::Conflicts => key.value() == "aequora:conflicts",
            Self::Diagnostics => key.value() == "aequora:diagnostics",
        }
    }
}

#[derive(Clone, Debug)]
struct InvalidationNotice {
    generation: u64,
    namespace: QueryNamespace,
    invalidation: Invalidation,
}

/// Latest-value invalidation receiver. If intermediate notices are coalesced, it conservatively
/// refreshes the query rather than risking a stale cache decision.
#[derive(Clone)]
pub struct InvalidationReceiver {
    receiver: watch::Receiver<InvalidationNotice>,
    namespace: QueryNamespace,
    last_generation: u64,
}

impl InvalidationReceiver {
    /// Waits until this query should be reread. Closure of the provider ends the stream.
    pub async fn changed(&mut self, key: &QueryKey) -> bool {
        loop {
            if self.receiver.changed().await.is_err() {
                return false;
            }
            let notice = self.receiver.borrow_and_update().clone();
            let skipped = notice.generation != self.last_generation.wrapping_add(1);
            self.last_generation = notice.generation;
            if notice.namespace == self.namespace && (skipped || notice.invalidation.matches(key)) {
                return true;
            }
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct PlatformState {
    pub(crate) connectivity: Connectivity,
    pub(crate) lifecycle: LifecycleState,
}

struct HandleInner {
    client: AequoraClient,
    namespace: QueryNamespace,
    invalidations: watch::Sender<InvalidationNotice>,
    status: watch::Sender<SyncStatus>,
    events: watch::Sender<Option<SyncEvent>>,
    bootstrap: watch::Sender<BootstrapState>,
    platform: watch::Sender<PlatformState>,
}

/// Cheap cloneable Dioxus-facing reference to one Aequora client and local store.
#[derive(Clone)]
pub struct AequoraHandle(Arc<HandleInner>);

impl fmt::Debug for AequoraHandle {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("AequoraHandle")
            .field("namespace", &self.0.namespace)
            .finish_non_exhaustive()
    }
}

impl AequoraHandle {
    #[must_use]
    pub fn new(client: AequoraClient, namespace: QueryNamespace) -> Self {
        let initial_notice = InvalidationNotice {
            generation: INITIAL_GENERATION,
            namespace: namespace.clone(),
            invalidation: Invalidation::All,
        };
        let (invalidations, _) = watch::channel(initial_notice);
        let (status, _) = watch::channel(SyncStatus::Dormant);
        let (events, _) = watch::channel(None);
        let (bootstrap, _) = watch::channel(BootstrapState::Dormant);
        let (platform, _) = watch::channel(PlatformState {
            connectivity: Connectivity::Unknown,
            lifecycle: LifecycleState::Foreground,
        });
        Self(Arc::new(HandleInner {
            client,
            namespace,
            invalidations,
            status,
            events,
            bootstrap,
            platform,
        }))
    }

    #[must_use]
    pub fn client(&self) -> AequoraClient {
        self.0.client.clone()
    }

    #[must_use]
    pub fn namespace(&self) -> &QueryNamespace {
        &self.0.namespace
    }

    /// Emits a best-effort post-commit query hint. It must not be called before the underlying
    /// mutation/query transaction has committed.
    pub fn invalidate(&self, invalidation: Invalidation) {
        self.0.invalidations.send_modify(|notice| {
            notice.generation = notice.generation.wrapping_add(1);
            notice.namespace.clone_from(&self.0.namespace);
            notice.invalidation = invalidation;
        });
    }

    #[must_use]
    pub fn invalidations(&self) -> InvalidationReceiver {
        let receiver = self.0.invalidations.subscribe();
        let last_generation = receiver.borrow().generation;
        InvalidationReceiver {
            receiver,
            namespace: self.0.namespace.clone(),
            last_generation,
        }
    }

    #[must_use]
    pub(crate) fn status_receiver(&self) -> watch::Receiver<SyncStatus> {
        self.0.status.subscribe()
    }

    #[must_use]
    pub(crate) fn event_receiver(&self) -> watch::Receiver<Option<SyncEvent>> {
        self.0.events.subscribe()
    }

    #[must_use]
    pub(crate) fn bootstrap_receiver(&self) -> watch::Receiver<BootstrapState> {
        self.0.bootstrap.subscribe()
    }

    #[must_use]
    pub(crate) fn platform_receiver(&self) -> watch::Receiver<PlatformState> {
        self.0.platform.subscribe()
    }

    /// Rereads the durable status rather than trusting event delivery.
    ///
    /// # Errors
    ///
    /// Returns the client's stable storage or lifecycle error when durable status is unavailable.
    pub async fn refresh_status(&self) -> Result<SyncStatus, AequoraError> {
        let status = self.0.client.status().await.map(SyncStatus::from)?;
        self.0.status.send_replace(status);
        Ok(status)
    }

    /// Forwards a platform lifecycle/connectivity hint. OS background execution remains owned by
    /// the platform runtime, not by mounted components.
    pub fn notify_platform(&self, event: PlatformEvent) {
        let mut state = *self.0.platform.borrow();
        match event {
            PlatformEvent::Foreground | PlatformEvent::Resume => {
                state.lifecycle = LifecycleState::Foreground;
                let _accepted = self.0.client.request_sync(SyncReason::AppResumed);
            }
            PlatformEvent::Background => state.lifecycle = LifecycleState::Background,
            PlatformEvent::Suspend => state.lifecycle = LifecycleState::Suspended,
            PlatformEvent::Connectivity(connectivity) => {
                state.connectivity = connectivity;
                if connectivity == Connectivity::Online {
                    let _accepted = self.0.client.request_sync(SyncReason::ConnectivityChanged);
                } else if connectivity == Connectivity::Offline {
                    self.0.status.send_replace(SyncStatus::Offline);
                }
            }
        }
        self.0.platform.send_replace(state);
    }

    /// Runs the single provider-owned bridge from bounded client events into latest-value UI hints.
    pub(crate) async fn bridge_events(self) {
        let _ = self.refresh_status().await;
        let mut events = self.0.client.events();
        while let Some(event) = events.next().await {
            self.apply_event(event);
        }
    }

    fn apply_event(&self, event: SyncEvent) {
        match &event {
            SyncEvent::StatusChanged(status) => {
                self.0.status.send_replace(SyncStatus::from(*status));
            }
            SyncEvent::DataChanged(change) => {
                self.invalidate(Invalidation::Scope(change.scope_id));
            }
            SyncEvent::ConflictCreated(_) => self.invalidate(Invalidation::Conflicts),
            SyncEvent::BootstrapProgress(progress) => {
                self.0.bootstrap.send_replace(BootstrapState::Downloading {
                    completed: progress.completed,
                    total: progress.total,
                });
            }
            SyncEvent::SyncRequested(_) => {}
            SyncEvent::Closed => {
                self.0.status.send_replace(SyncStatus::Closed);
            }
            _ => self.invalidate(Invalidation::All),
        }
        self.0.events.send_replace(Some(event));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aequora_adapter_sdk::{
        AdapterError, ClientIdentity, ClientStore, ConflictId, ConflictResolution, ConflictSummary,
        Credential, CredentialProvider, DomainId, DurableClientStatus, ExchangeContext,
        ExchangeSummary, OperationSnapshot, SyncTransport,
    };
    use aequora_client::{BootstrapProgress, ClientSdkSyncStatus, DataChange};
    use aequora_operation::{EncodedOperation, LocalCommitStatus};
    use aequora_types::{DeviceId, OperationId, TenantId};
    use async_trait::async_trait;
    use std::time::Duration;

    struct Store;

    #[async_trait]
    impl ClientStore for Store {
        async fn commit_operation(
            &self,
            _operation: EncodedOperation,
        ) -> Result<LocalCommitStatus, AdapterError> {
            Ok(LocalCommitStatus::SavedLocally)
        }

        async fn operation(
            &self,
            _operation_id: OperationId,
        ) -> Result<Option<OperationSnapshot>, AdapterError> {
            Ok(None)
        }

        async fn status(&self) -> Result<DurableClientStatus, AdapterError> {
            Ok(DurableClientStatus::default())
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
            Ok(OperationId::new())
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

    async fn handle(namespace: &str) -> AequoraHandle {
        let domain = DomainId::new("test")
            .unwrap_or_else(|error| panic!("test domain must be valid: {error}"));
        let client = AequoraClient::builder()
            .store(Store)
            .transport(Transport)
            .credentials(Credentials)
            .identity(ClientIdentity {
                tenant_id: TenantId::new(),
                device_id: DeviceId::new(),
            })
            .domain(domain)
            .build()
            .await
            .unwrap_or_else(|error| panic!("test client must build: {error}"));
        AequoraHandle::new(client, QueryNamespace::new(namespace))
    }

    #[tokio::test]
    async fn invalidation_is_targeted_and_missed_notices_refresh_conservatively() {
        let handle = handle("tenant-a").await;
        let students = QueryKey::new("students");
        let invoices = QueryKey::new("invoices");
        let mut receiver = handle.invalidations();

        handle.invalidate(Invalidation::Key(invoices.clone()));
        let unrelated =
            tokio::time::timeout(Duration::from_millis(10), receiver.changed(&students)).await;
        assert!(unrelated.is_err());

        handle.invalidate(Invalidation::Key(invoices));
        handle.invalidate(Invalidation::Key(students.clone()));
        assert!(receiver.changed(&students).await);
    }

    #[tokio::test]
    async fn store_namespaces_cannot_wake_each_others_queries() {
        let first = handle("tenant-a").await;
        let second = handle("tenant-b").await;
        let key = QueryKey::new("dashboard");
        let mut receiver = first.invalidations();

        second.invalidate(Invalidation::All);
        assert!(
            tokio::time::timeout(Duration::from_millis(10), receiver.changed(&key))
                .await
                .is_err()
        );
        first.invalidate(Invalidation::All);
        assert!(receiver.changed(&key).await);
    }

    #[tokio::test]
    async fn event_storm_is_latest_value_bounded_and_scope_aware() {
        let handle = handle("tenant-a").await;
        let scope = SyncScopeId::new();
        let key = QueryKey::new("students").in_scope(scope);
        let mut receiver = handle.invalidations();

        for _ in 0..10_000 {
            handle.apply_event(SyncEvent::DataChanged(DataChange { scope_id: scope }));
        }
        assert!(receiver.changed(&key).await);
    }

    #[tokio::test]
    async fn status_and_bootstrap_are_coarse_latest_value_views() {
        let handle = handle("tenant-a").await;
        let mut status = handle.status_receiver();
        let mut bootstrap = handle.bootstrap_receiver();

        handle.apply_event(SyncEvent::StatusChanged(ClientSdkSyncStatus::Syncing));
        status
            .changed()
            .await
            .unwrap_or_else(|_| panic!("status sender must remain open"));
        assert_eq!(*status.borrow_and_update(), SyncStatus::Syncing);

        handle.apply_event(SyncEvent::BootstrapProgress(BootstrapProgress {
            completed: 7,
            total: Some(10),
        }));
        bootstrap
            .changed()
            .await
            .unwrap_or_else(|_| panic!("bootstrap sender must remain open"));
        assert_eq!(
            *bootstrap.borrow_and_update(),
            BootstrapState::Downloading {
                completed: 7,
                total: Some(10)
            }
        );
    }

    #[tokio::test]
    async fn offline_platform_hint_does_not_close_or_replace_the_client() {
        let handle = handle("tenant-a").await;
        let client = handle.client();
        handle.notify_platform(PlatformEvent::Connectivity(Connectivity::Offline));

        assert_eq!(*handle.status_receiver().borrow(), SyncStatus::Offline);
        assert_eq!(
            client.status().await.map(SyncStatus::from),
            Ok(SyncStatus::Idle)
        );
    }
}
