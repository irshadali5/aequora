//! Typed logical records shared by database adapters.

use crate::{
    LocalOperationSeq, LocalStoreGeneration, MetadataMigrationId, MetadataSchemaVersion,
    PersistenceError, ProcessInstanceId, ProjectionVersion, ScopeGeneration, ScopeVersion, StoreId,
    Timestamp, payload_digest,
};
use aequora_protocol::{ChangeKind, OperationKind};
use aequora_types::{
    ActorId, AuthorityEpoch, AuthorityId, AuthorityInstanceId, AuthorityTransitionId,
    CorrelationId, DeviceId, EntityRef, EntityVersion, EventId, JobId, LineageRef, OperationId,
    ProtocolVersion, RepairId, SchemaVersion, Sequence, SnapshotId, SyncScopeId, TenantId,
};
use serde::{Deserialize, Serialize};
use std::fmt;
use uuid::Uuid;

macro_rules! uuid_id {
    ($(#[$meta:meta])* $name:ident) => {
        $(#[$meta])*
        #[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
        #[serde(transparent)]
        pub struct $name(Uuid);
        impl $name {
            #[must_use]
            pub fn new() -> Self { Self(Uuid::now_v7()) }
            #[must_use]
            pub const fn from_uuid(value: Uuid) -> Self { Self(value) }
            #[must_use]
            pub const fn as_uuid(self) -> Uuid { self.0 }
        }
        impl Default for $name { fn default() -> Self { Self::new() } }
        impl fmt::Display for $name {
            fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                self.0.fmt(formatter)
            }
        }
    };
}

uuid_id!(/// Durable subscription identity.
    SubscriptionId);
uuid_id!(/// Durable local conflict identity.
    ConflictId);
uuid_id!(/// Durable bootstrap attempt identity.
    BootstrapJobId);
uuid_id!(/// Durable snapshot lease identity.
    SnapshotLeaseId);
uuid_id!(/// Import workflow identity.
    ImportJobId);
uuid_id!(/// Export workflow identity.
    ExportJobId);
uuid_id!(/// Replay artifact identity.
    ReplayArtifactId);
uuid_id!(/// Legal hold identity.
    LegalHoldId);
uuid_id!(/// Erasure request identity.
    ErasureRequestId);
uuid_id!(/// Purge workflow identity.
    PurgeJobId);
uuid_id!(/// Client purge directive identity.
    PurgeDirectiveId);
uuid_id!(/// Durable side-effect intent identity.
    SideEffectIntentId);
uuid_id!(/// Opaque public key identity; private material is never represented here.
    KeyId);
uuid_id!(/// Stable metadata storage-surface identity.
    StorageSurfaceId);
uuid_id!(/// Replica identity for regional control-plane watermarks.
    ReplicaId);
uuid_id!(/// Worker identity for durable jobs.
    WorkerId);

/// Stable semantic digest used for immutable payload comparison.
pub type Digest = [u8; 32];

/// Durable metadata available on every local store.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct LocalStoreMetadata {
    pub local_store_id: StoreId,
    pub metadata_schema_version: MetadataSchemaVersion,
    pub store_generation: LocalStoreGeneration,
    pub device_id: DeviceId,
    pub created_at: Timestamp,
    pub last_opened_at: Timestamp,
}

/// Stable numeric state IDs are encoded explicitly by adapters, never from Rust discriminants.
pub trait PersistentState {
    fn persistent_id(self) -> u16;
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
pub enum OutboxState {
    Pending,
    InFlight,
    Retryable,
    Blocked,
    Conflict,
    Committed,
    Rejected,
    Superseded,
}

impl PersistentState for OutboxState {
    fn persistent_id(self) -> u16 {
        match self {
            Self::Pending => 1,
            Self::InFlight => 2,
            Self::Retryable => 3,
            Self::Blocked => 4,
            Self::Conflict => 5,
            Self::Committed => 6,
            Self::Rejected => 7,
            Self::Superseded => 8,
        }
    }
}

impl OutboxState {
    #[must_use]
    pub const fn is_claimable(self) -> bool {
        matches!(self, Self::Pending | Self::Retryable)
    }

