//! Transport-neutral, best-effort live acceleration for durable Aequora synchronization.
//!
//! Nothing in this crate advances a cursor or applies domain state. Live messages can only wake
//! the normal authenticated reconciliation path. Loss, duplication, reordering, broker failure,
//! and restart are therefore latency events rather than correctness failures.

use std::{
    collections::{BTreeMap, BTreeSet},
    sync::{Arc, Mutex, MutexGuard},
};

use aequora_coordination::FencingToken;
use aequora_types::{ActorId, DeviceId, Sequence, SyncScopeId, TenantId};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use thiserror::Error;
use tokio::sync::broadcast;
use uuid::Uuid;

/// First additive live control protocol. It is negotiated separately from durable exchange v1.
pub const LIVE_PROTOCOL_V1: u16 = 1;

/// Ephemeral identity of one accepted live connection.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct LiveConnectionId(Uuid);

impl LiveConnectionId {
    /// Allocates an approximately time-ordered connection identity.
    #[must_use]
    pub fn new() -> Self {
        Self(Uuid::now_v7())
    }
}

impl Default for LiveConnectionId {
    fn default() -> Self {
        Self::new()
    }
}

/// Authenticated identity attached by the host before subscription authorization.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LiveSession {
    pub tenant_id: TenantId,
    pub actor_id: ActorId,
    pub device_id: DeviceId,
}

/// Small semantic reason for an advisory wake-up.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[non_exhaustive]
pub enum SyncHintReason {
    NewAuthoritativeChange,
    ScopeChanged,
    Revocation,
    MaintenanceChanged,
    UpgradeNotice,
    /// Backpressure collapsed more-specific notifications into one catch-up request.
    SyncRequired,
}

impl SyncHintReason {
    const fn priority(self) -> u8 {
        match self {
            Self::Revocation => 5,
            Self::ScopeChanged => 4,
            Self::MaintenanceChanged => 3,
            Self::UpgradeNotice => 2,
            Self::NewAuthoritativeChange => 1,
            Self::SyncRequired => 6,
        }
    }
}

/// Payload-free, transport-neutral advisory notification.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct SyncHint {
    pub protocol: u16,
    pub tenant_id: TenantId,
    pub scope_id: SyncScopeId,
    pub latest_sequence: Option<Sequence>,
    pub reason: SyncHintReason,
}

impl SyncHint {
    /// Creates a version-one hint. The sequence is advisory and may be absent or stale.
    #[must_use]
    pub const fn v1(
        tenant_id: TenantId,
        scope_id: SyncScopeId,
        latest_sequence: Option<Sequence>,
        reason: SyncHintReason,
    ) -> Self {
        Self {
            protocol: LIVE_PROTOCOL_V1,
            tenant_id,
            scope_id,
            latest_sequence,
            reason,
        }
    }

    fn merge(self, newer: Self) -> Self {
        let latest_sequence = match (self.latest_sequence, newer.latest_sequence) {
            (Some(left), Some(right)) => Some(left.max(right)),
            (Some(sequence), None) | (None, Some(sequence)) => Some(sequence),
            (None, None) => None,
        };
        let reason = if newer.reason.priority() >= self.reason.priority() {
            newer.reason
        } else {
            self.reason
        };
        Self {
            latest_sequence,
            reason,
            ..newer
        }
    }
}

/// Coarse, tenant-isolated broker routing key. Entity identifiers are intentionally absent.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
pub struct HintTopic {
    pub tenant_id: TenantId,
    pub scope_id: SyncScopeId,
}

impl From<SyncHint> for HintTopic {
    fn from(hint: SyncHint) -> Self {
        Self {
            tenant_id: hint.tenant_id,
            scope_id: hint.scope_id,
        }
    }
}

/// One typed control-plane message. Domain payload replication is deliberately unrepresentable.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[non_exhaustive]
pub enum LiveMessage {
    Hello { protocol: u16 },
    Subscribe { scopes: Vec<SyncScopeId> },
    Unsubscribe { scopes: Vec<SyncScopeId> },
    SyncHint(SyncHint),
    Ping,
    Pong,
    ServerNotice { code: u16 },
}

