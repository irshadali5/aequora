//! Database-, runtime-, broker-, and projection-neutral multi-consumer change feeds.
//!
//! The authoritative journal remains the only source of truth. This crate gives each downstream
//! consumer an independent epoch-bound cursor, bounded filtering and batching, fenced work claims,
//! explicit ordering/retention/failure policies, and fail-closed checkpoint/reset/rebuild rules.

#![forbid(unsafe_code)]
#![allow(clippy::missing_errors_doc)]

use aequora_metadata::{JournalRecord, Timestamp};
use aequora_types::{
    AuthorityEpoch, AuthorityId, CorrelationId, EntityRef, EventId, OperationId, Sequence,
    SnapshotId, TenantId,
};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use thiserror::Error;
use uuid::Uuid;

pub const FEED_SCHEMA_VERSION: u16 = 1;
pub const CONSUMER_TABLE: &str = "aequora_consumer";
pub const CONSUMER_CURSOR_TABLE: &str = "aequora_consumer_cursor";
pub const CONSUMER_PARTITION_CURSOR_TABLE: &str = "aequora_consumer_partition_cursor";
pub const CONSUMER_LEASE_TABLE: &str = "aequora_consumer_lease";
pub const CONSUMER_QUARANTINE_TABLE: &str = "aequora_consumer_quarantine";
pub const CONSUMER_CHECKPOINT_TABLE: &str = "aequora_consumer_checkpoint";
pub const FEED_ARCHIVE_SEGMENT_TABLE: &str = "aequora_feed_archive_segment";
pub const MAX_CONSUMER_NAME_BYTES: usize = 128;
pub const MAX_FILTER_VALUES: usize = 4_096;
pub const MAX_ERROR_CODE_BYTES: usize = 128;

macro_rules! uuid_id {
    ($name:ident, $doc:literal) => {
        #[doc = $doc]
        #[derive(
            Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize,
        )]
        #[serde(transparent)]
        pub struct $name(Uuid);
        impl $name {
            #[must_use]
            pub fn new() -> Self {
                Self(Uuid::now_v7())
            }
            #[must_use]
            pub const fn from_uuid(value: Uuid) -> Self {
                Self(value)
            }
            #[must_use]
            pub const fn as_uuid(self) -> Uuid {
                self.0
            }
        }
        impl Default for $name {
            fn default() -> Self {
                Self::new()
            }
        }
    };
}

uuid_id!(
    ConsumerId,
    "Stable identity of one logical downstream consumer."
);
uuid_id!(
    ConsumerGroupId,
    "Stable identity of cooperating consumer workers."
);
uuid_id!(
    ConsumerWorkerId,
    "Operational identity of one consumer worker."
);
uuid_id!(
    ConsumerPartitionId,
    "Stable identity of one consumer partition."
);
uuid_id!(
    ConsumerResetPlanId,
    "Audited identity of one reviewed reset plan."
);
uuid_id!(
    ConsumerCheckpointId,
    "Stable identity of one durable checkpoint."
);
uuid_id!(
    FeedArchiveSegmentId,
    "Stable identity of one verified archive segment."
);

macro_rules! nonzero_u32 {
    ($name:ident) => {
        #[derive(
            Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize,
        )]
        #[serde(transparent)]
        pub struct $name(u32);
        impl $name {
            pub const fn new(value: u32) -> Result<Self, FeedError> {
                if value == 0 {
                    Err(FeedError::ZeroIdentity)
                } else {
                    Ok(Self(value))
                }
            }
            #[must_use]
            pub const fn get(self) -> u32 {
                self.0
            }
        }
    };
}

nonzero_u32!(ConsumerKind);
nonzero_u32!(ConsumerProjectionVersion);
nonzero_u32!(ConsumerFilterVersion);
nonzero_u32!(ConsumerRuleVersion);
nonzero_u32!(EventKind);
nonzero_u32!(EventSchemaVersion);

