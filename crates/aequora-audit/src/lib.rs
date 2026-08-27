//! Canonical business-audit, provenance, tamper-evidence, query, and explanation contracts.
//!
//! Sync journals replicate state, operation ledgers deduplicate effects, and logs diagnose runtime
//! behavior. This crate defines a fourth, independent history: durable business audit evidence.
//! It owns no database, network, cryptographic signing key, localization, or UI dependency.

#![allow(clippy::missing_errors_doc)]

use aequora_types::{
    ActorId, AuthorityTimeline, CorrelationId, DeviceId, EntityRef, EventId, LineageRef,
    OperationId, RepairId, SyncScopeId, TenantId,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use thiserror::Error;
use uuid::Uuid;

pub const AUDIT_FORMAT_VERSION: u16 = 1;
pub const MAX_AUDIT_CHANGES: usize = 256;
pub const MAX_AUDIT_PARAMETERS: usize = 64;
pub const MAX_AUDIT_VALUE_BYTES: usize = 16 * 1024;
pub const MAX_QUERY_LIMIT: usize = 1_000;
const MAX_LABEL_BYTES: usize = 256;

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
    AuditEventId,
    "Globally unique canonical business-audit event identity."
);
uuid_id!(
    AuditExportId,
    "Globally unique identity of an authorized audit export."
);

/// Stable registered application audit action.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct AuditActionId(u32);

impl AuditActionId {
    pub const fn new(value: u32) -> Result<Self, AuditError> {
        if value == 0 {
            Err(AuditError::ZeroIdentity)
        } else {
            Ok(Self(value))
        }
    }

    #[must_use]
    pub const fn get(self) -> u32 {
        self.0
    }
}

/// Stable registered application field identity, independent of Rust field names.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct AuditFieldId(u32);

impl AuditFieldId {
    pub const fn new(value: u32) -> Result<Self, AuditError> {
        if value == 0 {
            Err(AuditError::ZeroIdentity)
        } else {
            Ok(Self(value))
        }
    }

    #[must_use]
    pub const fn get(self) -> u32 {
        self.0
    }
}

/// Stable machine-readable explanation reason, localized outside the core.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct ReasonCode(u32);

impl ReasonCode {
    pub const fn new(value: u32) -> Result<Self, AuditError> {
        if value == 0 {
            Err(AuditError::ZeroIdentity)
        } else {
            Ok(Self(value))
        }
    }

    #[must_use]
    pub const fn get(self) -> u32 {
        self.0
    }
}

/// Authoritative occurrence time captured by the domain execution context, in Unix milliseconds.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct AuditTimestamp(pub u64);

/// Per-tenant, per-partition canonical audit order. Independent from sync journal sequence.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct AuditSequence(pub u64);

/// What one business audit event concerns.
#[derive(Clone, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
pub enum AuditSubject {
    Entity(EntityRef),
    Aggregate {
        aggregate_type: u32,
        aggregate_key: String,
    },
    Scope(SyncScopeId),
    Device(DeviceId),
    ImportJob(Uuid),
    Principal(ActorId),
    SystemResource {
        resource_type: u32,
        resource_key: String,
    },
}

impl AuditSubject {
    fn verify(&self) -> Result<(), AuditError> {
        match self {
            Self::Aggregate {
                aggregate_type,
                aggregate_key,
            }
            | Self::SystemResource {
                resource_type: aggregate_type,
                resource_key: aggregate_key,
            } => {
                if *aggregate_type == 0 {
                    return Err(AuditError::ZeroIdentity);
                }
                verify_label(aggregate_key)
            }
            _ => Ok(()),
        }
    }
}

/// Truthful initiator identity. Scheduled work must use `Service` or `System`, never a stale user.
#[derive(Clone, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
pub enum AuditActor {
    User {
        actor_id: ActorId,
        display_ref: Option<String>,
    },
    Service {
        service_id: String,
    },
    System {
        component_id: String,
    },
    Import {
        import_job_id: Uuid,
    },
}

impl AuditActor {
    fn verify(&self) -> Result<(), AuditError> {
        match self {
            Self::User { display_ref, .. } => display_ref.as_deref().map_or(Ok(()), verify_label),
            Self::Service { service_id } => verify_label(service_id),
            Self::System { component_id } => verify_label(component_id),
            Self::Import { .. } => Ok(()),
        }
    }