/// Failure class for an optional live channel.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LiveFailureKind {
    OptionalUnavailable,
    AuthFailed,
    ProtocolMismatch,
    RateLimited,
    PermanentConfig,
    Disconnected,
}

/// Payload-free live feature failure. Durable sync health is independent.
#[derive(Clone, Debug, Error, Eq, PartialEq)]
#[error("live feature {kind:?}: {message}")]
pub struct LiveError {
    pub kind: LiveFailureKind,
    pub message: String,
}

impl LiveError {
    #[must_use]
    pub fn new(kind: LiveFailureKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
        }
    }
}

/// Transport-neutral stream produced by WebSocket, SSE, platform push, IPC, or future adapters.
#[async_trait]
pub trait LiveHintStream: Send {
    async fn next_hint(&mut self) -> Result<Option<SyncHint>, LiveError>;
}

/// Optional edge transport. Connecting must authenticate and authorize every requested scope.
#[async_trait]
pub trait LiveHintTransport: Send + Sync {
    async fn connect(
        &self,
        session: LiveSession,
        scopes: &[SyncScopeId],
    ) -> Result<Box<dyn LiveHintStream>, LiveError>;
}

/// Cross-node ephemeral fan-out. Implementations must never be used as a durable journal.
#[async_trait]
pub trait HintSubscription: Send {
    async fn next_hint(&mut self) -> Result<Option<SyncHint>, LiveError>;
}

/// Replaceable fan-out provider, for example in-memory, `PostgreSQL` NOTIFY, Redis, or NATS.
#[async_trait]
pub trait HintBroker: Send + Sync {
    async fn publish(&self, hint: SyncHint) -> Result<(), LiveError>;
    async fn subscribe(&self) -> Result<Box<dyn HintSubscription>, LiveError>;
}

/// Post-commit notifier boundary. Publication failure is reported but cannot undo the commit.
#[async_trait]
pub trait PostCommitHintPublisher: Send + Sync {
    async fn publish_after_commit(&self, hint: SyncHint) -> Result<(), LiveError>;
}

/// Best-effort post-commit result. `Degraded` changes latency only and is never a transaction error.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PublicationOutcome {
    Published,
    Degraded,
}

/// Publishes after commit without exposing broker failure as a rollback-capable error.
pub async fn publish_best_effort(
    publisher: &dyn PostCommitHintPublisher,
    hint: SyncHint,
) -> PublicationOutcome {
    match publisher.publish_after_commit(hint).await {
        Ok(()) => PublicationOutcome::Published,
        Err(_) => PublicationOutcome::Degraded,
    }
}

#[async_trait]
impl<T> PostCommitHintPublisher for T
where
    T: HintBroker,
{
    async fn publish_after_commit(&self, hint: SyncHint) -> Result<(), LiveError> {
        self.publish(hint).await
    }
}

/// Bounded in-process fan-out for single-node deployments and deterministic tests.
#[derive(Debug)]
pub struct InMemoryHintBroker {
    sender: broadcast::Sender<SyncHint>,
}

impl InMemoryHintBroker {
    /// Creates a bounded lossy channel. Lagging receivers perform normal catch-up later.
    #[must_use]
    pub fn new(capacity: usize) -> Self {
        let (sender, _) = broadcast::channel(capacity.max(1));
        Self { sender }
    }
}

struct InMemorySubscription {
    receiver: broadcast::Receiver<SyncHint>,
}

#[async_trait]
impl HintSubscription for InMemorySubscription {
    async fn next_hint(&mut self) -> Result<Option<SyncHint>, LiveError> {
        loop {
            match self.receiver.recv().await {
                Ok(hint) => return Ok(Some(hint)),
                Err(broadcast::error::RecvError::Lagged(_)) => {}
                Err(broadcast::error::RecvError::Closed) => return Ok(None),
            }
        }
    }
}

#[async_trait]
impl HintBroker for InMemoryHintBroker {
    async fn publish(&self, hint: SyncHint) -> Result<(), LiveError> {
        // No listeners is a healthy best-effort outcome.
        let _receiver_count = self.sender.send(hint);
        Ok(())
    }