#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct ConsumerFencingToken(u64);
impl ConsumerFencingToken {
    pub const INITIAL: Self = Self(1);
    pub const fn new(value: u64) -> Result<Self, FeedError> {
        if value == 0 {
            Err(FeedError::ZeroIdentity)
        } else {
            Ok(Self(value))
        }
    }
    #[must_use]
    pub const fn get(self) -> u64 {
        self.0
    }
    #[must_use]
    pub const fn next(self) -> Option<Self> {
        match self.0.checked_add(1) {
            Some(value) => Some(Self(value)),
            None => None,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ChangeFeedEvent {
    pub event_id: EventId,
    pub authority_epoch: AuthorityEpoch,
    pub sequence: Sequence,
    pub tenant_id: TenantId,
    pub entity: EntityRef,
    pub entity_version: aequora_types::EntityVersion,
    pub event_kind: EventKind,
    pub schema_version: EventSchemaVersion,
    pub payload: Vec<u8>,
    pub payload_digest: [u8; 32],
    pub operation_id: OperationId,
    /// Registered operation kind when the source enriches the journal record from its ledger.
    pub operation_kind: Option<u32>,
    pub correlation_id: CorrelationId,
    pub occurred_at: Timestamp,
}

impl TryFrom<&JournalRecord> for ChangeFeedEvent {
    type Error = FeedError;
    fn try_from(record: &JournalRecord) -> Result<Self, Self::Error> {
        record
            .validate()
            .map_err(|_| FeedError::CorruptJournalEvent)?;
        let event_kind = match record.event_kind {
            aequora_protocol::ChangeKind::Upsert => EventKind::new(1)?,
            aequora_protocol::ChangeKind::Tombstone => EventKind::new(2)?,
        };
        let schema = u32::from(record.event_schema_version.0);
        Ok(Self {
            event_id: record.event_id,
            authority_epoch: record.authority_epoch,
            sequence: record.sequence,
            tenant_id: record.tenant_id,
            entity: record.entity,
            entity_version: record.entity_version,
            event_kind,
            schema_version: EventSchemaVersion::new(schema)?,
            payload: record.payload_bytes.clone(),
            payload_digest: record.payload_digest,
            operation_id: record.operation_id,
            operation_kind: None,
            correlation_id: record.correlation_id,
            occurred_at: record.occurred_at,
        })
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ConsumerCursor {
    pub consumer_id: ConsumerId,
    pub authority_id: AuthorityId,
    pub authority_epoch: AuthorityEpoch,
    pub sequence: Sequence,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ConsumerPartitionCursor {
    pub consumer_id: ConsumerId,
    pub partition_id: ConsumerPartitionId,
    pub authority_id: AuthorityId,
    pub authority_epoch: AuthorityEpoch,
    pub sequence: Sequence,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum ConsumerStatus {
    Registered,
    Bootstrapping,
    Active,
    Paused,
    Draining,
    Lagging,
    Failed,
    Rebuilding,
    Disabled,
}

impl ConsumerStatus {
    pub fn transition(self, next: Self) -> Result<Self, FeedError> {
        let valid = matches!(
            (self, next),
            (Self::Registered, Self::Bootstrapping | Self::Disabled)
                | (
                    Self::Bootstrapping | Self::Rebuilding,
                    Self::Active | Self::Failed | Self::Disabled
                )
                | (
                    Self::Active,
                    Self::Paused | Self::Draining | Self::Lagging | Self::Failed
                )
                | (
                    Self::Paused,
                    Self::Active | Self::Rebuilding | Self::Disabled
                )
                | (Self::Draining, Self::Paused | Self::Disabled | Self::Failed)
                | (
                    Self::Lagging,
                    Self::Active | Self::Paused | Self::Failed | Self::Rebuilding
                )
                | (
                    Self::Failed,
                    Self::Paused | Self::Rebuilding | Self::Disabled
                )
        );
        if valid || self == next {
            Ok(next)
        } else {
            Err(FeedError::InvalidStatusTransition)
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum ConsumerOrderingPolicy {
    Global,
    PerTenant,
    PerEntity,
    Unordered,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum ConsumerRetentionPolicy {
    PinJournal,
    RebuildIfBehind,
    BestEffort,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum ConsumerFailurePolicy {
    Stop,
    Retry,
    Quarantine,
    SkipWithAudit,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum ConsumerReadPolicy {
    Authority,
    ReplicaAllowed,
    ArchivePreferred,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum ConsumerEpochRecoveryPolicy {
    ContinueIfMapped,
    Rebuild,
    ReplayFromRecoveryBoundary,
    Manual,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum Rebuildability {
    CanRebuild,
    CannotRebuild,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum ConsumerBootstrapMode {
    FromBeginning,
    SnapshotAndTail,
    CurrentOnly,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
pub enum FeedVisibility {
    InternalOnly,
    TenantIntegration,
    PublicIntegration,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum DeliveryMode {
    OrderedPull,
    InternalDirect,
    BrokerPublication,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum TenantSelector {
    Explicit(BTreeSet<TenantId>),
    AllAuthorized,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ConsumerFilter {
    pub tenants: TenantSelector,
    pub entity_types: BTreeSet<u16>,
    pub event_kinds: BTreeSet<EventKind>,
    pub operation_kinds: BTreeSet<u32>,
}

impl ConsumerFilter {
    pub fn validate(&self, authorization: &ConsumerAuthorization) -> Result<(), FeedError> {
        if self.entity_types.len() > MAX_FILTER_VALUES
            || self.event_kinds.len() > MAX_FILTER_VALUES
            || self.operation_kinds.len() > MAX_FILTER_VALUES
        {
            return Err(FeedError::LimitExceeded("filter values"));
        }
        if let TenantSelector::Explicit(tenants) = &self.tenants {
            if tenants.len() > MAX_FILTER_VALUES || !tenants.is_subset(&authorization.tenants) {
                return Err(FeedError::TenantDenied);
            }
        }
        Ok(())
    }

    #[must_use]
    pub fn matches(&self, event: &ChangeFeedEvent, authorization: &ConsumerAuthorization) -> bool {
        authorization.tenants.contains(&event.tenant_id)
            && match &self.tenants {
                TenantSelector::AllAuthorized => true,
                TenantSelector::Explicit(tenants) => tenants.contains(&event.tenant_id),
            }
            && (self.entity_types.is_empty()
                || self.entity_types.contains(&event.entity.entity_type.get()))
            && (self.event_kinds.is_empty() || self.event_kinds.contains(&event.event_kind))
            && (self.operation_kinds.is_empty()
                || event
                    .operation_kind
                    .is_some_and(|kind| self.operation_kinds.contains(&kind)))
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ConsumerAuthorization {
    pub service_identity: String,
    pub tenants: BTreeSet<TenantId>,
    pub allowed_visibility: BTreeSet<FeedVisibility>,
    pub can_historical_replay: bool,
    pub can_skip_history: bool,
}

impl ConsumerAuthorization {
    pub fn validate(&self) -> Result<(), FeedError> {
        if self.service_identity.is_empty()
            || self.service_identity.len() > MAX_CONSUMER_NAME_BYTES
            || self.tenants.is_empty()
            || self.allowed_visibility.is_empty()
        {
            return Err(FeedError::InvalidRegistration("authorization"));
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct BatchLimits {
    pub max_events: usize,
    pub max_bytes: usize,
    pub max_processing_ms: u64,
    pub max_concurrency: usize,
}

impl BatchLimits {
    pub fn validate(self) -> Result<(), FeedError> {
        if self.max_events == 0
            || self.max_bytes == 0
            || self.max_processing_ms == 0
            || self.max_concurrency == 0
        {
            return Err(FeedError::InvalidRegistration("batch limits"));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ConsumerRegistration {
    pub consumer_id: ConsumerId,
    pub group_id: ConsumerGroupId,
    pub kind: ConsumerKind,
    pub name: String,
    pub status: ConsumerStatus,
    pub filter: ConsumerFilter,
    pub filter_version: ConsumerFilterVersion,
    pub projection_version: ConsumerProjectionVersion,
    pub rule_version: ConsumerRuleVersion,
    pub ordering: ConsumerOrderingPolicy,
    pub retention: ConsumerRetentionPolicy,
    pub failure: ConsumerFailurePolicy,
    pub read_policy: ConsumerReadPolicy,
    pub epoch_recovery: ConsumerEpochRecoveryPolicy,
    pub rebuildability: Rebuildability,
    pub bootstrap: ConsumerBootstrapMode,
    pub delivery_mode: DeliveryMode,
    pub visibility: FeedVisibility,
    pub authorization: ConsumerAuthorization,
    pub batch: BatchLimits,
    pub max_retention_pin_age_ms: Option<u64>,
    pub created_at: Timestamp,
}

impl ConsumerRegistration {
    pub fn validate(&self) -> Result<(), FeedError> {
        if self.name.is_empty() || self.name.len() > MAX_CONSUMER_NAME_BYTES {
            return Err(FeedError::InvalidRegistration("name"));
        }
        self.authorization.validate()?;
        self.filter.validate(&self.authorization)?;
        self.batch.validate()?;
        if !self
            .authorization
            .allowed_visibility
            .contains(&self.visibility)
        {
            return Err(FeedError::VisibilityDenied);
        }
        if self.retention == ConsumerRetentionPolicy::PinJournal
            && (self.rebuildability != Rebuildability::CannotRebuild
                || self.max_retention_pin_age_ms.is_none())
        {
            return Err(FeedError::InvalidRegistration("retention pin review"));
        }
        if self.bootstrap == ConsumerBootstrapMode::CurrentOnly
            && self.retention != ConsumerRetentionPolicy::BestEffort
        {
            return Err(FeedError::InvalidRegistration("current-only delivery"));
        }
        if matches!(self.ordering, ConsumerOrderingPolicy::Global)
            && self.batch.max_concurrency != 1
        {
            return Err(FeedError::OrderingViolation);
        }
        if self.failure == ConsumerFailurePolicy::SkipWithAudit
            && self.retention != ConsumerRetentionPolicy::BestEffort
        {
            return Err(FeedError::UnsafeSkip);
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ConsumerRegistry {
    consumers: BTreeMap<ConsumerId, ConsumerRegistration>,
    retired: BTreeSet<ConsumerId>,
}

impl ConsumerRegistry {
    #[must_use]
    pub fn new() -> Self {
        Self {
            consumers: BTreeMap::new(),
            retired: BTreeSet::new(),
        }
    }
    pub fn register(&mut self, registration: ConsumerRegistration) -> Result<(), FeedError> {
        registration.validate()?;
        if self.consumers.contains_key(&registration.consumer_id)
            || self.retired.contains(&registration.consumer_id)
        {
            return Err(FeedError::ConsumerIdentityReused);
        }
        self.consumers
            .insert(registration.consumer_id, registration);
        Ok(())
    }
    pub fn transition(&mut self, id: ConsumerId, status: ConsumerStatus) -> Result<(), FeedError> {
        let registration = self
            .consumers
            .get_mut(&id)
            .ok_or(FeedError::UnknownConsumer)?;
        registration.status = registration.status.transition(status)?;
        Ok(())
    }
    pub fn retire(&mut self, id: ConsumerId) -> Result<ConsumerRegistration, FeedError> {
        let registration = self
            .consumers
            .remove(&id)
            .ok_or(FeedError::UnknownConsumer)?;
        if registration.status != ConsumerStatus::Disabled {
            return Err(FeedError::ConsumerNotDisabled);
        }
        self.retired.insert(id);
        Ok(registration)
    }
    #[must_use]
    pub fn get(&self, id: ConsumerId) -> Option<&ConsumerRegistration> {
        self.consumers.get(&id)
    }
}

impl Default for ConsumerRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FeedBatch {
    pub consumer_id: ConsumerId,
    pub authority_id: AuthorityId,
    pub authority_epoch: AuthorityEpoch,
    pub after: Sequence,
    pub events: Vec<ChangeFeedEvent>,
    pub total_bytes: usize,
    pub scanned_bytes: usize,
    /// Highest journal sequence inspected, including safely filtered-out events.
    pub scanned_through: Sequence,
}

impl FeedBatch {
    pub fn build(
        cursor: ConsumerCursor,
        events: impl IntoIterator<Item = ChangeFeedEvent>,
        registration: &ConsumerRegistration,
    ) -> Result<Self, FeedError> {
        registration.validate()?;
        if cursor.consumer_id != registration.consumer_id {
            return Err(FeedError::CursorConsumerMismatch);
        }
        let mut accepted = Vec::new();
        let mut total_bytes = 0_usize;
        let mut scanned_bytes = 0_usize;
        let mut scanned_events = 0_usize;
        let mut previous = cursor.sequence;
        for event in events {
            scanned_events = scanned_events
                .checked_add(1)
                .ok_or(FeedError::ArithmeticOverflow)?;
            scanned_bytes = scanned_bytes
                .checked_add(event.payload.len())
                .ok_or(FeedError::ArithmeticOverflow)?;
            if scanned_events > registration.batch.max_events
                || scanned_bytes > registration.batch.max_bytes
            {
                return Err(FeedError::LimitExceeded("journal scan batch"));
            }
            if event.authority_epoch != cursor.authority_epoch || event.sequence <= previous {
                return Err(FeedError::WrongEpochOrOrder);
            }
            previous = event.sequence;
            if registration
                .filter
                .matches(&event, &registration.authorization)
            {
                total_bytes = total_bytes
                    .checked_add(event.payload.len())
                    .ok_or(FeedError::ArithmeticOverflow)?;
                if accepted.len() >= registration.batch.max_events
                    || total_bytes > registration.batch.max_bytes
                {
                    return Err(FeedError::LimitExceeded("batch"));
                }
                accepted.push(event);
            }
        }
        Ok(Self {
            consumer_id: cursor.consumer_id,
            authority_id: cursor.authority_id,
            authority_epoch: cursor.authority_epoch,
            after: cursor.sequence,
            events: accepted,
            total_bytes,
            scanned_bytes,
            scanned_through: previous,
        })
    }
    #[must_use]
    pub fn through(&self) -> Sequence {
        self.scanned_through
    }
}

pub trait ConsumerProjector {
    type Output;
    fn project(&self, event: &ChangeFeedEvent) -> Result<Option<Self::Output>, ProjectionError>;
}

/// Bounded reference deduplication ledger; production stores persist the same `EventId` contract.
#[derive(Clone, Debug)]
pub struct ConsumerIdempotencyLedger {
    max_entries: usize,
    processed: BTreeMap<EventId, [u8; 32]>,
}

impl ConsumerIdempotencyLedger {
    pub fn new(max_entries: usize) -> Result<Self, FeedError> {
        if max_entries == 0 {
            return Err(FeedError::InvalidRegistration("dedup retention"));
        }
        Ok(Self {
            max_entries,
            processed: BTreeMap::new(),
        })
    }

    pub fn observe(&mut self, event: &ChangeFeedEvent) -> Result<DeliveryDisposition, FeedError> {
        if let Some(digest) = self.processed.get(&event.event_id) {
            return if *digest == event.payload_digest {
                Ok(DeliveryDisposition::Duplicate)
            } else {
                Err(FeedError::EventIdentityMismatch)
            };
        }
        if self.processed.len() >= self.max_entries {
            return Err(FeedError::LimitExceeded("dedup retention"));
        }
        self.processed.insert(event.event_id, event.payload_digest);
        Ok(DeliveryDisposition::FirstDelivery)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DeliveryDisposition {
    FirstDelivery,
    Duplicate,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ConsumerCapabilities {
    pub event_schemas: BTreeMap<EventKind, BTreeSet<EventSchemaVersion>>,
    pub projection_version: ConsumerProjectionVersion,
}

impl ConsumerCapabilities {
    pub fn activate_for(
        &self,
        required: &BTreeMap<EventKind, EventSchemaVersion>,
    ) -> Result<(), FeedError> {
        if required.iter().any(|(kind, schema)| {
            self.event_schemas
                .get(kind)
                .is_none_or(|schemas| !schemas.contains(schema))
        }) {
            return Err(FeedError::UnsupportedSchema);
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Error, Eq, PartialEq)]
pub enum ProjectionError {
    #[error("consumer cannot interpret required event semantics")]
    UnsupportedRequiredEvent,
    #[error("consumer projection rejected event: {0}")]
    Rejected(String),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ConsumerResult {
    Applied,
    Ignored,
    Retryable,
    Quarantine,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DurableEventReceipt {
    pub event_id: EventId,
    pub sequence: Sequence,
    pub result: ConsumerResult,
    pub effect_durable: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AckRequest {
    pub expected: ConsumerCursor,
    pub ack_through: Sequence,
    pub receipts: Vec<DurableEventReceipt>,
    pub projection_applied_through: Sequence,
    pub fencing_token: ConsumerFencingToken,
}

impl AckRequest {
    pub fn validate(
        &self,
        batch: &FeedBatch,
        lease: &ConsumerLease,
        now_unix_ms: u64,
    ) -> Result<ConsumerCursor, FeedError> {
        if self.expected.consumer_id != batch.consumer_id
            || self.expected.authority_id != batch.authority_id
            || self.expected.authority_epoch != batch.authority_epoch
            || self.expected.sequence != batch.after
        {
            return Err(FeedError::CheckpointConflict);
        }
        lease.validate(self.expected.consumer_id, self.fencing_token)?;
        if !lease.is_active(now_unix_ms) {
            return Err(FeedError::StaleWorker);
        }
        if self.ack_through != batch.through()
            || self.projection_applied_through < self.ack_through
            || self.receipts.len() != batch.events.len()
            || self
                .receipts
                .iter()
                .zip(&batch.events)
                .any(|(receipt, event)| {
                    receipt.event_id != event.event_id
                        || receipt.sequence != event.sequence
                        || !receipt.effect_durable
                        || !matches!(
                            receipt.result,
                            ConsumerResult::Applied | ConsumerResult::Ignored
                        )
                })
        {
            return Err(FeedError::EffectNotDurable);
        }
        Ok(ConsumerCursor {
            sequence: self.ack_through,
            ..self.expected
        })
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ConsumerLease {
    pub consumer_id: ConsumerId,
    pub worker_id: ConsumerWorkerId,
    pub fencing_token: ConsumerFencingToken,
    pub expires_at_unix_ms: u64,
}

impl ConsumerLease {
    pub fn validate(
        &self,
        consumer: ConsumerId,
        token: ConsumerFencingToken,
    ) -> Result<(), FeedError> {
        if self.consumer_id != consumer || self.fencing_token != token {
            return Err(FeedError::StaleWorker);
        }
        Ok(())
    }
    #[must_use]
    pub fn is_active(self, now_unix_ms: u64) -> bool {
        now_unix_ms < self.expires_at_unix_ms
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RetentionDecision {
    Continue,
    PinFloor(Sequence),
    Rebuild,
    SkipTo(Sequence),
    ManualRecovery,
}

pub fn evaluate_retention(
    registration: &ConsumerRegistration,
    cursor: ConsumerCursor,
    journal_floor: Sequence,
    current: Sequence,
) -> Result<RetentionDecision, FeedError> {
    if cursor.sequence >= journal_floor {
        return Ok(match registration.retention {
            ConsumerRetentionPolicy::PinJournal => RetentionDecision::PinFloor(cursor.sequence),
            _ => RetentionDecision::Continue,
        });
    }
    match registration.retention {
        ConsumerRetentionPolicy::PinJournal => Err(FeedError::PinnedHistoryLost),
        ConsumerRetentionPolicy::RebuildIfBehind => Ok(RetentionDecision::Rebuild),
        ConsumerRetentionPolicy::BestEffort => Ok(RetentionDecision::SkipTo(current)),
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EpochRecoveryDecision {
    ContinueMapped(ConsumerCursor),
    ReplayFromBoundary(Sequence),
    Rebuild,
    ManualRecovery,
}

pub fn recover_epoch(
    registration: &ConsumerRegistration,
    cursor: ConsumerCursor,
    authority_id: AuthorityId,
    authority_epoch: AuthorityEpoch,
    mapped_boundary: Option<Sequence>,
) -> Result<EpochRecoveryDecision, FeedError> {
    if cursor.authority_id != authority_id || authority_epoch < cursor.authority_epoch {
        return Err(FeedError::AuthorityRollback);
    }
    if cursor.authority_epoch == authority_epoch {
        return Ok(EpochRecoveryDecision::ContinueMapped(cursor));
    }
    Ok(match registration.epoch_recovery {
        ConsumerEpochRecoveryPolicy::ContinueIfMapped => {
            let boundary = mapped_boundary.ok_or(FeedError::MissingRecoveryBoundary)?;
            EpochRecoveryDecision::ContinueMapped(ConsumerCursor {
                authority_epoch,
                sequence: boundary,
                ..cursor
            })
        }
        ConsumerEpochRecoveryPolicy::ReplayFromRecoveryBoundary => {
            EpochRecoveryDecision::ReplayFromBoundary(
                mapped_boundary.ok_or(FeedError::MissingRecoveryBoundary)?,
            )
        }
        ConsumerEpochRecoveryPolicy::Rebuild => EpochRecoveryDecision::Rebuild,
        ConsumerEpochRecoveryPolicy::Manual => EpochRecoveryDecision::ManualRecovery,
    })
}

pub fn partition_for(
    registration: &ConsumerRegistration,
    event: &ChangeFeedEvent,
    partitions: &[ConsumerPartitionId],
) -> Result<ConsumerPartitionId, FeedError> {
    if partitions.is_empty() || partitions.len() > registration.batch.max_concurrency {
        return Err(FeedError::InvalidPartitioning);
    }
    if registration.ordering == ConsumerOrderingPolicy::Global && partitions.len() != 1 {
        return Err(FeedError::OrderingViolation);
    }
    let mut hasher = blake3::Hasher::new();
    match registration.ordering {
        ConsumerOrderingPolicy::Global => {
            hasher.update(b"global");
        }
        ConsumerOrderingPolicy::PerTenant => {
            hasher.update(event.tenant_id.as_uuid().as_bytes());
        }
        ConsumerOrderingPolicy::PerEntity => {
            hasher.update(event.tenant_id.as_uuid().as_bytes());
            hasher.update(event.entity.entity_id.as_uuid().as_bytes());
        }
        ConsumerOrderingPolicy::Unordered => {
            hasher.update(event.event_id.as_uuid().as_bytes());
        }
    }
    let bytes = hasher.finalize();
    let index = u64::from_le_bytes(
        bytes.as_bytes()[..8]
            .try_into()
            .map_err(|_| FeedError::ArithmeticOverflow)?,
    ) % u64::try_from(partitions.len()).map_err(|_| FeedError::ArithmeticOverflow)?;
    Ok(partitions[usize::try_from(index).map_err(|_| FeedError::ArithmeticOverflow)?])
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ConsumerQuarantineRecord {
    pub consumer_id: ConsumerId,
    pub event_id: EventId,
    pub sequence: Sequence,
    pub error_code: String,
    pub payload_digest: [u8; 32],
    pub state: QuarantineState,
    pub created_at: Timestamp,
}

impl ConsumerQuarantineRecord {
    pub fn validate(&self) -> Result<(), FeedError> {
        if self.error_code.is_empty() || self.error_code.len() > MAX_ERROR_CODE_BYTES {
            return Err(FeedError::LimitExceeded("quarantine error code"));
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum QuarantineState {
    Pending,
    RetryApproved,
    IgnoredWithAudit,
    ResolvedByRebuild,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FailureAction {
    Stop,
    Retry,
    Quarantine,
    SkipWithAudit,
}

pub fn failure_action(
    policy: ConsumerFailurePolicy,
    authoritative_projection: bool,
) -> Result<FailureAction, FeedError> {
    match policy {
        ConsumerFailurePolicy::Stop => Ok(FailureAction::Stop),
        ConsumerFailurePolicy::Retry => Ok(FailureAction::Retry),
        ConsumerFailurePolicy::Quarantine => Ok(FailureAction::Quarantine),
        ConsumerFailurePolicy::SkipWithAudit if !authoritative_projection => {
            Ok(FailureAction::SkipWithAudit)
        }
        ConsumerFailurePolicy::SkipWithAudit => Err(FeedError::UnsafeSkip),
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum ConsumerResetMode {
    ReplayFrom(Sequence),
    RebuildFromSnapshot(SnapshotId),
    SkipToCurrent,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ConsumerResetPlan {
    pub plan_id: ConsumerResetPlanId,
    pub consumer_id: ConsumerId,
    pub expected_cursor: ConsumerCursor,
    pub mode: ConsumerResetMode,
    pub reason: String,
    pub authorized_by: String,
    pub approved: bool,
    pub audit_event_id: Option<Uuid>,
    pub expires_at_unix_ms: u64,
}

impl ConsumerResetPlan {
    pub fn validate(
        &self,
        authorization: &ConsumerAuthorization,
        now_unix_ms: u64,
    ) -> Result<(), FeedError> {
        if self.reason.is_empty()
            || self.reason.len() > 4_096
            || self.authorized_by.is_empty()
            || !self.approved
            || self.audit_event_id.is_none()
            || now_unix_ms >= self.expires_at_unix_ms
        {
            return Err(FeedError::ResetNotAuthorized);
        }
        if matches!(self.mode, ConsumerResetMode::ReplayFrom(_))
            && !authorization.can_historical_replay
        {
            return Err(FeedError::ReplayDenied);
        }
        if matches!(self.mode, ConsumerResetMode::SkipToCurrent) && !authorization.can_skip_history
        {
            return Err(FeedError::ResetNotAuthorized);
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ConsumerCheckpoint {
    pub checkpoint_id: ConsumerCheckpointId,
    pub cursor: ConsumerCursor,
    pub projection_applied_sequence: Sequence,
    pub projection_digest: Option<[u8; 32]>,
    pub updated_at: Timestamp,
}

impl ConsumerCheckpoint {
    pub fn verify_two_sided(self) -> Result<(), FeedError> {
        if self.cursor.sequence > self.projection_applied_sequence {
            Err(FeedError::ProjectionBehindCursor)
        } else {
            Ok(())
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FeedArchiveSegment {
    pub segment_id: FeedArchiveSegmentId,
    pub authority_id: AuthorityId,
    pub authority_epoch: AuthorityEpoch,
    pub start_sequence: Sequence,
    pub end_sequence: Sequence,
    pub object_ref: String,
    pub root_digest: [u8; 32],
    pub complete: bool,
    pub manifest_durable: bool,
}

impl FeedArchiveSegment {
    #[must_use]
    pub fn digest(events: &[ChangeFeedEvent]) -> [u8; 32] {
        let mut hasher = blake3::Hasher::new();
        hasher.update(b"aequora.feed.archive.v1\0");
        for event in events {
            hasher.update(event.event_id.as_uuid().as_bytes());
            hasher.update(&event.sequence.0.to_le_bytes());
            hasher.update(&event.payload_digest);
        }
        *hasher.finalize().as_bytes()
    }
    pub fn verify(&self, events: &[ChangeFeedEvent]) -> Result<(), FeedError> {
        if !self.complete
            || !self.manifest_durable
            || self.start_sequence > self.end_sequence
            || events
                .first()
                .is_none_or(|event| event.sequence != self.start_sequence)
            || events
                .last()
                .is_none_or(|event| event.sequence != self.end_sequence)
            || Self::digest(events) != self.root_digest
        {
            return Err(FeedError::ArchiveVerificationFailed);
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FeedLag {
    pub sequence_lag: u64,
    pub oldest_event_age_ms: Option<u64>,
    pub stalled: bool,
}

pub fn calculate_lag(
    cursor: Sequence,
    authority: Sequence,
    oldest_event_age_ms: Option<u64>,
    unchanged_while_authority_advanced: bool,
) -> Result<FeedLag, FeedError> {
    let sequence_lag = authority
        .0
        .checked_sub(cursor.0)
        .ok_or(FeedError::CursorAheadOfAuthority)?;
    Ok(FeedLag {
        sequence_lag,
        oldest_event_age_ms,
        stalled: sequence_lag > 0 && unchanged_while_authority_advanced,
    })
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct IntegrationEvent {
    pub kind: String,
    pub schema_version: u32,
    pub visibility: FeedVisibility,
    pub tenant_id: TenantId,
    pub event_id: EventId,
    pub minimized_payload: Vec<u8>,
}

impl IntegrationEvent {
    pub fn validate(&self, registration: &ConsumerRegistration) -> Result<(), FeedError> {
        if self.kind.is_empty()
            || self.kind.len() > MAX_CONSUMER_NAME_BYTES
            || self.schema_version == 0
            || !registration.authorization.tenants.contains(&self.tenant_id)
            || !registration
                .authorization
                .allowed_visibility
                .contains(&self.visibility)
            || self.visibility == FeedVisibility::InternalOnly
        {
            return Err(FeedError::VisibilityDenied);
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ConsumerStorageSurface {
    pub consumer_id: ConsumerId,
    pub contains_governed_tenant_data: bool,
    pub governance_registered: bool,
    pub residency_policy_id: Option<String>,
    pub region: Option<aequora_types::RegionId>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum ConsumerReplaySource {
    HotJournal,
    Archive,
    SnapshotPlusJournal,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ConsumerRebuildPlan {
    pub consumer_id: ConsumerId,
    pub snapshot_id: SnapshotId,
    pub authority_id: AuthorityId,
    pub authority_epoch: AuthorityEpoch,
    pub snapshot_sequence: Sequence,
    pub projection_version: ConsumerProjectionVersion,
    pub source: ConsumerReplaySource,
    pub expected_projection_digest: Option<[u8; 32]>,
}

impl ConsumerRebuildPlan {
    pub fn activate(
        &self,
        built_through: Sequence,
        actual_digest: Option<[u8; 32]>,
    ) -> Result<ConsumerCursor, FeedError> {
        if built_through != self.snapshot_sequence
            || self
                .expected_projection_digest
                .is_some_and(|expected| Some(expected) != actual_digest)
        {
            return Err(FeedError::RebuildVerificationFailed);
        }
        Ok(ConsumerCursor {
            consumer_id: self.consumer_id,
            authority_id: self.authority_id,
            authority_epoch: self.authority_epoch,
            sequence: self.snapshot_sequence,
        })
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ConsumerAdminView {
    pub consumer_id: ConsumerId,
    pub status: ConsumerStatus,
    pub cursor: ConsumerCursor,
    pub lag: FeedLag,
    pub projection_version: ConsumerProjectionVersion,
    pub active_lease: Option<ConsumerLease>,
    pub quarantine_count: usize,
    pub last_error_code: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ConsumerIncidentView {
    pub consumer_id: ConsumerId,
    pub event_id: Option<EventId>,
    pub cursor: ConsumerCursor,
    pub lag: FeedLag,
    pub projection_version: ConsumerProjectionVersion,
    pub last_checkpoint: Option<ConsumerCheckpoint>,
    pub quarantined_event_ids: Vec<EventId>,
}

impl ConsumerIncidentView {
    pub fn validate(&self, maximum_quarantine_refs: usize) -> Result<(), FeedError> {
        if maximum_quarantine_refs == 0
            || self.quarantined_event_ids.len() > maximum_quarantine_refs
        {
            return Err(FeedError::LimitExceeded("incident quarantine references"));
        }
        Ok(())
    }
}

impl ConsumerStorageSurface {
    pub fn validate(&self) -> Result<(), FeedError> {
        if self.contains_governed_tenant_data
            && (!self.governance_registered
                || self
                    .residency_policy_id
                    .as_deref()
                    .is_none_or(str::is_empty))
        {
            return Err(FeedError::GovernanceCoverageMissing);
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FeedBatchManifest {
    pub consumer_id: ConsumerId,
    pub authority_epoch: AuthorityEpoch,
    pub start_sequence: Sequence,
    pub end_sequence: Sequence,
    pub root_digest: [u8; 32],
}

impl FeedBatchManifest {
    #[must_use]
    pub fn from_batch(batch: &FeedBatch) -> Self {
        let mut hasher = blake3::Hasher::new();
        hasher.update(b"aequora.feed.batch.v1\0");
        for event in &batch.events {
            hasher.update(event.event_id.as_uuid().as_bytes());
            hasher.update(&event.payload_digest);
        }
        Self {
            consumer_id: batch.consumer_id,
            authority_epoch: batch.authority_epoch,
            start_sequence: batch
                .events
                .first()
                .map_or(batch.after, |event| event.sequence),
            end_sequence: batch.through(),
            root_digest: *hasher.finalize().as_bytes(),
        }
    }
}

#[async_trait]
pub trait ConsumerStore: Send + Sync {
    async fn registration(
        &self,
        consumer: ConsumerId,
    ) -> Result<Option<ConsumerRegistration>, FeedError>;
    async fn cursor(&self, consumer: ConsumerId) -> Result<Option<ConsumerCursor>, FeedError>;
    async fn claim(
        &self,
        consumer: ConsumerId,
        worker: ConsumerWorkerId,
        now_unix_ms: u64,
        lease_ms: u64,
    ) -> Result<ConsumerLease, FeedError>;
    async fn checkpoint(
        &self,
        request: AckRequest,
        batch: &FeedBatch,
        lease: &ConsumerLease,
        now_unix_ms: u64,
    ) -> Result<ConsumerCursor, FeedError>;
    async fn quarantine(
        &self,
        record: ConsumerQuarantineRecord,
        fencing_token: ConsumerFencingToken,
    ) -> Result<(), FeedError>;
}

#[async_trait]
pub trait ChangeFeedSource: Send + Sync {
    async fn scan(
        &self,
        registration: &ConsumerRegistration,
        cursor: ConsumerCursor,
        limits: BatchLimits,
    ) -> Result<FeedBatch, FeedError>;
}

#[async_trait]
pub trait BrokerAdapter: Send + Sync {
    async fn publish(
        &self,
        manifest: &FeedBatchManifest,
        events: &[IntegrationEvent],
    ) -> Result<BrokerReceipt, FeedError>;
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BrokerReceipt {
    pub publication_id: String,
    pub accepted_event_ids: BTreeSet<EventId>,
    pub durable: bool,
}

impl BrokerReceipt {
    pub fn validate(&self, events: &[IntegrationEvent]) -> Result<(), FeedError> {
        if !self.durable
            || self.publication_id.is_empty()
            || events
                .iter()
                .any(|event| !self.accepted_event_ids.contains(&event.event_id))
        {
            return Err(FeedError::BrokerPublicationNotDurable);
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FeedInvariant {
    pub id: &'static str,
    pub test: &'static str,
    pub metric: &'static str,
}

pub const FEED_INVARIANTS: [FeedInvariant; 9] = [
    FeedInvariant {
        id: "AEQ-INV-FEED001",
        test: "independent_epoch_bound_cursor",
        metric: "consumer_cursor_invalid_total",
    },
    FeedInvariant {
        id: "AEQ-INV-FEED002",
        test: "durable_effect_before_ack",
        metric: "consumer_unsafe_ack_total",
    },
    FeedInvariant {
        id: "AEQ-INV-FEED003",
        test: "lagging_consumer_isolation",
        metric: "consumer_isolation_violation_total",
    },
    FeedInvariant {
        id: "AEQ-INV-FEED004",
        test: "duplicate_delivery_idempotency",
        metric: "consumer_duplicate_effect_total",
    },
    FeedInvariant {
        id: "AEQ-INV-FEED005",
        test: "journal_floor_recovery_policy",
        metric: "consumer_floor_miss_total",
    },
    FeedInvariant {
        id: "AEQ-INV-FEED006",
        test: "external_projection_visibility",
        metric: "feed_visibility_denied_total",
    },
    FeedInvariant {
        id: "AEQ-INV-FEED007",
        test: "partition_ordering_stability",
        metric: "consumer_ordering_violation_total",
    },
    FeedInvariant {
        id: "AEQ-INV-FEED008",
        test: "reset_plan_authorization_audit",
        metric: "consumer_reset_denied_total",
    },
    FeedInvariant {
        id: "AEQ-INV-FEED009",
        test: "governance_residency_coverage",
        metric: "consumer_governance_gap_total",
    },
];

#[derive(Clone, Debug, Error, Eq, PartialEq)]
pub enum FeedError {
    #[error("numeric identity must be non-zero")]
    ZeroIdentity,
    #[error("invalid consumer registration: {0}")]
    InvalidRegistration(&'static str),
    #[error("consumer tenant access denied")]
    TenantDenied,
    #[error("feed visibility denied")]
    VisibilityDenied,
    #[error("feed limit exceeded: {0}")]
    LimitExceeded(&'static str),
    #[error("consumer identity cannot be reused")]
    ConsumerIdentityReused,
    #[error("unknown consumer")]
    UnknownConsumer,
    #[error("invalid consumer status transition")]
    InvalidStatusTransition,
    #[error("consumer must be disabled before retirement")]
    ConsumerNotDisabled,
    #[error("cursor belongs to another consumer")]
    CursorConsumerMismatch,
    #[error("event epoch or order is invalid")]
    WrongEpochOrOrder,
    #[error("checked feed arithmetic overflowed")]
    ArithmeticOverflow,
    #[error("consumer checkpoint changed concurrently")]
    CheckpointConflict,
    #[error("consumer effect is not durably complete")]
    EffectNotDurable,
    #[error("worker lease is stale or fenced")]
    StaleWorker,
    #[error("required pinned history was lost")]
    PinnedHistoryLost,
    #[error("authority rollback detected")]
    AuthorityRollback,
    #[error("epoch recovery boundary is missing")]
    MissingRecoveryBoundary,
    #[error("partition configuration is invalid")]
    InvalidPartitioning,
    #[error("declared ordering would be weakened")]
    OrderingViolation,
    #[error("skipping an event is unsafe for this consumer")]
    UnsafeSkip,
    #[error("consumer reset lacks authorization, approval, or audit")]
    ResetNotAuthorized,
    #[error("historical replay is not authorized")]
    ReplayDenied,
    #[error("projection is behind its Aequora cursor")]
    ProjectionBehindCursor,
    #[error("archive segment verification failed")]
    ArchiveVerificationFailed,
    #[error("consumer cursor is ahead of authority")]
    CursorAheadOfAuthority,
    #[error("governance or residency coverage is missing")]
    GovernanceCoverageMissing,
    #[error("broker publication is not durably acknowledged")]
    BrokerPublicationNotDurable,
    #[error("journal event failed canonical integrity validation")]
    CorruptJournalEvent,
    #[error("event schema is unsupported")]
    UnsupportedSchema,
    #[error("one EventId was reused with different consumer semantics")]
    EventIdentityMismatch,
    #[error("consumer rebuild verification failed")]
    RebuildVerificationFailed,
}

#[cfg(test)]
mod tests {
    use super::*;
    use aequora_types::{EntityId, EntityType, EntityVersion};

    fn registration(tenant: TenantId) -> ConsumerRegistration {
        ConsumerRegistration {
            consumer_id: ConsumerId::new(),
            group_id: ConsumerGroupId::new(),
            kind: ConsumerKind::new(1).unwrap_or_else(|error| panic!("{error}")),
            name: "search-main".to_owned(),
            status: ConsumerStatus::Active,
            filter: ConsumerFilter {
                tenants: TenantSelector::Explicit(BTreeSet::from([tenant])),
                entity_types: BTreeSet::new(),
                event_kinds: BTreeSet::new(),
                operation_kinds: BTreeSet::new(),
            },
            filter_version: ConsumerFilterVersion::new(1).unwrap_or_else(|error| panic!("{error}")),
            projection_version: ConsumerProjectionVersion::new(1)
                .unwrap_or_else(|error| panic!("{error}")),
            rule_version: ConsumerRuleVersion::new(1).unwrap_or_else(|error| panic!("{error}")),
            ordering: ConsumerOrderingPolicy::PerEntity,
            retention: ConsumerRetentionPolicy::RebuildIfBehind,
            failure: ConsumerFailurePolicy::Quarantine,
            read_policy: ConsumerReadPolicy::ReplicaAllowed,
            epoch_recovery: ConsumerEpochRecoveryPolicy::Rebuild,
            rebuildability: Rebuildability::CanRebuild,
            bootstrap: ConsumerBootstrapMode::SnapshotAndTail,
            delivery_mode: DeliveryMode::InternalDirect,
            visibility: FeedVisibility::InternalOnly,
            authorization: ConsumerAuthorization {
                service_identity: "search-worker".to_owned(),
                tenants: BTreeSet::from([tenant]),
                allowed_visibility: BTreeSet::from([FeedVisibility::InternalOnly]),
                can_historical_replay: false,
                can_skip_history: false,
            },
            batch: BatchLimits {
                max_events: 100,
                max_bytes: 1024 * 1024,
                max_processing_ms: 10_000,
                max_concurrency: 4,
            },
            max_retention_pin_age_ms: None,
            created_at: Timestamp::new(1, 0).unwrap_or_else(|error| panic!("{error}")),
        }
    }

    fn event(tenant: TenantId, sequence: u64, entity: EntityId) -> ChangeFeedEvent {
        let payload = sequence.to_le_bytes().to_vec();
        ChangeFeedEvent {
            event_id: EventId::new(),
            authority_epoch: AuthorityEpoch::INITIAL,
            sequence: Sequence(sequence),
            tenant_id: tenant,
            entity: EntityRef {
                entity_type: EntityType::new(1).unwrap_or_else(|error| panic!("{error}")),
                entity_id: entity,
            },
            entity_version: EntityVersion::INITIAL,
            event_kind: EventKind::new(1).unwrap_or_else(|error| panic!("{error}")),
            schema_version: EventSchemaVersion::new(1).unwrap_or_else(|error| panic!("{error}")),
            payload_digest: *blake3::hash(&payload).as_bytes(),
            payload,
            operation_id: OperationId::new(),
            operation_kind: Some(1),
            correlation_id: CorrelationId::new(),
            occurred_at: Timestamp::new(1, 0).unwrap_or_else(|error| panic!("{error}")),
        }
    }

    fn cursor(consumer_id: ConsumerId, sequence: u64) -> ConsumerCursor {
        ConsumerCursor {
            consumer_id,
            authority_id: AuthorityId::LOCAL_DEVELOPMENT,
            authority_epoch: AuthorityEpoch::INITIAL,
            sequence: Sequence(sequence),
        }
    }

    #[test]
    fn independent_epoch_bound_cursor_advances_only_after_durable_effect() {
        let tenant = TenantId::new();
        let registration = registration(tenant);
        let event = event(tenant, 1, EntityId::new());
        let batch = FeedBatch::build(
            cursor(registration.consumer_id, 0),
            [event.clone()],
            &registration,
        )
        .unwrap_or_else(|error| panic!("{error}"));
        let lease = ConsumerLease {
            consumer_id: registration.consumer_id,
            worker_id: ConsumerWorkerId::new(),
            fencing_token: ConsumerFencingToken::INITIAL,
            expires_at_unix_ms: 100,
        };
        let mut ack = AckRequest {
            expected: cursor(registration.consumer_id, 0),
            ack_through: Sequence(1),
            receipts: vec![DurableEventReceipt {
                event_id: event.event_id,
                sequence: Sequence(1),
                result: ConsumerResult::Applied,
                effect_durable: false,
            }],
            projection_applied_through: Sequence(1),
            fencing_token: ConsumerFencingToken::INITIAL,
        };
        assert_eq!(
            ack.validate(&batch, &lease, 1),
            Err(FeedError::EffectNotDurable)
        );
        ack.receipts[0].effect_durable = true;
        assert_eq!(
            ack.validate(&batch, &lease, 1).map(|value| value.sequence),
            Ok(Sequence(1))
        );
    }

    #[test]
    fn safely_filtered_events_still_advance_scanned_cursor() {
        let tenant = TenantId::new();
        let mut registration = registration(tenant);
        registration.filter.event_kinds =
            BTreeSet::from([EventKind::new(2).unwrap_or_else(|error| panic!("{error}"))]);
        let batch = FeedBatch::build(
            cursor(registration.consumer_id, 0),
            [event(tenant, 7, EntityId::new())],
            &registration,
        )
        .unwrap_or_else(|error| panic!("{error}"));
        assert!(batch.events.is_empty());
        assert_eq!(batch.through(), Sequence(7));
    }

    #[test]
    fn duplicate_delivery_has_one_logical_identity() {
        let event = event(TenantId::new(), 1, EntityId::new());
        let mut ledger =
            ConsumerIdempotencyLedger::new(2).unwrap_or_else(|error| panic!("{error}"));
        assert_eq!(
            ledger.observe(&event),
            Ok(DeliveryDisposition::FirstDelivery)
        );
        assert_eq!(ledger.observe(&event), Ok(DeliveryDisposition::Duplicate));
        let mut substituted = event.clone();
        substituted.payload_digest = [9; 32];
        assert_eq!(
            ledger.observe(&substituted),
            Err(FeedError::EventIdentityMismatch)
        );
    }

    #[test]
    fn floor_and_epoch_recovery_never_invent_history() {
        let tenant = TenantId::new();
        let registration = registration(tenant);
        let cursor = cursor(registration.consumer_id, 2);
        assert_eq!(
            evaluate_retention(&registration, cursor, Sequence(3), Sequence(9)),
            Ok(RetentionDecision::Rebuild)
        );
        let epoch_two = AuthorityEpoch::new(2).unwrap_or(AuthorityEpoch::INITIAL);
        assert_eq!(
            recover_epoch(
                &registration,
                cursor,
                AuthorityId::LOCAL_DEVELOPMENT,
                epoch_two,
                None
            ),
            Ok(EpochRecoveryDecision::Rebuild)
        );
        assert_eq!(
            recover_epoch(
                &registration,
                ConsumerCursor {
                    authority_epoch: epoch_two,
                    ..cursor
                },
                AuthorityId::LOCAL_DEVELOPMENT,
                AuthorityEpoch::INITIAL,
                None,
            ),
            Err(FeedError::AuthorityRollback)
        );
    }

    #[test]
    fn ordering_partition_is_stable_and_global_parallelism_fails() {
        let tenant = TenantId::new();
        let mut registration = registration(tenant);
        let partitions = [ConsumerPartitionId::new(), ConsumerPartitionId::new()];
        let first = event(tenant, 1, EntityId::new());
        let mut second = first.clone();
        second.event_id = EventId::new();
        second.sequence = Sequence(2);
        assert_eq!(
            partition_for(&registration, &first, &partitions),
            partition_for(&registration, &second, &partitions)
        );
        registration.ordering = ConsumerOrderingPolicy::Global;
        assert_eq!(
            partition_for(&registration, &first, &partitions),
            Err(FeedError::OrderingViolation)
        );
    }

    #[test]
    fn cross_tenant_filter_and_external_visibility_fail_closed() {
        let tenant = TenantId::new();
        let mut invalid_registration = registration(tenant);
        invalid_registration.filter.tenants =
            TenantSelector::Explicit(BTreeSet::from([TenantId::new()]));
        assert_eq!(
            invalid_registration.validate(),
            Err(FeedError::TenantDenied)
        );

        let integration = IntegrationEvent {
            kind: "student.updated.v1".to_owned(),
            schema_version: 1,
            visibility: FeedVisibility::PublicIntegration,
            tenant_id: tenant,
            event_id: EventId::new(),
            minimized_payload: vec![],
        };
        assert_eq!(
            integration.validate(&registration(tenant)),
            Err(FeedError::VisibilityDenied)
        );
    }

    #[test]
    fn reset_requires_plan_authorization_and_audit() {
        let tenant = TenantId::new();
        let registration = registration(tenant);
        let mut plan = ConsumerResetPlan {
            plan_id: ConsumerResetPlanId::new(),
            consumer_id: registration.consumer_id,
            expected_cursor: cursor(registration.consumer_id, 5),
            mode: ConsumerResetMode::SkipToCurrent,
            reason: "projection corrupted".to_owned(),
            authorized_by: "operator".to_owned(),
            approved: true,
            audit_event_id: Some(Uuid::now_v7()),
            expires_at_unix_ms: 100,
        };
        assert_eq!(
            plan.validate(&registration.authorization, 1),
            Err(FeedError::ResetNotAuthorized)
        );
        plan.mode = ConsumerResetMode::RebuildFromSnapshot(SnapshotId::new());
        assert!(plan.validate(&registration.authorization, 1).is_ok());
    }

    #[test]
    fn archive_rebuild_and_governance_require_verification() {
        let tenant = TenantId::new();
        let events = [
            event(tenant, 1, EntityId::new()),
            event(tenant, 2, EntityId::new()),
        ];
        let mut archive = FeedArchiveSegment {
            segment_id: FeedArchiveSegmentId::new(),
            authority_id: AuthorityId::LOCAL_DEVELOPMENT,
            authority_epoch: AuthorityEpoch::INITIAL,
            start_sequence: Sequence(1),
            end_sequence: Sequence(2),
            object_ref: "opaque/segment".to_owned(),
            root_digest: FeedArchiveSegment::digest(&events),
            complete: true,
            manifest_durable: true,
        };
        assert!(archive.verify(&events).is_ok());
        archive.root_digest = [0; 32];
        assert_eq!(
            archive.verify(&events),
            Err(FeedError::ArchiveVerificationFailed)
        );
        let surface = ConsumerStorageSurface {
            consumer_id: ConsumerId::new(),
            contains_governed_tenant_data: true,
            governance_registered: false,
            residency_policy_id: None,
            region: None,
        };
        assert_eq!(
            surface.validate(),
            Err(FeedError::GovernanceCoverageMissing)
        );
    }

    #[test]
    fn capability_and_invariant_registries_are_complete() {
        let kind = EventKind::new(1).unwrap_or_else(|error| panic!("{error}"));
        let schema = EventSchemaVersion::new(1).unwrap_or_else(|error| panic!("{error}"));
        let capabilities = ConsumerCapabilities {
            event_schemas: BTreeMap::from([(kind, BTreeSet::from([schema]))]),
            projection_version: ConsumerProjectionVersion::new(1)
                .unwrap_or_else(|error| panic!("{error}")),
        };
        assert!(
            capabilities
                .activate_for(&BTreeMap::from([(kind, schema)]))
                .is_ok()
        );
        assert_eq!(
            FEED_INVARIANTS
                .iter()
                .map(|item| item.id)
                .collect::<BTreeSet<_>>()
                .len(),
            9
        );
    }
}