    #[must_use]
    pub const fn actor_id(&self) -> Option<ActorId> {
        match self {
            Self::User { actor_id, .. } => Some(*actor_id),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
pub enum AuditCategory {
    BusinessChange,
    Security,
    Administrative,
    DataAccess,
    Migration,
    Repair,
    Configuration,
    Authentication,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum AuditOutcome {
    Accepted,
    Rejected,
    Conflict,
    Compensated,
    Revoked,
    Repaired,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum AuditDurability {
    RequiredAtomic,
    DurableAsync,
    BestEffort,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum AuditRetentionClass {
    Short,
    Standard,
    LongTerm,
    LegalHoldEligible,
    PermanentByPolicy,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum AuditValuePolicy {
    Full,
    Redacted,
    Hashed,
    MetadataOnly,
    Omit,
}

/// Canonical policy-filtered value. Secrets should use digest, metadata, or omission.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum AuditValue {
    Full(Vec<u8>),
    Redacted(String),
    Digest([u8; 32]),
    MetadataOnly { present: bool, byte_len: u32 },
}

impl AuditValue {
    fn verify(&self) -> Result<(), AuditError> {
        match self {
            Self::Full(value) if value.len() > MAX_AUDIT_VALUE_BYTES => {
                Err(AuditError::LimitExceeded)
            }
            Self::Redacted(value) => verify_label(value),
            Self::Digest(value) if *value == [0; 32] => Err(AuditError::ZeroDigest),
            Self::MetadataOnly { byte_len, .. }
                if usize::try_from(*byte_len).unwrap_or(usize::MAX) > MAX_AUDIT_VALUE_BYTES =>
            {
                Err(AuditError::LimitExceeded)
            }
            _ => Ok(()),
        }
    }

    #[must_use]
    pub const fn policy(&self) -> AuditValuePolicy {
        match self {
            Self::Full(_) => AuditValuePolicy::Full,
            Self::Redacted(_) => AuditValuePolicy::Redacted,
            Self::Digest(_) => AuditValuePolicy::Hashed,
            Self::MetadataOnly { .. } => AuditValuePolicy::MetadataOnly,
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum AuditChangeKind {
    Set,
    Cleared,
    Added,
    Removed,
}

/// One stable, policy-filtered field change.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct AuditChange {
    pub field: AuditFieldId,
    pub kind: AuditChangeKind,
    pub before: Option<AuditValue>,
    pub after: Option<AuditValue>,
}

impl AuditChange {
    fn verify(&self) -> Result<(), AuditError> {
        if self.before.is_none() && self.after.is_none() {
            return Err(AuditError::EmptyChange);
        }
        if let Some(value) = &self.before {
            value.verify()?;
        }
        if let Some(value) = &self.after {
            value.verify()?;
        }
        Ok(())
    }
}

/// Explicit origin beyond ordinary user/service execution.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum AuditOrigin {
    Import {
        import_job_id: Uuid,
        source_system: String,
        source_record: Option<String>,
        mapping_version: u32,
    },
    Repair {
        repair_id: RepairId,
        repaired_device: Option<DeviceId>,
        authority_version: Option<u64>,
    },
    ScopeChange {
        scope_id: SyncScopeId,
        removed_locally_only: bool,
    },
    Bootstrap {
        snapshot_id: Uuid,
        boundary: u64,
    },
}

impl AuditOrigin {
    fn verify(&self) -> Result<(), AuditError> {
        match self {
            Self::Import {
                source_system,
                source_record,
                mapping_version,
                ..
            } => {
                verify_label(source_system)?;
                if let Some(value) = source_record {
                    verify_label(value)?;
                }
                if *mapping_version == 0 {
                    return Err(AuditError::ZeroIdentity);
                }
                Ok(())
            }
            _ => Ok(()),
        }
    }
}

/// Technical lineage and deterministic-decision evidence referenced by a business event.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct AuditProvenance {
    pub operation_id: Option<OperationId>,
    pub authoritative_event_id: Option<EventId>,
    pub caused_by: Option<LineageRef>,
    pub device_id: Option<DeviceId>,
    pub handler_id: Option<u32>,
    pub handler_version: Option<u32>,
    pub policy_version: Option<u32>,
    pub execution_plan_digest: Option<[u8; 32]>,
    pub origin: Option<AuditOrigin>,
}

impl AuditProvenance {
    fn verify(&self) -> Result<(), AuditError> {
        for value in [self.handler_id, self.handler_version, self.policy_version]
            .into_iter()
            .flatten()
        {
            if value == 0 {
                return Err(AuditError::ZeroIdentity);
            }
        }
        if self.execution_plan_digest == Some([0; 32]) {
            return Err(AuditError::ZeroDigest);
        }
        if let Some(origin) = &self.origin {
            origin.verify()?;
        }
        Ok(())
    }
}

/// Structured explanation input; localization maps codes and parameters outside this crate.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct AuditReason {
    pub code: ReasonCode,
    pub parameters: Vec<(AuditFieldId, AuditValue)>,
}

impl AuditReason {
    fn verify(&self) -> Result<(), AuditError> {
        if self.parameters.len() > MAX_AUDIT_PARAMETERS {
            return Err(AuditError::LimitExceeded);
        }
        let mut fields = BTreeSet::new();
        for (field, value) in &self.parameters {
            if !fields.insert(*field) {
                return Err(AuditError::DuplicateField);
            }
            value.verify()?;
        }
        Ok(())
    }
}

/// Immutable canonical business-audit event declared by a domain decision.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct AuditEvent {
    pub format_version: u16,
    pub audit_event_id: AuditEventId,
    pub tenant_id: TenantId,
    pub subject: AuditSubject,
    pub action: AuditActionId,
    pub actor: AuditActor,
    pub occurred_at: AuditTimestamp,
    pub correlation_id: CorrelationId,
    pub outcome: AuditOutcome,
    pub category: AuditCategory,
    pub durability: AuditDurability,
    pub retention: AuditRetentionClass,
    pub changes: Vec<AuditChange>,
    pub reason: Option<AuditReason>,
    pub provenance: AuditProvenance,
    pub corrects: Option<AuditEventId>,
}

impl AuditEvent {
    pub fn verify(&self) -> Result<(), AuditError> {
        if self.format_version != AUDIT_FORMAT_VERSION {
            return Err(AuditError::UnsupportedFormat);
        }
        if self.changes.len() > MAX_AUDIT_CHANGES {
            return Err(AuditError::LimitExceeded);
        }
        self.subject.verify()?;
        self.actor.verify()?;
        self.provenance.verify()?;
        if let Some(reason) = &self.reason {
            reason.verify()?;
        }
        let mut fields = BTreeSet::new();
        for change in &self.changes {
            change.verify()?;
            if !fields.insert(change.field) {
                return Err(AuditError::DuplicateField);
            }
        }
        if self.corrects == Some(self.audit_event_id) {
            return Err(AuditError::SelfCorrection);
        }
        if self.durability == AuditDurability::BestEffort
            && matches!(
                self.category,
                AuditCategory::BusinessChange
                    | AuditCategory::Security
                    | AuditCategory::Administrative
            )
        {
            return Err(AuditError::UnsafeDurability);
        }
        Ok(())
    }

    pub fn digest(&self) -> Result<[u8; 32], AuditError> {
        self.verify()?;
        Ok(*blake3::hash(&postcard::to_stdvec(self)?).as_bytes())
    }

    /// Retry-stable identity for an operation/action/semantic ordinal.
    #[must_use]
    pub fn derive_id(
        operation_id: OperationId,
        action: AuditActionId,
        ordinal: u32,
    ) -> AuditEventId {
        let mut hasher = blake3::Hasher::new();
        hasher.update(b"aequora-audit-event-id-v1");
        hasher.update(operation_id.as_uuid().as_bytes());
        hasher.update(&action.get().to_le_bytes());
        hasher.update(&ordinal.to_le_bytes());
        let mut bytes = [0_u8; 16];
        bytes.copy_from_slice(&hasher.finalize().as_bytes()[..16]);
        AuditEventId::from_uuid(Uuid::from_bytes(bytes))
    }
}

/// Field selection policy registered with an aggregate/action.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct AuditFieldPolicy {
    pub field: AuditFieldId,
    pub value_policy: AuditValuePolicy,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct AuditPolicy {
    pub category: AuditCategory,
    pub durability: AuditDurability,
    pub retention: AuditRetentionClass,
    pub include_rejections: bool,
    pub fields: Vec<AuditFieldPolicy>,
}

impl AuditPolicy {
    pub fn verify(&self) -> Result<(), AuditError> {
        let mut fields = BTreeSet::new();
        for policy in &self.fields {
            if !fields.insert(policy.field) {
                return Err(AuditError::DuplicateField);
            }
        }
        if self.durability == AuditDurability::BestEffort
            && matches!(
                self.category,
                AuditCategory::BusinessChange
                    | AuditCategory::Security
                    | AuditCategory::Administrative
            )
        {
            return Err(AuditError::UnsafeDurability);
        }
        Ok(())
    }

    pub fn allows(&self, change: &AuditChange) -> Result<bool, AuditError> {
        self.verify()?;
        let Some(policy) = self
            .fields
            .iter()
            .find(|policy| policy.field == change.field)
        else {
            return Ok(false);
        };
        Ok(policy.value_policy != AuditValuePolicy::Omit
            && change
                .before
                .as_ref()
                .is_none_or(|value| value.policy() == policy.value_policy)
            && change
                .after
                .as_ref()
                .is_none_or(|value| value.policy() == policy.value_policy))
    }
}

/// Independent tenant/partition hash-chain identity.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
pub enum AuditPartition {
    Business,
    Security,
    Administrative,
    General,
}

/// One immutable event wrapped in a contiguous tamper-evident chain.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ChainedAuditRecord {
    pub partition: AuditPartition,
    pub sequence: AuditSequence,
    pub previous_hash: [u8; 32],
    pub event: AuditEvent,
    pub event_digest: [u8; 32],
    pub current_hash: [u8; 32],
}

impl ChainedAuditRecord {
    pub fn append(
        previous: Option<&Self>,
        partition: AuditPartition,
        event: AuditEvent,
    ) -> Result<Self, AuditError> {
        event.verify()?;
        if let Some(record) = previous {
            if record.partition != partition || record.event.tenant_id != event.tenant_id {
                return Err(AuditError::ChainBoundaryMismatch);
            }
        }
        let sequence =
            AuditSequence(previous.map_or(1, |record| record.sequence.0.saturating_add(1)));
        if sequence.0 == u64::MAX {
            return Err(AuditError::SequenceOverflow);
        }
        let previous_hash = previous.map_or([0; 32], |record| record.current_hash);
        let event_digest = event.digest()?;
        let current_hash = chain_hash(
            event.tenant_id,
            partition,
            sequence,
            previous_hash,
            event_digest,
        )?;
        Ok(Self {
            partition,
            sequence,
            previous_hash,
            event,
            event_digest,
            current_hash,
        })
    }
}

pub fn verify_chain(records: &[ChainedAuditRecord]) -> Result<[u8; 32], AuditError> {
    let Some(first) = records.first() else {
        return Ok([0; 32]);
    };
    let mut previous_hash = [0; 32];
    let mut expected_sequence = 1_u64;
    for record in records {
        if record.event.tenant_id != first.event.tenant_id || record.partition != first.partition {
            return Err(AuditError::ChainBoundaryMismatch);
        }
        if record.sequence.0 != expected_sequence || record.previous_hash != previous_hash {
            return Err(AuditError::ChainDiscontinuity);
        }
        let event_digest = record.event.digest()?;
        if event_digest != record.event_digest
            || chain_hash(
                record.event.tenant_id,
                record.partition,
                record.sequence,
                previous_hash,
                event_digest,
            )? != record.current_hash
        {
            return Err(AuditError::DigestMismatch);
        }
        previous_hash = record.current_hash;
        expected_sequence = expected_sequence
            .checked_add(1)
            .ok_or(AuditError::SequenceOverflow)?;
    }
    Ok(previous_hash)
}

fn chain_hash(
    tenant: TenantId,
    partition: AuditPartition,
    sequence: AuditSequence,
    previous: [u8; 32],
    event: [u8; 32],
) -> Result<[u8; 32], AuditError> {
    Ok(*blake3::hash(&postcard::to_stdvec(&(
        b"aequora-audit-chain-v1",
        tenant,
        partition,
        sequence,
        previous,
        event,
    ))?)
    .as_bytes())
}

/// Unsigned checkpoint suitable for an application-owned signing/immutable-anchor boundary.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct AuditCheckpoint {
    pub authority: AuthorityTimeline,
    pub tenant_id: TenantId,
    pub partition: AuditPartition,
    pub sequence: AuditSequence,
    pub root_hash: [u8; 32],
    pub created_at: AuditTimestamp,
}

impl AuditCheckpoint {
    pub fn verify_against(&self, records: &[ChainedAuditRecord]) -> Result<(), AuditError> {
        let last = records.last().ok_or(AuditError::CheckpointMismatch)?;
        if self.tenant_id != last.event.tenant_id
            || self.partition != last.partition
            || self.sequence != last.sequence
            || self.root_hash != verify_chain(records)?
        {
            return Err(AuditError::CheckpointMismatch);
        }
        Ok(())
    }
}

/// External immutable storage/signing integration. Core never owns anchor credentials.
pub trait AuditAnchorSink {
    type Receipt;
    fn anchor(&mut self, checkpoint: &AuditCheckpoint) -> Result<Self::Receipt, AuditError>;
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum ExplanationLevel {
    Summary,
    ChangeHistory,
    Decision,
    FullLineage,
}

/// Authoritative pointer for the latest committed mutation of a selected field.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct FieldProvenance {
    pub entity: EntityRef,
    pub field: AuditFieldId,
    pub last_audit_event_id: AuditEventId,
    pub last_event_id: EventId,
}

/// Tenant-bounded query filters. `None` means no filter on that dimension.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct AuditPageCursor {
    pub occurred_at: AuditTimestamp,
    pub audit_event_id: AuditEventId,
}

/// Tenant-bounded query filters with an obligatory time window, page cursor, and hard limit.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct AuditQuery {
    pub tenant_id: TenantId,
    pub subject: Option<AuditSubject>,
    pub actor_id: Option<ActorId>,
    pub operation_id: Option<OperationId>,
    pub correlation_id: Option<CorrelationId>,
    pub action: Option<AuditActionId>,
    pub category: Option<AuditCategory>,
    pub occurred_from: Option<AuditTimestamp>,
    pub occurred_through: Option<AuditTimestamp>,
    pub after: Option<AuditPageCursor>,
    pub limit: usize,
}