    async fn subscribe(&self) -> Result<Box<dyn HintSubscription>, LiveError> {
        Ok(Box::new(InMemorySubscription {
            receiver: self.sender.subscribe(),
        }))
    }
}

/// Scope authorization performed on connect and every dynamic subscription change.
#[async_trait]
pub trait LiveScopeAuthorizer: Send + Sync {
    async fn authorize(&self, session: LiveSession, scope: SyncScopeId) -> bool;
}

/// Result of inserting into a latest-only connection queue.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum QueueOutcome {
    Inserted,
    Coalesced,
    OverflowDropped,
}

/// Latest-only per-scope queue with a hard cardinality bound.
#[derive(Clone, Debug)]
pub struct LatestHintQueue {
    limit: usize,
    pending: BTreeMap<HintTopic, SyncHint>,
    dropped: u64,
}

impl LatestHintQueue {
    #[must_use]
    pub fn new(limit: usize) -> Self {
        Self {
            limit: limit.max(1),
            pending: BTreeMap::new(),
            dropped: 0,
        }
    }

    pub fn push(&mut self, hint: SyncHint) -> QueueOutcome {
        let topic = hint.into();
        if let Some(current) = self.pending.get_mut(&topic) {
            *current = current.merge(hint);
            return QueueOutcome::Coalesced;
        }
        if self.pending.len() >= self.limit {
            self.dropped = self.dropped.saturating_add(1);
            return QueueOutcome::OverflowDropped;
        }
        self.pending.insert(topic, hint);
        QueueOutcome::Inserted
    }

    /// Removes the highest-priority available hint in deterministic topic order.
    pub fn pop(&mut self) -> Option<SyncHint> {
        let topic = self.pending.keys().next().copied()?;
        self.pending.remove(&topic)
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.pending.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.pending.is_empty()
    }

    #[must_use]
    pub const fn dropped(&self) -> u64 {
        self.dropped
    }
}

/// One node-local authorized connection and its bounded output queue.
#[derive(Clone, Debug)]
pub struct LiveConnection {
    pub id: LiveConnectionId,
    pub session: LiveSession,
    scopes: BTreeSet<SyncScopeId>,
    queue: LatestHintQueue,
}

impl LiveConnection {
    #[must_use]
    pub fn is_subscribed(&self, topic: HintTopic) -> bool {
        self.session.tenant_id == topic.tenant_id && self.scopes.contains(&topic.scope_id)
    }

    pub fn next_hint(&mut self) -> Option<SyncHint> {
        self.queue.pop()
    }

    #[must_use]
    pub fn queued(&self) -> usize {
        self.queue.len()
    }
}

/// Admission limits applied independently of durable synchronization availability.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LiveLimits {
    pub max_connections: usize,
    pub max_scopes_per_connection: usize,
    pub max_queued_scopes_per_connection: usize,
}

impl Default for LiveLimits {
    fn default() -> Self {
        Self {
            max_connections: 10_000,
            max_scopes_per_connection: 64,
            max_queued_scopes_per_connection: 64,
        }
    }
}

/// Node-local connection routing. Cross-node delivery is supplied by [`HintBroker`].
pub struct LiveRouter<A> {
    authorizer: Arc<A>,
    limits: LiveLimits,
    connections: BTreeMap<LiveConnectionId, LiveConnection>,
}

