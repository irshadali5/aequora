//! Database-neutral data lifecycle, retention, legal-hold, erasure, and purge contracts.
//!
//! This crate plans and verifies destructive transitions. It never directly deletes application
//! data, chooses jurisdiction-specific durations, owns encryption keys, or claims that an offline
//! uncontrolled device was physically erased.

#![allow(clippy::missing_errors_doc)]

use aequora_types::{ActorId, CorrelationId, DeviceId, EntityRef, Sequence, SyncScopeId, TenantId};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use thiserror::Error;
use uuid::Uuid;

pub const GOVERNANCE_FORMAT_VERSION: u16 = 1;
pub const MAX_GRAPH_NODES: usize = 100_000;
pub const MAX_PLAN_ACTIONS: usize = 100_000;
pub const MAX_SURFACES: usize = 256;
const MAX_TEXT_BYTES: usize = 512;

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

uuid_id!(LegalHoldId, "Stable legal-hold identity.");
uuid_id!(ErasureRequestId, "Stable subject-erasure request identity.");
uuid_id!(PurgeId, "Stable destructive plan identity.");
uuid_id!(
    PurgeDirectiveId,
    "Stable server-issued client purge identity."
);

#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct RetentionClassId(u16);
impl RetentionClassId {
    pub const fn new(value: u16) -> Result<Self, GovernanceError> {
        if value == 0 {
            Err(GovernanceError::ZeroIdentity)
        } else {
            Ok(Self(value))
        }
    }
    #[must_use]
    pub const fn get(self) -> u16 {
        self.0
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct RetentionPolicyVersion(u32);
impl RetentionPolicyVersion {
    pub const fn new(value: u32) -> Result<Self, GovernanceError> {
        if value == 0 {
            Err(GovernanceError::ZeroIdentity)
        } else {
            Ok(Self(value))
        }
    }
    #[must_use]
    pub const fn get(self) -> u32 {
        self.0
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct GovernancePolicyGeneration(u32);
impl GovernancePolicyGeneration {
    pub const fn new(value: u32) -> Result<Self, GovernanceError> {
        if value == 0 {
            Err(GovernanceError::ZeroIdentity)
        } else {
            Ok(Self(value))
        }
    }
    #[must_use]
    pub const fn get(self) -> u32 {
        self.0
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct StorageSurfaceId(u16);
impl StorageSurfaceId {
    pub const fn new(value: u16) -> Result<Self, GovernanceError> {
        if value == 0 {
            Err(GovernanceError::ZeroIdentity)
        } else {
            Ok(Self(value))
        }
    }
    #[must_use]
    pub const fn get(self) -> u16 {
        self.0
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
pub enum GovernedDataClass {
    AuthoritativeBusinessData,
    JournalHistory,
    OperationLedger,
    AuditTrail,
    ConflictRecord,
    Tombstone,
    Snapshot,
    ReplayArtifact,
    ImportArtifact,
    RepairArtifact,
    Blob,
    ClientReplica,
    DerivedCache,
    Export,
    ColdArchive,
    Backup,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum DeletionMode {
    HardDeleteWhenSafe,
    TombstoneThenGc,
    Pseudonymize,
    CryptographicErase,
    PermanentByPolicy,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct DurationPolicy {
    pub millis: u64,
}
impl DurationPolicy {
    pub const fn new(millis: u64) -> Result<Self, GovernanceError> {
        if millis == 0 {
            Err(GovernanceError::ZeroDuration)
        } else {
            Ok(Self { millis })
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct RetentionPolicy {
    pub class: RetentionClassId,
    pub version: RetentionPolicyVersion,
    pub minimum_retention: DurationPolicy,
    pub maximum_retention: Option<DurationPolicy>,
    pub deletion_mode: DeletionMode,
    pub legal_hold_eligible: bool,
    pub archive_before_delete: bool,
}
impl RetentionPolicy {
    pub fn verify(&self) -> Result<(), GovernanceError> {
        if self
            .maximum_retention
            .is_some_and(|maximum| maximum.millis < self.minimum_retention.millis)
        {
            return Err(GovernanceError::InvalidRetentionRange);
        }
        if self.deletion_mode == DeletionMode::PermanentByPolicy && self.maximum_retention.is_some()
        {
            return Err(GovernanceError::PermanentHasMaximum);
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum LifecycleState {
    Active,
    SoftDeleted,
    Tombstoned,
    Archived,
    Held,
    ErasurePending,
    Purged,
}

#[derive(Clone, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
pub enum DataSubjectRef {
    Principal(ActorId),
    Entity(EntityRef),
    Pseudonymous([u8; 32]),
    Custom(String),
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum HoldSelector {
    Tenant,
    Subject(DataSubjectRef),
    Entity(EntityRef),
    DataClass(GovernedDataClass),
    TimeRange {
        from_unix_ms: u64,
        through_unix_ms: u64,
    },
    LegalMatter(String),
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum HoldState {
    Active,
    Released,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct LegalHold {
    pub hold_id: LegalHoldId,
    pub tenant_id: TenantId,
    pub selector: HoldSelector,
    pub reason_code: u32,
    pub created_at_unix_ms: u64,
    pub created_by: ActorId,
    pub state: HoldState,
    pub released_at_unix_ms: Option<u64>,
    pub released_by: Option<ActorId>,
}
impl LegalHold {
    pub fn verify(&self) -> Result<(), GovernanceError> {
        if self.reason_code == 0 {
            return Err(GovernanceError::ZeroIdentity);
        }
        verify_selector(&self.selector)?;
        match self.state {
            HoldState::Active
                if self.released_at_unix_ms.is_some() || self.released_by.is_some() =>
            {
                Err(GovernanceError::InvalidHoldTransition)
            }
            HoldState::Released
                if self.released_at_unix_ms.is_none() || self.released_by.is_none() =>
            {
                Err(GovernanceError::InvalidHoldTransition)
            }
            _ => Ok(()),
        }
    }
    #[must_use]
    pub const fn blocks_purge(&self) -> bool {
        matches!(self.state, HoldState::Active)
    }
}

fn verify_selector(selector: &HoldSelector) -> Result<(), GovernanceError> {
    match selector {
        HoldSelector::TimeRange {
            from_unix_ms,
            through_unix_ms,
        } if from_unix_ms > through_unix_ms => Err(GovernanceError::InvalidTimeRange),
        HoldSelector::LegalMatter(value) | HoldSelector::Subject(DataSubjectRef::Custom(value)) => {
            verify_text(value)
        }
        _ => Ok(()),
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum GovernanceRelation {
    Owned,
    Referenced,
    Shared,
    Derived,
    AuditOnly,
}

#[derive(Clone, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
pub enum GovernedObjectRef {
    Entity(EntityRef),
    Blob(String),
    AuditEvent(Uuid),
    GovernedCopy(String),
    Credential(String),
    Scope(SyncScopeId),
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct SubjectGraphNode {
    pub object: GovernedObjectRef,
    pub relation: GovernanceRelation,
    pub retention_class: RetentionClassId,
    pub fields: Vec<u32>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct DataSubjectGraph {
    pub subject: DataSubjectRef,
    pub nodes: Vec<SubjectGraphNode>,
}
impl DataSubjectGraph {
    pub fn verify(&self) -> Result<(), GovernanceError> {
        if self.nodes.len() > MAX_GRAPH_NODES {
            return Err(GovernanceError::LimitExceeded);
        }
        let mut objects = BTreeSet::new();
        for node in &self.nodes {
            if !objects.insert(&node.object) {
                return Err(GovernanceError::DuplicateObject);
            }
            match &node.object {
                GovernedObjectRef::Blob(value)
                | GovernedObjectRef::GovernedCopy(value)
                | GovernedObjectRef::Credential(value) => verify_text(value)?,
                _ => {}
            }
        }
        Ok(())
    }
}

pub trait DataSubjectResolver {
    fn resolve(
        &self,
        tenant: TenantId,
        subject: &DataSubjectRef,
    ) -> Result<DataSubjectGraph, GovernanceError>;
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum FieldErasureMode {
    Null,
    ReplaceWithPseudonym,
    Hash,
    KeepRequired,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct FieldGovernancePolicy {
    pub field_id: u32,
    pub retention_class: RetentionClassId,
    pub erasure: FieldErasureMode,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum ErasureBlocker {
    LegalHold,
    RequiredRetention,
    OpenDispute,
    PendingExport,
    ActiveMigration,
    SharedReference,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum ErasureActionKind {
    DeleteEntity(EntityRef),
    PseudonymizeFields { entity: EntityRef, fields: Vec<u32> },
    RemoveBlob(String),
    RevokeScope(SyncScopeId),
    PurgeGovernedCopy(String),
    CompactAuditIdentity,
    RotateEncryptionKey(String),
    RetainMinimizedEvidence,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ErasureAction {
    pub action_id: [u8; 32],
    pub kind: ErasureActionKind,
    pub irreversible: bool,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ErasurePlan {
    pub format_version: u16,
    pub request_id: ErasureRequestId,
    pub tenant_id: TenantId,
    pub subject: DataSubjectRef,
    pub policy_version: RetentionPolicyVersion,
    pub correlation_id: CorrelationId,
    pub actions: Vec<ErasureAction>,
    pub blockers: Vec<ErasureBlocker>,
    pub digest: [u8; 32],
}
impl ErasurePlan {
    pub fn build(
        request_id: ErasureRequestId,
        tenant_id: TenantId,
        subject: DataSubjectRef,
        policy_version: RetentionPolicyVersion,
        correlation_id: CorrelationId,
        actions: Vec<ErasureAction>,
        blockers: Vec<ErasureBlocker>,
    ) -> Result<Self, GovernanceError> {
        let mut plan = Self {
            format_version: GOVERNANCE_FORMAT_VERSION,
            request_id,
            tenant_id,
            subject,
            policy_version,
            correlation_id,
            actions,
            blockers,
            digest: [0; 32],
        };
        plan.verify_fields()?;
        plan.digest = plan.calculate_digest()?;
        Ok(plan)
    }
    pub fn verify(&self) -> Result<(), GovernanceError> {
        self.verify_fields()?;
        if self.calculate_digest()? != self.digest {
            return Err(GovernanceError::DigestMismatch);
        }
        Ok(())
    }
    #[must_use]
    pub fn executable(&self) -> bool {
        self.blockers.is_empty()
    }
    fn verify_fields(&self) -> Result<(), GovernanceError> {
        if self.format_version != GOVERNANCE_FORMAT_VERSION {
            return Err(GovernanceError::UnsupportedFormat);
        }
        if self.actions.len() > MAX_PLAN_ACTIONS {
            return Err(GovernanceError::LimitExceeded);
        }
        let mut ids = BTreeSet::new();
        for action in &self.actions {
            if action.action_id == [0; 32] || !ids.insert(action.action_id) {
                return Err(GovernanceError::DuplicateAction);
            }
            match &action.kind {
                ErasureActionKind::PseudonymizeFields { fields, .. } if fields.is_empty() => {
                    return Err(GovernanceError::EmptyAction);
                }
                ErasureActionKind::RemoveBlob(value)
                | ErasureActionKind::PurgeGovernedCopy(value)
                | ErasureActionKind::RotateEncryptionKey(value) => verify_text(value)?,
                _ => {}
            }
        }
        Ok(())
    }
    fn calculate_digest(&self) -> Result<[u8; 32], GovernanceError> {
        Ok(*blake3::hash(&postcard::to_stdvec(&(
            self.format_version,
            self.request_id,
            self.tenant_id,
            &self.subject,
            self.policy_version,
            self.correlation_id,
            &self.actions,
            &self.blockers,
        ))?)
        .as_bytes())
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum DeviceLifecycle {
    Active,
    RequiresRebootstrap,
    Retired,
    Revoked,
    Expired,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct DeviceWatermark {
    pub device_id: DeviceId,
    pub acknowledged: Sequence,
    pub lifecycle: DeviceLifecycle,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct JournalFloor {
    pub scope_id: SyncScopeId,
    pub scope_generation: u64,
    pub minimum_resumable: Sequence,
    pub bootstrap_available: bool,
}
impl JournalFloor {
    #[must_use]
    pub const fn cursor_valid(self, cursor: Sequence, generation: u64) -> bool {
        generation == self.scope_generation && cursor.0 >= self.minimum_resumable.0
    }
    pub const fn verify(self) -> Result<(), GovernanceError> {
        if self.scope_generation == 0 || !self.bootstrap_available {
            Err(GovernanceError::UnsafeJournalFloor)
        } else {
            Ok(())
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct TombstoneGcCandidate {
    pub entity: EntityRef,
    pub deletion_sequence: Sequence,
    pub retained_identity_guard: bool,
    pub held: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TombstoneGcDecision {
    Eligible,
    RetainForClient,
    RetainForHold,
    RetainForIdentityGuard,
    RebootstrapUnavailable,
}

pub fn evaluate_tombstone_gc(
    candidate: &TombstoneGcCandidate,
    devices: &[DeviceWatermark],
    floor: JournalFloor,
) -> Result<TombstoneGcDecision, GovernanceError> {
    floor.verify()?;
    if candidate.held {
        return Ok(TombstoneGcDecision::RetainForHold);
    }
    if !candidate.retained_identity_guard {
        return Ok(TombstoneGcDecision::RetainForIdentityGuard);
    }
    if !floor.bootstrap_available {
        return Ok(TombstoneGcDecision::RebootstrapUnavailable);
    }
    for device in devices {
        if device.lifecycle == DeviceLifecycle::Active
            && device.acknowledged.0 <= candidate.deletion_sequence.0
        {
            return Ok(TombstoneGcDecision::RetainForClient);
        }
    }
    Ok(TombstoneGcDecision::Eligible)
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct OperationLedgerPolicy {
    pub minimum_retry_window_ms: u64,
    pub compact_after_ms: u64,
    pub reject_before_unix_ms: u64,
}
impl OperationLedgerPolicy {
    pub const fn verify(self) -> Result<(), GovernanceError> {
        if self.minimum_retry_window_ms == 0 || self.compact_after_ms < self.minimum_retry_window_ms
        {
            Err(GovernanceError::UnsafeLedgerHorizon)
        } else {
            Ok(())
        }
    }
    #[must_use]
    pub const fn retry_supported(self, created_at_unix_ms: u64) -> bool {
        created_at_unix_ms >= self.reject_before_unix_ms
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
pub enum GovernedCopyKind {
    Snapshot,
    Blob,
    Export,
    ReplayBundle,
    ImportSource,
    RepairBundle,
    ColdArchive,
    Backup,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct GovernedCopyRef {
    pub kind: GovernedCopyKind,
    pub opaque_storage_ref: String,
    pub retention_class: RetentionClassId,
    pub surface: StorageSurfaceId,
    pub subject_digest: Option<[u8; 32]>,
}
impl GovernedCopyRef {
    pub fn verify(&self) -> Result<(), GovernanceError> {
        verify_text(&self.opaque_storage_ref)?;
        if self.subject_digest == Some([0; 32]) {
            return Err(GovernanceError::ZeroDigest);
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum TenantLifecycle {
    Active,
    Suspended,
    ReadOnly,
    ExportPending,
    DeletionScheduled,
    Held,
    Purging,
    Purged,
}
impl TenantLifecycle {
    #[must_use]
    pub const fn allows_authoritative_writes(self) -> bool {
        matches!(self, Self::Active)
    }
    pub const fn transition(self, next: Self) -> Result<Self, GovernanceError> {
        let valid = matches!(
            (self, next),
            (Self::Active, Self::Suspended | Self::ReadOnly)
                | (Self::Suspended, Self::ReadOnly | Self::Held)
                | (
                    Self::ReadOnly,
                    Self::ExportPending | Self::DeletionScheduled | Self::Held
                )
                | (Self::ExportPending, Self::DeletionScheduled | Self::Held)
                | (Self::DeletionScheduled, Self::Purging | Self::Held)
                | (Self::Held, Self::DeletionScheduled)
                | (Self::Purging, Self::Purged)
        );
        if valid {
            Ok(next)
        } else {
            Err(GovernanceError::InvalidTenantTransition)
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum ApprovalState {
    PendingApproval,
    Approved,
    Rejected,
}
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum GovernanceJobState {
    Requested,
    Planned,
    AwaitingApproval,
    Executing,
    Verifying,
    Completed,
    Blocked,
    Failed,
    PartiallyCompleted,
    Canceled,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct PurgePlan {
    pub purge_id: PurgeId,
    pub tenant_id: TenantId,
    pub policy_version: RetentionPolicyVersion,
    pub correlation_id: CorrelationId,
    /// Identity of the canonical administrative audit event authorizing this plan.
    pub administrative_audit_event_id: Uuid,
    pub dry_run: bool,
    pub requested_by: ActorId,
    pub approved_by: Option<ActorId>,
    pub approval: ApprovalState,
    pub required_surfaces: BTreeSet<StorageSurfaceId>,
    pub actions_digest: [u8; 32],
}
impl PurgePlan {
    pub fn verify(&self) -> Result<(), GovernanceError> {
        if self.required_surfaces.is_empty()
            || self.required_surfaces.len() > MAX_SURFACES
            || self.actions_digest == [0; 32]
            || self.administrative_audit_event_id.is_nil()
        {
            return Err(GovernanceError::InvalidPurgePlan);
        }
        if self.approval == ApprovalState::Approved && self.approved_by.is_none() {
            return Err(GovernanceError::ApprovalMissing);
        }
        if self.approved_by == Some(self.requested_by) {
            return Err(GovernanceError::SeparationOfDuties);
        }
        Ok(())
    }
    #[must_use]
    pub fn may_execute(&self) -> bool {
        !self.dry_run && self.approval == ApprovalState::Approved && self.verify().is_ok()
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
pub enum GovernanceCapability {
    HardDelete,
    TransactionalBatchDelete,
    GenerationDrop,
    CryptographicErase,
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct GovernanceCapabilities(pub BTreeSet<GovernanceCapability>);

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct StorageSurface {
    pub id: StorageSurfaceId,
    pub required: bool,
    pub data_classes: BTreeSet<GovernedDataClass>,
    pub capabilities: GovernanceCapabilities,
}

#[derive(Clone, Debug, Default)]
pub struct GovernanceRegistry {
    surfaces: BTreeMap<StorageSurfaceId, StorageSurface>,
}
impl GovernanceRegistry {
    pub fn register(&mut self, surface: StorageSurface) -> Result<(), GovernanceError> {
        if self.surfaces.len() >= MAX_SURFACES {
            return Err(GovernanceError::LimitExceeded);
        }
        if surface.data_classes.is_empty() || self.surfaces.insert(surface.id, surface).is_some() {
            return Err(GovernanceError::DuplicateSurface);
        }
        Ok(())
    }
    #[must_use]
    pub fn required(&self) -> BTreeSet<StorageSurfaceId> {
        self.surfaces
            .values()
            .filter(|surface| surface.required)
            .map(|surface| surface.id)
            .collect()
    }
    pub fn verify_coverage(&self, copies: &[GovernedCopyRef]) -> Result<(), GovernanceError> {
        for copy in copies {
            copy.verify()?;
            if !self.surfaces.contains_key(&copy.surface) {
                return Err(GovernanceError::UnregisteredSurface);
            }
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum SurfaceOutcome {
    Planned,
    Executed,
    Verified,
    Failed,
    Unreachable,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct StoreVerification {
    pub surface: StorageSurfaceId,
    pub outcome: SurfaceOutcome,
    pub remaining_items: u64,
    pub digest: [u8; 32],
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct PurgeVerificationReport {
    pub purge_id: PurgeId,
    pub completed_at_unix_ms: u64,
    pub stores: Vec<StoreVerification>,
    pub retained_exceptions: u64,
}
impl PurgeVerificationReport {
    pub fn state(
        &self,
        required: &BTreeSet<StorageSurfaceId>,
    ) -> Result<GovernanceJobState, GovernanceError> {
        let by_id = self
            .stores
            .iter()
            .map(|item| (item.surface, item))
            .collect::<BTreeMap<_, _>>();
        let mut partial = false;
        for surface in required {
            let Some(result) = by_id.get(surface) else {
                return Ok(GovernanceJobState::PartiallyCompleted);
            };
            if result.outcome != SurfaceOutcome::Verified
                || result.remaining_items != 0
                || result.digest == [0; 32]
            {
                partial = true;
            }
        }
        Ok(if partial {
            GovernanceJobState::PartiallyCompleted
        } else {
            GovernanceJobState::Completed
        })
    }
}

pub trait GovernanceStore {
    fn surface(&self) -> StorageSurfaceId;
    fn plan(&self, plan: &PurgePlan) -> Result<[u8; 32], GovernanceError>;
    fn execute(&mut self, plan: &PurgePlan) -> Result<(), GovernanceError>;
    fn verify(&self, plan: &PurgePlan) -> Result<StoreVerification, GovernanceError>;
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum PurgeReason {
    ScopeRevoked,
    TenantOffboarded,
    RetentionExpired,
    ErasureApproved,
    CredentialRevoked,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct PurgeDirective {
    pub directive_id: PurgeDirectiveId,
    pub tenant_id: TenantId,
    pub scope_id: SyncScopeId,
    pub reason: PurgeReason,
    pub policy_generation: GovernancePolicyGeneration,
    pub minimum_store_generation: Option<u64>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum LocalPurgeState {
    Planned,
    WaitingForSafePoint,
    Purging,
    RebuildingIndexes,
    Complete,
    Restricted,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct PurgeAck {
    pub directive_id: PurgeDirectiveId,
    pub device_id: DeviceId,
    pub completed_at_unix_ms: u64,
    pub resulting_store_generation: u64,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ErasureLedgerEntry {
    pub request_id: ErasureRequestId,
    pub subject_digest: [u8; 32],
    pub completed_at_unix_ms: u64,
    pub policy_version: RetentionPolicyVersion,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct RestoreGovernanceGate {
    pub restored_at_unix_ms: u64,
    pub erasures_required: u64,
    pub erasures_applied: u64,
    pub revocations_required: u64,
    pub revocations_applied: u64,
    pub holds_loaded: bool,
    pub verified: bool,
}
impl RestoreGovernanceGate {
    #[must_use]
    pub const fn may_serve(self) -> bool {
        self.holds_loaded
            && self.verified
            && self.erasures_applied == self.erasures_required
            && self.revocations_applied == self.revocations_required
    }
}

fn verify_text(value: &str) -> Result<(), GovernanceError> {
    if value.trim().is_empty() || value.len() > MAX_TEXT_BYTES {
        Err(GovernanceError::InvalidText)
    } else {
        Ok(())
    }
}

#[derive(Clone, Debug, Error, Eq, PartialEq)]
pub enum GovernanceError {
    #[error("governance identity/version must be non-zero")]
    ZeroIdentity,
    #[error("retention duration must be non-zero")]
    ZeroDuration,
    #[error("retention minimum exceeds maximum")]
    InvalidRetentionRange,
    #[error("permanent policy cannot declare a maximum retention")]
    PermanentHasMaximum,
    #[error("governance text is blank or oversized")]
    InvalidText,
    #[error("hold time range is invalid")]
    InvalidTimeRange,
    #[error("legal-hold state metadata is inconsistent")]
    InvalidHoldTransition,
    #[error("governance hard limit exceeded")]
    LimitExceeded,
    #[error("subject graph contains duplicate governed object")]
    DuplicateObject,
    #[error("governance action is empty")]
    EmptyAction,
    #[error("governance action identity is zero or duplicated")]
    DuplicateAction,
    #[error("governance artifact format is unsupported")]
    UnsupportedFormat,
    #[error("governance artifact digest mismatch")]
    DigestMismatch,
    #[error("governance digest must be non-zero")]
    ZeroDigest,
    #[error("journal floor lacks generation or bootstrap recovery")]
    UnsafeJournalFloor,
    #[error("operation-ledger horizon is shorter than legitimate retry window")]
    UnsafeLedgerHorizon,
    #[error("tenant lifecycle transition is invalid")]
    InvalidTenantTransition,
    #[error("purge plan is incomplete or unbounded")]
    InvalidPurgePlan,
    #[error("approved purge lacks approver")]
    ApprovalMissing,
    #[error("requester cannot approve the same destructive purge")]
    SeparationOfDuties,
    #[error("governance storage surface is duplicated")]
    DuplicateSurface,
    #[error("governed copy references an unregistered storage surface")]
    UnregisteredSurface,
    #[error("canonical governance encoding failed: {0}")]
    Encoding(#[from] postcard::Error),
    #[error("fault injection interrupted governance work")]
    InjectedFailure,
}

#[cfg(test)]
mod tests {
    use super::*;
    use aequora_types::{EntityId, EntityType};
    use proptest::prelude::*;

    fn entity() -> EntityRef {
        EntityRef {
            entity_type: EntityType::new(1).unwrap_or_else(|error| panic!("{error}")),
            entity_id: EntityId::new(),
        }
    }

    #[test]
    fn tombstone_waits_for_active_client_then_retired_client_cannot_resume()
    -> Result<(), GovernanceError> {
        let floor = JournalFloor {
            scope_id: SyncScopeId::new(),
            scope_generation: 2,
            minimum_resumable: Sequence(11),
            bootstrap_available: true,
        };
        let candidate = TombstoneGcCandidate {
            entity: entity(),
            deletion_sequence: Sequence(10),
            retained_identity_guard: true,
            held: false,
        };
        let mut device = DeviceWatermark {
            device_id: DeviceId::new(),
            acknowledged: Sequence(9),
            lifecycle: DeviceLifecycle::Active,
        };
        assert_eq!(
            evaluate_tombstone_gc(&candidate, &[device], floor)?,
            TombstoneGcDecision::RetainForClient
        );
        device.lifecycle = DeviceLifecycle::Retired;
        assert_eq!(
            evaluate_tombstone_gc(&candidate, &[device], floor)?,
            TombstoneGcDecision::Eligible
        );
        assert!(!floor.cursor_valid(Sequence(9), 2));
        Ok(())
    }

    #[test]
    fn legal_hold_and_restore_gate_fail_closed() -> Result<(), GovernanceError> {
        let hold = LegalHold {
            hold_id: LegalHoldId::new(),
            tenant_id: TenantId::new(),
            selector: HoldSelector::Tenant,
            reason_code: 1,
            created_at_unix_ms: 1,
            created_by: ActorId::new(),
            state: HoldState::Active,
            released_at_unix_ms: None,
            released_by: None,
        };
        hold.verify()?;
        assert!(hold.blocks_purge());
        let gate = RestoreGovernanceGate {
            restored_at_unix_ms: 10,
            erasures_required: 2,
            erasures_applied: 1,
            revocations_required: 1,
            revocations_applied: 1,
            holds_loaded: true,
            verified: true,
        };
        assert!(!gate.may_serve());
        Ok(())
    }

    #[test]
    fn purge_completion_requires_every_registered_surface() -> Result<(), GovernanceError> {
        let one = StorageSurfaceId::new(1)?;
        let two = StorageSurfaceId::new(2)?;
        let report = PurgeVerificationReport {
            purge_id: PurgeId::new(),
            completed_at_unix_ms: 3,
            stores: vec![StoreVerification {
                surface: one,
                outcome: SurfaceOutcome::Verified,
                remaining_items: 0,
                digest: [1; 32],
            }],
            retained_exceptions: 0,
        };
        assert_eq!(
            report.state(&BTreeSet::from([one, two]))?,
            GovernanceJobState::PartiallyCompleted
        );
        Ok(())
    }

    proptest! {
        #[test]
        fn active_client_at_or_before_delete_always_pins(client in 0_u64..1_000, deletion in 0_u64..1_000) {
            let floor = JournalFloor { scope_id: SyncScopeId::new(), scope_generation: 1, minimum_resumable: Sequence(deletion.saturating_add(1)), bootstrap_available: true };
            let candidate = TombstoneGcCandidate { entity: entity(), deletion_sequence: Sequence(deletion), retained_identity_guard: true, held: false };
            let device = DeviceWatermark { device_id: DeviceId::new(), acknowledged: Sequence(client), lifecycle: DeviceLifecycle::Active };
            let result = evaluate_tombstone_gc(&candidate, &[device], floor);
            if client <= deletion { prop_assert_eq!(result, Ok(TombstoneGcDecision::RetainForClient)); }
        }
    }
}