impl AuditQuery {
    pub fn verify(&self) -> Result<(), AuditError> {
        if self.limit == 0 || self.limit > MAX_QUERY_LIMIT {
            return Err(AuditError::LimitExceeded);
        }
        let (Some(from), Some(through)) = (self.occurred_from, self.occurred_through) else {
            return Err(AuditError::UnboundedTimeRange);
        };
        if from > through {
            return Err(AuditError::InvalidTimeRange);
        }
        if let Some(subject) = &self.subject {
            subject.verify()?;
        }
        Ok(())
    }

    #[must_use]
    pub fn matches(&self, event: &AuditEvent) -> bool {
        event.tenant_id == self.tenant_id
            && self
                .subject
                .as_ref()
                .is_none_or(|value| value == &event.subject)
            && self
                .actor_id
                .is_none_or(|value| event.actor.actor_id() == Some(value))
            && self
                .operation_id
                .is_none_or(|value| event.provenance.operation_id == Some(value))
            && self
                .correlation_id
                .is_none_or(|value| event.correlation_id == value)
            && self.action.is_none_or(|value| event.action == value)
            && self.category.is_none_or(|value| event.category == value)
            && self
                .occurred_from
                .is_none_or(|value| event.occurred_at >= value)
            && self
                .occurred_through
                .is_none_or(|value| event.occurred_at <= value)
            && self.after.as_ref().is_none_or(|cursor| {
                (event.occurred_at, event.audit_event_id)
                    > (cursor.occurred_at, cursor.audit_event_id)
            })
    }
}