impl<A> LiveRouter<A>
where
    A: LiveScopeAuthorizer,
{
    #[must_use]
    pub fn new(authorizer: Arc<A>, limits: LiveLimits) -> Self {
        Self {
            authorizer,
            limits,
            connections: BTreeMap::new(),
        }
    }

    /// Authenticates requested scope membership before making a connection routable.
    ///
    /// # Errors
    ///
    /// Returns an authorization or quota failure before installing the connection.
    pub async fn connect(
        &mut self,
        session: LiveSession,
        scopes: &[SyncScopeId],
    ) -> Result<LiveConnectionId, LiveError> {
        if self.connections.len() >= self.limits.max_connections {
            return Err(LiveError::new(
                LiveFailureKind::RateLimited,
                "connection quota",
            ));
        }
        let unique: BTreeSet<_> = scopes.iter().copied().collect();
        if unique.len() > self.limits.max_scopes_per_connection {
            return Err(LiveError::new(LiveFailureKind::RateLimited, "scope quota"));
        }
        for scope in &unique {
            if !self.authorizer.authorize(session, *scope).await {
                return Err(LiveError::new(LiveFailureKind::AuthFailed, "scope denied"));
            }
        }
        let id = LiveConnectionId::new();
        self.connections.insert(
            id,
            LiveConnection {
                id,
                session,
                scopes: unique,
                queue: LatestHintQueue::new(self.limits.max_queued_scopes_per_connection),
            },
        );
        Ok(id)
    }

    /// Routes only to currently authorized tenant/scope subscribers.
    pub fn route(&mut self, hint: SyncHint) -> FanoutOutcome {
        let topic = hint.into();
        let mut outcome = FanoutOutcome::default();
        for connection in self.connections.values_mut() {
            if connection.is_subscribed(topic) {
                outcome.matched = outcome.matched.saturating_add(1);
                match connection.queue.push(hint) {
                    QueueOutcome::Inserted => outcome.queued = outcome.queued.saturating_add(1),
                    QueueOutcome::Coalesced => {
                        outcome.coalesced = outcome.coalesced.saturating_add(1);
                    }
                    QueueOutcome::OverflowDropped => {
                        outcome.dropped = outcome.dropped.saturating_add(1);
                    }
                }
            }
        }
        outcome
    }

    /// Adds dynamically requested scopes only after reauthorization.
    ///
    /// # Errors
    ///
    /// Returns an authorization, quota, or disconnected-connection failure.
    pub async fn subscribe_scopes(
        &mut self,
        id: LiveConnectionId,
        scopes: &[SyncScopeId],
    ) -> Result<(), LiveError> {
        let Some(connection) = self.connections.get(&id) else {
            return Err(LiveError::new(
                LiveFailureKind::Disconnected,
                "unknown connection",
            ));
        };
        let session = connection.session;
        let mut combined = connection.scopes.clone();
        combined.extend(scopes.iter().copied());
        if combined.len() > self.limits.max_scopes_per_connection {
            return Err(LiveError::new(LiveFailureKind::RateLimited, "scope quota"));
        }
        for scope in scopes {
            if !self.authorizer.authorize(session, *scope).await {
                return Err(LiveError::new(LiveFailureKind::AuthFailed, "scope denied"));
            }
        }
        if let Some(connection) = self.connections.get_mut(&id) {
            connection.scopes = combined;
        }
        Ok(())
    }

    /// Removes subscriptions immediately; reconnect is not required.
    ///
    /// # Errors
    ///
    /// Returns a disconnected-connection failure when the identity is unknown.
    pub fn unsubscribe_scopes(
        &mut self,
        id: LiveConnectionId,
        scopes: &[SyncScopeId],
    ) -> Result<(), LiveError> {
        let Some(connection) = self.connections.get_mut(&id) else {
            return Err(LiveError::new(
                LiveFailureKind::Disconnected,
                "unknown connection",
            ));
        };
        for scope in scopes {
            connection.scopes.remove(scope);
        }
        Ok(())
    }

    /// Removes revoked routing before any later hint can be delivered.
    pub fn revoke_scope(&mut self, tenant_id: TenantId, scope_id: SyncScopeId) -> usize {
        let mut removed = 0_usize;
        for connection in self.connections.values_mut() {
            if connection.session.tenant_id == tenant_id && connection.scopes.remove(&scope_id) {
                removed = removed.saturating_add(1);
            }
        }
        removed
    }

    pub fn disconnect(&mut self, id: LiveConnectionId) -> Option<LiveConnection> {
        self.connections.remove(&id)
    }

    pub fn connection_mut(&mut self, id: LiveConnectionId) -> Option<&mut LiveConnection> {
        self.connections.get_mut(&id)
    }

    #[must_use]
    pub fn connection_count(&self) -> usize {
        self.connections.len()
    }
}

/// Aggregate node-local fan-out decision, suitable for payload-free metrics.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct FanoutOutcome {
    pub matched: u64,
    pub queued: u64,
    pub coalesced: u64,
    pub dropped: u64,
}