    #[must_use]
    pub const fn is_terminal(self) -> bool {
        matches!(self, Self::Committed | Self::Rejected | Self::Superseded)
    }
}

/// Canonical client outbox record. Queryable fields remain separate from opaque payload bytes.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct OutboxRecord {
    pub operation_id: OperationId,
    pub tenant_id: TenantId,
    pub actor_id: ActorId,
    pub device_id: DeviceId,
    pub entity: EntityRef,
    pub operation_kind: OperationKind,
    pub operation_schema_version: SchemaVersion,
    pub base_version: Option<EntityVersion>,
    pub local_seq: LocalOperationSeq,
    pub state: OutboxState,
    pub priority: u16,
    pub created_at: Timestamp,
    pub next_retry_at: Option<Timestamp>,
    pub attempt_count: u32,
    pub ever_sent: bool,
    pub payload_bytes: Vec<u8>,
    pub payload_digest: Digest,
    pub correlation_id: CorrelationId,
    pub causation_id: Option<LineageRef>,
    pub authority_epoch_first_sent: Option<AuthorityEpoch>,
    pub compaction_key: Option<Vec<u8>>,
}

impl OutboxRecord {
    /// Validates immutable payload identity and state-dependent retry metadata.
    pub fn validate(&self) -> Result<(), PersistenceError> {
        if self.payload_digest != payload_digest(&self.payload_bytes) {
            return Err(PersistenceError::PayloadDigestMismatch);
        }
        if self.ever_sent != self.authority_epoch_first_sent.is_some() {
            return Err(PersistenceError::InvalidRecord(
                "ever_sent and first authority epoch disagree".into(),
            ));
        }
        if self.state == OutboxState::Retryable && self.next_retry_at.is_none() {
            return Err(PersistenceError::InvalidRecord(
                "retryable outbox record lacks next_retry_at".into(),
            ));
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct OutboxHistoryRecord {
    pub operation_id: OperationId,
    pub final_state: OutboxState,
    pub committed_sequence: Option<Sequence>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ScopeCursorRecord {
    pub scope_id: SyncScopeId,
    pub scope_version: ScopeVersion,
    pub scope_generation: ScopeGeneration,
    pub authority_id: AuthorityId,
    pub authority_epoch: AuthorityEpoch,
    pub sequence: Sequence,
    pub projection_schema_version: ProjectionVersion,
    pub updated_at: Timestamp,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum SubscriptionState {
    Requested,
    Active,
    Paused,
    Revoked,
}
impl PersistentState for SubscriptionState {
    fn persistent_id(self) -> u16 {
        match self {
            Self::Requested => 1,
            Self::Active => 2,
            Self::Paused => 3,
            Self::Revoked => 4,
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum CachePolicy {
    Minimal,
    Standard,
    Full,
}
impl PersistentState for CachePolicy {
    fn persistent_id(self) -> u16 {
        match self {
            Self::Minimal => 1,
            Self::Standard => 2,
            Self::Full => 3,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct SubscriptionRecord {
    pub subscription_id: SubscriptionId,
    pub scope_id: SyncScopeId,
    pub state: SubscriptionState,
    pub cache_policy: CachePolicy,
    pub requested_at: Timestamp,
    pub activated_at: Option<Timestamp>,
    pub last_used_at: Timestamp,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ScopeDescriptorRecord {
    pub scope_id: SyncScopeId,
    pub scope_version: ScopeVersion,
    pub scope_generation: ScopeGeneration,
    pub projection_schema_version: ProjectionVersion,
    pub resolved_parameters: Vec<u8>,
    pub policy_version: u32,
    pub authority_epoch: AuthorityEpoch,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum MembershipState {
    Present,
    PendingEviction,
}
impl PersistentState for MembershipState {
    fn persistent_id(self) -> u16 {
        match self {
            Self::Present => 1,
            Self::PendingEviction => 2,
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct EntityScopeRefRecord {
    pub entity: EntityRef,
    pub scope_id: SyncScopeId,
    pub membership_state: MembershipState,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum ConflictResolutionState {
    Open,
    Resolving,
    Resolved,
    Expired,
}
impl PersistentState for ConflictResolutionState {
    fn persistent_id(self) -> u16 {
        match self {
            Self::Open => 1,
            Self::Resolving => 2,
            Self::Resolved => 3,
            Self::Expired => 4,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ConflictRecord {
    pub conflict_id: ConflictId,
    pub operation_id: OperationId,
    pub entity_ref: EntityRef,
    pub base_version: Option<EntityVersion>,
    pub authoritative_version: Option<EntityVersion>,
    pub conflict_kind: u16,
    pub conflicting_fields: Vec<u32>,
    pub created_at: Timestamp,
    pub resolution_state: ConflictResolutionState,
    pub retained_payload_ref: Option<String>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum BootstrapState {
    Requested,
    Downloading,
    Installing,
    Verifying,
    Active,
    Failed,
    Quarantined,
}
impl PersistentState for BootstrapState {
    fn persistent_id(self) -> u16 {
        match self {
            Self::Requested => 1,
            Self::Downloading => 2,
            Self::Installing => 3,
            Self::Verifying => 4,
            Self::Active => 5,
            Self::Failed => 6,
            Self::Quarantined => 7,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct BootstrapJobRecord {
    pub bootstrap_job_id: BootstrapJobId,
    pub scope_id: SyncScopeId,
    pub snapshot_id: SnapshotId,
    pub authority_epoch: AuthorityEpoch,
    pub state: BootstrapState,
    pub manifest_digest: Digest,
    pub started_at: Timestamp,
    pub updated_at: Timestamp,
    pub staging_generation: LocalStoreGeneration,
    pub boundary_sequence: Sequence,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum ChunkState {
    Pending,
    Downloading,
    Downloaded,
    Verified,
    Installed,
    Failed,
}
impl PersistentState for ChunkState {
    fn persistent_id(self) -> u16 {
        match self {
            Self::Pending => 1,
            Self::Downloading => 2,
            Self::Downloaded => 3,
            Self::Verified => 4,
            Self::Installed => 5,
            Self::Failed => 6,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct BootstrapChunkRecord {
    pub bootstrap_job_id: BootstrapJobId,
    pub chunk_id: Digest,
    pub ordinal: u32,
    pub state: ChunkState,
    pub downloaded_bytes: u64,
    pub expected_bytes: u64,
    pub hash: Digest,
    pub local_storage_ref: Option<String>,
    pub attempts: u32,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum RepairState {
    Detected,
    Planned,
    Applying,
    Verifying,
    Complete,
    Failed,
    Quarantined,
}
impl PersistentState for RepairState {
    fn persistent_id(self) -> u16 {
        match self {
            Self::Detected => 1,
            Self::Planned => 2,
            Self::Applying => 3,
            Self::Verifying => 4,
            Self::Complete => 5,
            Self::Failed => 6,
            Self::Quarantined => 7,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct RepairJobRecord {
    pub repair_id: RepairId,
    pub scope_id: SyncScopeId,
    pub partition_id: Vec<u8>,
    pub state: RepairState,
    pub detected_at: Timestamp,
    pub expected_digest: Digest,
    pub actual_digest: Digest,
    pub repair_plan_digest: Option<Digest>,
    pub completed_at: Option<Timestamp>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct IntegrityNodeRecord {
    pub scope_id: SyncScopeId,
    pub generation: u64,
    pub partition_path: Vec<u8>,
    pub digest: Digest,
    pub sequence_boundary: Sequence,
    pub updated_at: Timestamp,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum CircuitState {
    Closed,
    Open,
    HalfOpen,
}
impl PersistentState for CircuitState {
    fn persistent_id(self) -> u16 {
        match self {
            Self::Closed => 1,
            Self::Open => 2,
            Self::HalfOpen => 3,
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct SchedulerStateRecord {
    pub backoff_until: Option<Timestamp>,
    pub last_success: Option<Timestamp>,
    pub circuit_state: CircuitState,
    pub batch_size_hint: u32,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct CoordinatorLeaseRecord {
    pub local_store_id: StoreId,
    pub process_instance_id: ProcessInstanceId,
    pub fencing_token: u64,
    pub expires_at: Timestamp,
    pub updated_at: Timestamp,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum PurgeDirectiveState {
    Received,
    WaitingForSafePoint,
    Purging,
    Complete,
    Restricted,
}
impl PersistentState for PurgeDirectiveState {
    fn persistent_id(self) -> u16 {
        match self {
            Self::Received => 1,
            Self::WaitingForSafePoint => 2,
            Self::Purging => 3,
            Self::Complete => 4,
            Self::Restricted => 5,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct PurgeDirectiveRecord {
    pub directive_id: PurgeDirectiveId,
    pub scope_id: SyncScopeId,
    pub reason: u16,
    pub state: PurgeDirectiveState,
    pub received_at: Timestamp,
    pub completed_at: Option<Timestamp>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum PublicKeyStatus {
    Pending,
    Active,
    Retiring,
    VerificationOnly,
    Revoked,
    Destroyed,
}
impl PersistentState for PublicKeyStatus {
    fn persistent_id(self) -> u16 {
        match self {
            Self::Pending => 1,
            Self::Active => 2,
            Self::Retiring => 3,
            Self::VerificationOnly => 4,
            Self::Revoked => 5,
            Self::Destroyed => 6,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ClientKeyMetadataRecord {
    pub device_signing_key_id: KeyId,
    pub secure_store_reference: String,
    pub key_status: PublicKeyStatus,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum AuthorityRole {
    Primary,
    Standby,
    ReadReplica,
    Recovering,
    Demoted,
}
impl PersistentState for AuthorityRole {
    fn persistent_id(self) -> u16 {
        match self {
            Self::Primary => 1,
            Self::Standby => 2,
            Self::ReadReplica => 3,
            Self::Recovering => 4,
            Self::Demoted => 5,
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct AuthorityStateRecord {
    pub authority_id: AuthorityId,
    pub authority_epoch: AuthorityEpoch,
    pub authority_instance_id: AuthorityInstanceId,
    pub role: AuthorityRole,
    pub fence_token: u64,
    pub transition_id: AuthorityTransitionId,
    pub updated_at: Timestamp,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct AuthorityTransitionRecord {
    pub transition_id: AuthorityTransitionId,
    pub old_epoch: AuthorityEpoch,
    pub new_epoch: AuthorityEpoch,
    pub promotion_class: u16,
    pub old_final_sequence: Sequence,
    pub new_base_sequence: Sequence,
    pub reason: u16,
    pub created_at: Timestamp,
    pub actor: String,
    pub signature_ref: String,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum LedgerStatus {
    Accepted,
    Rejected,
    Conflict,
    Superseded,
}
impl PersistentState for LedgerStatus {
    fn persistent_id(self) -> u16 {
        match self {
            Self::Accepted => 1,
            Self::Rejected => 2,
            Self::Conflict => 3,
            Self::Superseded => 4,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct OperationLedgerRecord {
    pub operation_id: OperationId,
    pub tenant_id: TenantId,
    pub operation_kind: OperationKind,
    pub operation_schema_version: SchemaVersion,
    pub semantic_payload_digest: Digest,
    pub actor_id: ActorId,
    pub device_id: DeviceId,
    pub status: LedgerStatus,
    pub first_seen_at: Timestamp,
    pub committed_at: Option<Timestamp>,
    pub authority_epoch: AuthorityEpoch,
    pub committed_sequence: Option<Sequence>,
    pub entity_ref: EntityRef,
    pub base_version: Option<EntityVersion>,
    pub result_code: u32,
    pub handler_version: u32,
    pub execution_input_digest: Digest,
    pub execution_plan_digest: Digest,
}

pub type LedgerRecord = OperationLedgerRecord;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct JournalRecord {
    pub authority_epoch: AuthorityEpoch,
    pub sequence: Sequence,
    pub event_id: EventId,
    pub tenant_id: TenantId,
    pub entity: EntityRef,
    pub entity_version: EntityVersion,
    pub event_kind: ChangeKind,
    pub event_schema_version: SchemaVersion,
    pub routing_metadata: Vec<u8>,
    pub payload_bytes: Vec<u8>,
    pub payload_digest: Digest,
    pub operation_id: OperationId,
    pub correlation_id: CorrelationId,
    pub causation_id: Option<LineageRef>,
    pub occurred_at: Timestamp,
}

impl JournalRecord {
    pub fn validate(&self) -> Result<(), PersistenceError> {
        if self.payload_digest == payload_digest(&self.payload_bytes) {
            Ok(())
        } else {
            Err(PersistenceError::CorruptionDetected(
                "journal payload digest mismatch".into(),
            ))
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum ScopeRegistryState {
    Active,
    Paused,
    Retired,
}
impl PersistentState for ScopeRegistryState {
    fn persistent_id(self) -> u16 {
        match self {
            Self::Active => 1,
            Self::Paused => 2,
            Self::Retired => 3,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ScopeRegistryRecord {
    pub scope_id: SyncScopeId,
    pub tenant_id: TenantId,
    pub scope_kind: u32,
    pub scope_version: ScopeVersion,
    pub scope_generation: ScopeGeneration,
    pub projection_schema_version: ProjectionVersion,
    pub resolved_parameters: Vec<u8>,
    pub policy_version: u32,
    pub status: ScopeRegistryState,
    pub created_at: Timestamp,
    pub updated_at: Timestamp,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ScopeMembershipRecord {
    pub scope_id: SyncScopeId,
    pub entity: EntityRef,
    pub membership_version: u64,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum DeviceStatus {
    Active,
    Retired,
    Revoked,
    Expired,
}
impl PersistentState for DeviceStatus {
    fn persistent_id(self) -> u16 {
        match self {
            Self::Active => 1,
            Self::Retired => 2,
            Self::Revoked => 3,
            Self::Expired => 4,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct DeviceRecord {
    pub device_id: DeviceId,
    pub tenant_id: TenantId,
    pub principal_id: ActorId,
    pub status: DeviceStatus,
    pub registered_at: Timestamp,
    pub last_seen_at: Timestamp,
    pub current_public_key_id: Option<KeyId>,
    pub client_build: String,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct DeviceScopeWatermarkRecord {
    pub device_id: DeviceId,
    pub scope_id: SyncScopeId,
    pub authority_epoch: AuthorityEpoch,
    pub scope_generation: ScopeGeneration,
    pub last_ack_sequence: Sequence,
    pub last_seen_at: Timestamp,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum SnapshotState {
    Building,
    Verifying,
    Published,
    Expired,
    Failed,
}
impl PersistentState for SnapshotState {
    fn persistent_id(self) -> u16 {
        match self {
            Self::Building => 1,
            Self::Verifying => 2,
            Self::Published => 3,
            Self::Expired => 4,
            Self::Failed => 5,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct SnapshotRecord {
    pub snapshot_id: SnapshotId,
    pub tenant_id: TenantId,
    pub scope_id: SyncScopeId,
    pub scope_generation: ScopeGeneration,
    pub authority_epoch: AuthorityEpoch,
    pub boundary_sequence: Sequence,
    pub snapshot_schema_version: u32,
    pub profile: u16,
    pub state: SnapshotState,
    pub manifest_digest: Digest,
    pub root_digest: Digest,
    pub created_at: Timestamp,
    pub published_at: Option<Timestamp>,
    pub expires_at: Option<Timestamp>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct SnapshotChunkRecord {
    pub snapshot_id: SnapshotId,
    pub chunk_id: Digest,
    pub ordinal: u32,
    pub object_ref: String,
    pub compressed_bytes: u64,
    pub uncompressed_bytes: u64,
    pub ciphertext_digest: Digest,
    pub canonical_digest: Digest,
    pub compression: u16,
    pub encryption_key_id: Option<KeyId>,
    pub durable: bool,
    pub verified: bool,
    pub authority_epoch: AuthorityEpoch,
    pub boundary_sequence: Sequence,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct SnapshotLeaseRecord {
    pub snapshot_id: SnapshotId,
    pub lease_id: SnapshotLeaseId,
    pub device_or_session_ref: String,
    pub expires_at: Timestamp,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct SnapshotBuildJobRecord {
    pub snapshot_id: SnapshotId,
    pub builder_lease: Option<SnapshotLeaseId>,
    pub checkpoint: Vec<u8>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct AuditEventRecord {
    pub audit_event_id: Uuid,
    pub tenant_id: TenantId,
    pub audit_epoch: u64,
    pub audit_sequence: u64,
    pub category: u16,
    pub action_id: u32,
    pub subject_kind: u16,
    pub subject_id: Vec<u8>,
    pub actor_kind: u16,
    pub actor_id: Option<ActorId>,
    pub occurred_at: Timestamp,
    pub operation_id: Option<OperationId>,
    pub event_id: Option<EventId>,
    pub correlation_id: CorrelationId,
    pub reason_code: u32,
    pub schema_version: u32,
    pub payload_bytes: Vec<u8>,
    pub previous_hash: Digest,
    pub event_hash: Digest,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct FieldProvenanceRecord {
    pub entity: EntityRef,
    pub field_id: u32,
    pub audit_event_id: Uuid,
    pub event_id: EventId,
    pub updated_at: Timestamp,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct AuditCheckpointRecord {
    pub tenant_id: TenantId,
    pub audit_epoch: u64,
    pub sequence: u64,
    pub root_hash: Digest,
    pub signing_key_id: KeyId,
    pub signature: Vec<u8>,
    pub created_at: Timestamp,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct JournalCheckpointRecord {
    pub authority_id: AuthorityId,
    pub authority_epoch: AuthorityEpoch,
    pub sequence: Sequence,
    pub journal_root: Digest,
    pub signature_ref: String,
    pub created_at: Timestamp,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum JobState {
    Queued,
    Running,
    Waiting,
    Complete,
    Failed,
    Cancelled,
    Quarantined,
}
impl PersistentState for JobState {
    fn persistent_id(self) -> u16 {
        match self {
            Self::Queued => 1,
            Self::Running => 2,
            Self::Waiting => 3,
            Self::Complete => 4,
            Self::Failed => 5,
            Self::Cancelled => 6,
            Self::Quarantined => 7,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ImportJobRecord {
    pub job_id: ImportJobId,
    pub tenant_id: TenantId,
    pub mode: u16,
    pub source_kind: u16,
    pub source_fingerprint: Digest,
    pub state: JobState,
    pub mapping_version: u32,
    pub checkpoint: Vec<u8>,
    pub correlation_id: CorrelationId,
    pub started_at: Timestamp,
    pub updated_at: Timestamp,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ImportRecord {
    pub job_id: ImportJobId,
    pub source_key: Vec<u8>,
    pub target_entity_id: Option<EntityRef>,
    pub status: u16,
    pub canonical_digest: Digest,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ImportIdentityMapRecord {
    pub job_id: ImportJobId,
    pub source_type: u32,
    pub source_key: Vec<u8>,
    pub entity: EntityRef,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ImportQuarantineRecord {
    pub job_id: ImportJobId,
    pub source_key: Vec<u8>,
    pub error_code: u32,
    pub status: u16,
    pub sanitized_details: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ExportJobRecord {
    pub export_id: ExportJobId,
    pub tenant_id: TenantId,
    pub mode: u16,
    pub state: JobState,
    pub boundary: Vec<u8>,
    pub manifest_digest: Option<Digest>,
    pub storage_ref: Option<String>,
    pub expires_at: Option<Timestamp>,
    pub created_by: ActorId,
    pub created_at: Timestamp,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ReplayArtifactRecord {
    pub replay_id: ReplayArtifactId,
    pub operation_id: OperationId,
    pub handler_version: u32,
    pub artifact_ref: String,
    pub artifact_digest: Digest,
    pub retention_class: u32,
    pub expires_at: Option<Timestamp>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct RetentionPolicyRecord {
    pub retention_class_id: u32,
    pub policy_version: u32,
    pub minimum_retention_seconds: u64,
    pub maximum_retention_seconds: Option<u64>,
    pub deletion_mode: u16,
    pub legal_hold_eligible: bool,
    pub archive_before_delete: bool,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct LegalHoldRecord {
    pub hold_id: LegalHoldId,
    pub tenant_id: TenantId,
    pub selector_kind: u16,
    pub selector_payload: Vec<u8>,
    pub reason_code: u32,
    pub state: u16,
    pub created_by: ActorId,
    pub created_at: Timestamp,
    pub released_by: Option<ActorId>,
    pub released_at: Option<Timestamp>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ErasureRequestRecord {
    pub request_id: ErasureRequestId,
    pub tenant_id: TenantId,
    pub subject_ref: Vec<u8>,
    pub state: JobState,
    pub policy_version: u32,
    pub requested_at: Timestamp,
    pub completed_at: Option<Timestamp>,
    pub verification_digest: Option<Digest>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct PurgeJobRecord {
    pub purge_id: PurgeJobId,
    pub tenant_id: TenantId,
    pub state: JobState,
    pub policy_version: u32,
    pub plan_digest: Digest,
    pub started_at: Timestamp,
    pub completed_at: Option<Timestamp>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ErasureLedgerRecord {
    pub request_id: ErasureRequestId,
    pub subject_pseudonymous_ref: Digest,
    pub completed_at: Timestamp,
    pub policy_version: u32,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct StorageSurfaceRecord {
    pub storage_surface_id: StorageSurfaceId,
    pub kind: u16,
    pub status: u16,
    pub capabilities: u64,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct KeyRegistryRecord {
    pub key_id: KeyId,
    pub purpose: u16,
    pub algorithm: u16,
    pub public_key: Vec<u8>,
    pub status: PublicKeyStatus,
    pub not_before: Timestamp,
    pub not_after: Option<Timestamp>,
    pub revoked_at: Option<Timestamp>,
    pub registry_generation: u64,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct CryptoPolicyRecord {
    pub policy_version: u32,
    pub allowed_digests: Vec<u16>,
    pub allowed_signatures: Vec<u16>,
    pub allowed_encryption: Vec<u16>,
    pub required_features: Vec<u32>,
    pub created_at: Timestamp,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct CompatibilityPolicyRecord {
    pub policy_generation: u64,
    pub preferred_protocol: ProtocolVersion,
    pub supported_protocols: Vec<ProtocolVersion>,
    pub deprecated_protocols: Vec<ProtocolVersion>,
    pub minimum_build_constraints: Vec<u8>,
    pub required_capabilities: Vec<u32>,
    pub updated_at: Timestamp,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct RegionalReplicaWatermarkRecord {
    pub replica_id: ReplicaId,
    pub region_id: u16,
    pub authority_epoch: AuthorityEpoch,
    pub sequence: Sequence,
    pub projection_id: u32,
    pub updated_at: Timestamp,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct JobRecord {
    pub job_id: JobId,
    pub tenant_id: Option<TenantId>,
    pub job_kind: u32,
    pub state: JobState,
    pub priority: u16,
    pub payload_ref: Option<String>,
    pub checkpoint: Vec<u8>,
    pub attempt_count: u32,
    pub next_run_at: Option<Timestamp>,
    pub created_at: Timestamp,
    pub updated_at: Timestamp,
    pub row_version: u64,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct JobLeaseRecord {
    pub job_id: JobId,
    pub worker_id: WorkerId,
    pub fencing_token: u64,
    pub expires_at: Timestamp,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct MetadataMigrationRecord {
    pub migration_id: MetadataMigrationId,
    pub applied_at: Timestamp,
    pub binary_version: String,
    pub checksum: Digest,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct JournalFloorRecord {
    pub scope_id: SyncScopeId,
    pub authority_epoch: AuthorityEpoch,
    pub minimum_sequence: Sequence,
    pub updated_at: Timestamp,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct RetiredEntityRecord {
    pub entity: EntityRef,
    pub deletion_epoch: AuthorityEpoch,
    pub deletion_sequence: Sequence,
    pub expires_at: Timestamp,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct OperationRecoveryRecord {
    pub operation_id: OperationId,
    pub old_epoch: AuthorityEpoch,
    pub new_epoch: AuthorityEpoch,
    pub resolution: u16,
    pub resolved_at: Timestamp,
    pub actor: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct SideEffectIntentRecord {
    pub intent_id: SideEffectIntentId,
    pub operation_id: OperationId,
    pub kind: u32,
    pub state: JobState,
    pub idempotency_key: Vec<u8>,
    pub attempt_count: u32,
    pub next_attempt_at: Option<Timestamp>,
    pub payload: Vec<u8>,
    pub payload_digest: Digest,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct SideEffectResultRecord {
    pub intent_id: SideEffectIntentId,
    pub provider_reference: String,
    pub outcome: u16,
    pub response_digest: Digest,
    pub completed_at: Timestamp,
}

/// Cached/approximate counts and byte totals suitable for bounded metrics scrapes.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct MetadataMetricsSnapshot {
    pub outbox_rows: u64,
    pub outbox_bytes: u64,
    pub journal_rows: u64,
    pub journal_bytes: u64,
    pub ledger_rows: u64,
    pub audit_rows: u64,
    pub audit_bytes: u64,
    pub snapshot_count: u64,
    pub conflict_count: u64,
    pub measured_at: Timestamp,
    pub approximate: bool,
}