/// Authorization already resolved by the host before canonical audit data is disclosed.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AuditAccess {
    TenantAuditor {
        tenant_id: TenantId,
    },
    Subjects {
        tenant_id: TenantId,
        subjects: BTreeSet<AuditSubject>,
    },
}

impl AuditAccess {
    pub fn authorize(&self, query: &AuditQuery) -> Result<(), AuditError> {
        query.verify()?;
        let (tenant_id, subjects) = match self {
            Self::TenantAuditor { tenant_id } => (*tenant_id, None),
            Self::Subjects {
                tenant_id,
                subjects,
            } => (*tenant_id, Some(subjects)),
        };
        if tenant_id != query.tenant_id {
            return Err(AuditError::Forbidden);
        }
        if let Some(subjects) = subjects {
            let requested = query.subject.as_ref().ok_or(AuditError::Forbidden)?;
            if !subjects.contains(requested) {
                return Err(AuditError::Forbidden);
            }
        }
        Ok(())
    }
}

/// Canonical export evidence; presentation formats are derived and non-authoritative.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct AuditExportManifest {
    pub export_id: AuditExportId,
    pub tenant_id: TenantId,
    pub exporter: ActorId,
    pub query_digest: [u8; 32],
    pub created_at: AuditTimestamp,
    pub record_count: u64,
    pub records_digest: [u8; 32],
    pub checkpoint: Option<AuditCheckpoint>,
}