/// Exact leadership epoch that may own one shared-store live connection.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LiveLeadership {
    pub fencing_token: FencingToken,
}

impl LiveLeadership {
    /// Stale leaders must close or ignore their live channel.
    #[must_use]
    pub const fn is_current(self, current: FencingToken) -> bool {
        self.fencing_token.0 == current.0
    }
}

/// Client-side wake result. No cursor or replica state appears in this API.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HintWakeOutcome {
    Wake { generation: u64 },
    Coalesced { generation: u64 },
    Unauthorized,
    ProtocolMismatch,
}

/// Bounded coalescing state for races between live arrival and a normal exchange.
#[derive(Debug)]
pub struct HintWakeTracker {
    tenant_id: TenantId,
    scopes: BTreeSet<SyncScopeId>,
    state: Mutex<WakeState>,
}

#[derive(Debug, Default)]
struct WakeState {
    generation: u64,
    last_hinted: BTreeMap<SyncScopeId, Option<Sequence>>,
}

impl HintWakeTracker {
    #[must_use]
    pub fn new(tenant_id: TenantId, scopes: impl IntoIterator<Item = SyncScopeId>) -> Self {
        Self {
            tenant_id,
            scopes: scopes.into_iter().collect(),
            state: Mutex::new(WakeState::default()),
        }
    }

    /// Records an advisory wake. This deliberately accepts no mutable cursor or local store.
    pub fn observe(&self, hint: SyncHint) -> HintWakeOutcome {
        if hint.protocol != LIVE_PROTOCOL_V1 {
            return HintWakeOutcome::ProtocolMismatch;
        }
        if hint.tenant_id != self.tenant_id || !self.scopes.contains(&hint.scope_id) {
            return HintWakeOutcome::Unauthorized;
        }
        let mut state = lock_unpoisoned(&self.state);
        let prior = state.last_hinted.get(&hint.scope_id).copied();
        let redundant = match (prior, hint.latest_sequence) {
            (Some(Some(old)), Some(new)) => new <= old,
            (Some(None), None) => true,
            _ => false,
        } && !matches!(
            hint.reason,
            SyncHintReason::Revocation | SyncHintReason::ScopeChanged
        );
        if redundant {
            return HintWakeOutcome::Coalesced {
                generation: state.generation,
            };
        }
        let merged = prior
            .flatten()
            .zip(hint.latest_sequence)
            .map_or(hint.latest_sequence, |(old, new)| Some(old.max(new)));
        state.last_hinted.insert(hint.scope_id, merged);
        state.generation = state.generation.saturating_add(1);
        HintWakeOutcome::Wake {
            generation: state.generation,
        }
    }

    /// Captures the generation immediately before a normal cursor exchange.
    #[must_use]
    pub fn begin_exchange(&self) -> u64 {
        lock_unpoisoned(&self.state).generation
    }

    /// True when another hint arrived during the exchange and a further pull is required.
    #[must_use]
    pub fn needs_follow_up(&self, started_at: u64) -> bool {
        lock_unpoisoned(&self.state).generation != started_at
    }
}

fn lock_unpoisoned<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// Bounded exponential reconnect schedule. Hosts supply jitter to avoid an RNG dependency.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ReconnectPolicy {
    pub initial_delay_ms: u64,
    pub maximum_delay_ms: u64,
}

impl ReconnectPolicy {
    #[must_use]
    pub const fn delay_ms(self, attempt: u32, jitter_ms: u64) -> u64 {
        let shift = if attempt > 31 { 31 } else { attempt };
        let multiplier = 1_u64 << shift;
        let delay = self
            .initial_delay_ms
            .saturating_mul(multiplier)
            .saturating_add(jitter_ms);
        if delay > self.maximum_delay_ms {
            self.maximum_delay_ms
        } else {
            delay
        }
    }
}

/// Advisory presence state. It is neither durable business state nor a lock.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum PresenceState {
    Online,
    Active,
    Idle,
    Away,
}

/// TTL-bound presence record using a host-supplied monotonic millisecond domain.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PresenceRecord {
    pub tenant_id: TenantId,
    pub principal_id: ActorId,
    pub device_id: DeviceId,
    pub scope_id: Option<SyncScopeId>,
    pub state: PresenceState,
    pub expires_at_ms: u64,
}

/// Bounded ephemeral presence directory. Restart intentionally clears all records.
#[derive(Debug)]
pub struct PresenceDirectory {
    limit: usize,
    records: BTreeMap<(TenantId, ActorId, DeviceId, Option<SyncScopeId>), PresenceRecord>,
}

impl PresenceDirectory {
    #[must_use]
    pub fn new(limit: usize) -> Self {
        Self {
            limit: limit.max(1),
            records: BTreeMap::new(),
        }
    }

    /// Refreshes one session after removing expired records.
    ///
    /// # Errors
    ///
    /// Returns a quota failure or rejects an already-expired TTL.
    pub fn refresh(&mut self, record: PresenceRecord, now_ms: u64) -> Result<(), LiveError> {
        self.expire(now_ms);
        let key = (
            record.tenant_id,
            record.principal_id,
            record.device_id,
            record.scope_id,
        );
        if !self.records.contains_key(&key) && self.records.len() >= self.limit {
            return Err(LiveError::new(
                LiveFailureKind::RateLimited,
                "presence quota",
            ));
        }
        if record.expires_at_ms <= now_ms {
            return Err(LiveError::new(
                LiveFailureKind::PermanentConfig,
                "presence TTL must be in the future",
            ));
        }
        self.records.insert(key, record);
        Ok(())
    }

    /// Removes expired state; absence is the offline representation.
    pub fn expire(&mut self, now_ms: u64) -> usize {
        let before = self.records.len();
        self.records
            .retain(|_, record| record.expires_at_ms > now_ms);
        before.saturating_sub(self.records.len())
    }