impl AuditExportManifest {
    pub fn verify(&self) -> Result<(), AuditError> {
        if self.query_digest == [0; 32] || self.records_digest == [0; 32] {
            return Err(AuditError::ZeroDigest);
        }
        Ok(())
    }
}

/// Archive/retention decision. Legal holds always override normal purge eligibility.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct AuditRetentionDecision {
    pub archive: bool,
    pub purge_hot_copy: bool,
    pub legal_hold: bool,
}

impl AuditRetentionDecision {
    pub const fn verify(self) -> Result<(), AuditError> {
        if self.legal_hold && self.purge_hot_copy {
            Err(AuditError::HeldRecordPurge)
        } else {
            Ok(())
        }
    }
}

pub trait AuditArchiveSink {
    fn archive(
        &mut self,
        records: &[ChainedAuditRecord],
        checkpoint: &AuditCheckpoint,
    ) -> Result<(), AuditError>;
}

fn verify_label(value: &str) -> Result<(), AuditError> {
    if value.trim().is_empty() || value.len() > MAX_LABEL_BYTES {
        Err(AuditError::InvalidLabel)
    } else {
        Ok(())
    }
}

#[derive(Clone, Debug, Error, Eq, PartialEq)]
pub enum AuditError {
    #[error("audit format is unsupported")]
    UnsupportedFormat,
    #[error("audit identity/version must be non-zero")]
    ZeroIdentity,
    #[error("audit digest must be non-zero")]
    ZeroDigest,
    #[error("audit label is blank or oversized")]
    InvalidLabel,
    #[error("audit field change has neither a before nor after value")]
    EmptyChange,
    #[error("audit field appears more than once")]
    DuplicateField,
    #[error("audit event cannot correct itself")]
    SelfCorrection,
    #[error("audit durability is unsafe for its category")]
    UnsafeDurability,
    #[error("audit hard limit exceeded")]
    LimitExceeded,
    #[error("audit chain crosses tenant or partition boundary")]
    ChainBoundaryMismatch,
    #[error("audit chain sequence or previous hash is discontinuous")]
    ChainDiscontinuity,
    #[error("audit digest does not match canonical content")]
    DigestMismatch,
    #[error("audit sequence overflowed")]
    SequenceOverflow,
    #[error("audit checkpoint does not match the chain")]
    CheckpointMismatch,
    #[error("audit query time range is invalid")]
    InvalidTimeRange,
    #[error("audit query requires an explicit bounded time range")]
    UnboundedTimeRange,
    #[error("audit query is not authorized")]
    Forbidden,
    #[error("legal-held audit evidence cannot be purged")]
    HeldRecordPurge,
    #[error("audit event identity was reused for different canonical content")]
    DuplicateEventIdentity,
    #[error("fault injection interrupted canonical audit commit")]
    InjectedFailure,
    #[error("canonical audit encoding failed: {0}")]
    Encoding(#[from] postcard::Error),
}

#[cfg(test)]
mod tests {
    use super::*;
    use aequora_types::{EntityId, EntityType};
    use proptest::prelude::*;