    /// Returns privacy-filtered records. Authorization policy stays application-owned.
    pub fn visible_to(
        &self,
        viewer: LiveSession,
        mut authorize: impl FnMut(LiveSession, &PresenceRecord) -> bool,
    ) -> Vec<PresenceRecord> {
        self.records
            .values()
            .filter(|record| record.tenant_id == viewer.tenant_id && authorize(viewer, record))
            .copied()
            .collect()
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.records.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.records.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct AllowOne(SyncScopeId);

    #[async_trait]
    impl LiveScopeAuthorizer for AllowOne {
        async fn authorize(&self, _session: LiveSession, scope: SyncScopeId) -> bool {
            scope == self.0
        }
    }

    fn session(tenant_id: TenantId) -> LiveSession {
        LiveSession {
            tenant_id,
            actor_id: ActorId::new(),
            device_id: DeviceId::new(),
        }
    }

    #[test]
    fn wire_envelope_contains_no_domain_payload_variant() {
        let hint = SyncHint::v1(
            TenantId::new(),
            SyncScopeId::new(),
            Some(Sequence(9)),
            SyncHintReason::NewAuthoritativeChange,
        );
        let encoded = postcard::to_stdvec(&LiveMessage::SyncHint(hint));
        assert!(encoded.is_ok());
        assert!(encoded.unwrap_or_default().len() < 128);
    }

    #[test]
    fn latest_only_queue_is_bounded_and_monotonic() {
        let tenant = TenantId::new();
        let scope = SyncScopeId::new();
        let other = SyncScopeId::new();
        let mut queue = LatestHintQueue::new(1);
        assert_eq!(
            queue.push(SyncHint::v1(
                tenant,
                scope,
                Some(Sequence(10)),
                SyncHintReason::NewAuthoritativeChange,
            )),
            QueueOutcome::Inserted
        );
        assert_eq!(
            queue.push(SyncHint::v1(
                tenant,
                scope,
                Some(Sequence(12)),
                SyncHintReason::ScopeChanged,
            )),
            QueueOutcome::Coalesced
        );
        assert_eq!(
            queue.push(SyncHint::v1(
                tenant,
                other,
                None,
                SyncHintReason::SyncRequired,
            )),
            QueueOutcome::OverflowDropped
        );
        let hint = queue.pop();
        assert_eq!(
            hint.map(|item| item.latest_sequence),
            Some(Some(Sequence(12)))
        );
        assert_eq!(queue.dropped(), 1);
    }

    #[test]
    fn stopped_consumer_keeps_one_entry_for_one_hundred_thousand_events() {
        let tenant = TenantId::new();
        let scope = SyncScopeId::new();
        let mut queue = LatestHintQueue::new(8);
        for sequence in 1..=100_000 {
            let _outcome = queue.push(SyncHint::v1(
                tenant,
                scope,
                Some(Sequence(sequence)),
                SyncHintReason::NewAuthoritativeChange,
            ));
        }
        assert_eq!(queue.len(), 1);
        assert_eq!(
            queue.pop().and_then(|hint| hint.latest_sequence),
            Some(Sequence(100_000))
        );
    }

    #[tokio::test]
    async fn router_enforces_tenant_scope_and_revocation() {
        let tenant = TenantId::new();
        let foreign = TenantId::new();
        let scope = SyncScopeId::new();
        let mut router = LiveRouter::new(Arc::new(AllowOne(scope)), LiveLimits::default());
        let id = router.connect(session(tenant), &[scope]).await;
        assert!(id.is_ok());
        let id = id.unwrap_or_default();
        assert_eq!(
            router.route(SyncHint::v1(
                foreign,
                scope,
                Some(Sequence(1)),
                SyncHintReason::NewAuthoritativeChange,
            )),
            FanoutOutcome::default()
        );
        assert_eq!(router.revoke_scope(tenant, scope), 1);
        assert_eq!(
            router.route(SyncHint::v1(
                tenant,
                scope,
                Some(Sequence(2)),
                SyncHintReason::Revocation,
            )),
            FanoutOutcome::default()
        );
        assert_eq!(router.connection_mut(id).map(|item| item.queued()), Some(0));
    }

    #[test]
    fn duplicates_reordering_and_exchange_race_only_schedule_work() {
        let tenant = TenantId::new();
        let scope = SyncScopeId::new();
        let tracker = HintWakeTracker::new(tenant, [scope]);
        let first = SyncHint::v1(
            tenant,
            scope,
            Some(Sequence(9)),
            SyncHintReason::NewAuthoritativeChange,
        );
        assert_eq!(
            tracker.observe(first),
            HintWakeOutcome::Wake { generation: 1 }
        );
        assert_eq!(
            tracker.observe(first),
            HintWakeOutcome::Coalesced { generation: 1 }
        );
        let started = tracker.begin_exchange();
        assert_eq!(
            tracker.observe(SyncHint {
                latest_sequence: Some(Sequence(8)),
                ..first
            }),
            HintWakeOutcome::Coalesced { generation: 1 }
        );
        assert!(!tracker.needs_follow_up(started));
        assert_eq!(
            tracker.observe(SyncHint {
                latest_sequence: Some(Sequence(10)),
                ..first
            }),
            HintWakeOutcome::Wake { generation: 2 }
        );
        assert!(tracker.needs_follow_up(started));
    }

    #[test]
    fn presence_expires_and_is_tenant_private() {
        let tenant = TenantId::new();
        let record = PresenceRecord {
            tenant_id: tenant,
            principal_id: ActorId::new(),
            device_id: DeviceId::new(),
            scope_id: None,
            state: PresenceState::Online,
            expires_at_ms: 10,
        };
        let mut directory = PresenceDirectory::new(1);
        assert!(directory.refresh(record, 1).is_ok());
        assert_eq!(
            directory
                .visible_to(session(TenantId::new()), |_, _| true)
                .len(),
            0
        );
        assert_eq!(directory.expire(10), 1);
        assert!(directory.is_empty());
    }

    #[tokio::test]
    async fn in_memory_broker_is_optional_and_loss_tolerant() {
        let broker = InMemoryHintBroker::new(1);
        let hint = SyncHint::v1(
            TenantId::new(),
            SyncScopeId::new(),
            None,
            SyncHintReason::SyncRequired,
        );
        assert!(broker.publish(hint).await.is_ok());
        let subscription = broker.subscribe().await;
        assert!(subscription.is_ok());
        assert!(broker.publish(hint).await.is_ok());
        let mut subscription = subscription.unwrap_or_else(|_| unreachable!());
        assert_eq!(subscription.next_hint().await, Ok(Some(hint)));
    }
}