    fn event(operation_id: OperationId, ordinal: u32) -> AuditEvent {
        let action = AuditActionId::new(7).unwrap_or_else(|error| panic!("{error}"));
        AuditEvent {
            format_version: AUDIT_FORMAT_VERSION,
            audit_event_id: AuditEvent::derive_id(operation_id, action, ordinal),
            tenant_id: TenantId::new(),
            subject: AuditSubject::Entity(EntityRef {
                entity_type: EntityType::new(1).unwrap_or_else(|error| panic!("{error}")),
                entity_id: EntityId::new(),
            }),
            action,
            actor: AuditActor::System {
                component_id: "billing".to_owned(),
            },
            occurred_at: AuditTimestamp(100),
            correlation_id: CorrelationId::new(),
            outcome: AuditOutcome::Accepted,
            category: AuditCategory::BusinessChange,
            durability: AuditDurability::RequiredAtomic,
            retention: AuditRetentionClass::LongTerm,
            changes: vec![AuditChange {
                field: AuditFieldId::new(1).unwrap_or_else(|error| panic!("{error}")),
                kind: AuditChangeKind::Set,
                before: Some(AuditValue::Digest([1; 32])),
                after: Some(AuditValue::Digest([2; 32])),
            }],
            reason: None,
            provenance: AuditProvenance {
                operation_id: Some(operation_id),
                ..AuditProvenance::default()
            },
            corrects: None,
        }
    }

    #[test]
    fn deterministic_identity_and_chain_detect_tampering() -> Result<(), AuditError> {
        let operation_id = OperationId::new();
        assert_eq!(
            event(operation_id, 0).audit_event_id,
            event(operation_id, 0).audit_event_id
        );
        let first =
            ChainedAuditRecord::append(None, AuditPartition::Business, event(operation_id, 0))?;
        let mut second_event = event(OperationId::new(), 0);
        second_event.tenant_id = first.event.tenant_id;
        let second =
            ChainedAuditRecord::append(Some(&first), AuditPartition::Business, second_event)?;
        let mut records = vec![first, second];
        assert_ne!(verify_chain(&records)?, [0; 32]);
        records[1].event.outcome = AuditOutcome::Rejected;
        assert_eq!(verify_chain(&records), Err(AuditError::DigestMismatch));
        Ok(())
    }

    #[test]
    fn subject_authorization_is_tenant_bounded_and_fail_closed() -> Result<(), AuditError> {
        let event = event(OperationId::new(), 0);
        let query = AuditQuery {
            tenant_id: event.tenant_id,
            subject: Some(event.subject.clone()),
            actor_id: None,
            operation_id: None,
            correlation_id: None,
            action: None,
            category: None,
            occurred_from: Some(AuditTimestamp(0)),
            occurred_through: Some(AuditTimestamp(u64::MAX)),
            after: None,
            limit: 10,
        };
        let access = AuditAccess::Subjects {
            tenant_id: event.tenant_id,
            subjects: BTreeSet::from([event.subject]),
        };
        access.authorize(&query)?;
        let mut other = query;
        other.tenant_id = TenantId::new();
        assert_eq!(access.authorize(&other), Err(AuditError::Forbidden));
        Ok(())
    }

    #[test]
    fn sensitive_policy_rejects_raw_values_and_holds_block_purge() -> Result<(), AuditError> {
        let policy = AuditPolicy {
            category: AuditCategory::Security,
            durability: AuditDurability::RequiredAtomic,
            retention: AuditRetentionClass::LegalHoldEligible,
            include_rejections: true,
            fields: vec![AuditFieldPolicy {
                field: AuditFieldId::new(1)?,
                value_policy: AuditValuePolicy::Hashed,
            }],
        };
        let change = AuditChange {
            field: AuditFieldId::new(1)?,
            kind: AuditChangeKind::Set,
            before: None,
            after: Some(AuditValue::Full(b"secret".to_vec())),
        };
        assert!(!policy.allows(&change)?);
        assert_eq!(
            AuditRetentionDecision {
                archive: true,
                purge_hot_copy: true,
                legal_hold: true
            }
            .verify(),
            Err(AuditError::HeldRecordPurge)
        );
        Ok(())
    }

    proptest! {
        #[test]
        fn derived_ids_are_retry_stable(ordinal in any::<u32>()) {
            let operation = OperationId::from_uuid(Uuid::from_bytes([9; 16]));
            let action = AuditActionId::new(9).unwrap_or_else(|error| panic!("{error}"));
            prop_assert_eq!(AuditEvent::derive_id(operation, action, ordinal), AuditEvent::derive_id(operation, action, ordinal));
        }
    }
}
