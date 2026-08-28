//! Database-neutral contracts for durable background jobs.
//!
//! A queue notification is only a wake hint. Implementations of [`JobStore`] own the durable
//! source of truth and must enforce compare-and-swap state transitions and fencing atomically.

#![allow(clippy::missing_errors_doc)]

use aequora_scheduler::WorkClass;
use aequora_types::{AuthorityEpoch, CorrelationId, JobId, LineageRef, RegionId, TenantId};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use thiserror::Error;
use uuid::Uuid;

pub const MAX_INLINE_PAYLOAD_BYTES: usize = 64 * 1024;
pub const MAX_CHECKPOINT_BYTES: usize = 64 * 1024;
pub const MAX_CLAIM_BATCH: usize = 1_024;
pub const MAX_DEPENDENCIES: usize = 1_024;
pub const MAX_FAN_OUT: usize = 10_000;

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

uuid_id!(WorkerId, "Operational identity of one worker process.");
uuid_id!(WorkflowId, "Stable identity of one durable workflow.");
uuid_id!(ScheduleId, "Stable identity of one recurring schedule.");

#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct JobKind(u32);
impl JobKind {
    pub const fn new(value: u32) -> Result<Self, JobError> {
        if value == 0 {
            Err(JobError::ZeroIdentity)
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
pub struct JobPayloadSchemaVersion(u16);
impl JobPayloadSchemaVersion {
    pub const fn new(value: u16) -> Result<Self, JobError> {
        if value == 0 {
            Err(JobError::ZeroIdentity)
        } else {
            Ok(Self(value))
        }
    }
    #[must_use]
    pub const fn get(self) -> u16 {
        self.0
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct FencingToken(u64);
impl FencingToken {
    pub const INITIAL: Self = Self(1);
    pub const fn new(value: u64) -> Result<Self, JobError> {
        if value == 0 {
            Err(JobError::ZeroIdentity)
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

#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct WorkerCapability(u32);
impl WorkerCapability {
    pub const fn new(value: u32) -> Result<Self, JobError> {
        if value == 0 {
            Err(JobError::ZeroIdentity)
        } else {
            Ok(Self(value))
        }
    }
    #[must_use]
    pub const fn get(self) -> u32 {
        self.0
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum JobState {
    Pending,
    Ready,
    Running,
    Waiting,
    RetryScheduled,
    CancelRequested,
    Completed,
    Failed,
    Canceled,
    Quarantined,
}
impl JobState {
    #[must_use]
    pub const fn is_terminal(self) -> bool {
        matches!(
            self,
            Self::Completed | Self::Failed | Self::Canceled | Self::Quarantined
        )
    }

    #[must_use]
    pub fn can_transition_to(self, next: Self) -> bool {
        use JobState::{
            CancelRequested, Canceled, Completed, Failed, Pending, Quarantined, Ready,
            RetryScheduled, Running, Waiting,
        };
        matches!(
            (self, next),
            (Pending | Waiting, Ready | Canceled)
                | (Ready | RetryScheduled, Running | Canceled)
                | (
                    Running,
                    Completed | Failed | Quarantined | RetryScheduled | Waiting | CancelRequested
                )
                | (CancelRequested, Canceled | Completed | Failed | Quarantined)
                | (Failed | Quarantined | Canceled, Ready)
        ) || self == next
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum JobPayloadRef {
    Inline(Vec<u8>),
    ObjectRef(String),
    BlobRef(String),
    ArtifactRef(String),
}
impl JobPayloadRef {
    pub fn verify(&self) -> Result<(), JobError> {
        match self {
            Self::Inline(bytes) if bytes.len() > MAX_INLINE_PAYLOAD_BYTES => {
                Err(JobError::PayloadTooLarge)
            }
            Self::ObjectRef(value) | Self::BlobRef(value) | Self::ArtifactRef(value)
                if value.is_empty() || value.len() > 1_024 =>
            {
                Err(JobError::InvalidReference)
            }
            _ => Ok(()),
        }
    }

    #[must_use]
    pub fn digest(&self) -> [u8; 32] {
        let mut hasher = blake3::Hasher::new();
        match self {
            Self::Inline(bytes) => {
                hasher.update(b"inline\0");
                hasher.update(bytes);
            }
            Self::ObjectRef(value) => {
                hasher.update(b"object\0");
                hasher.update(value.as_bytes());
            }
            Self::BlobRef(value) => {
                hasher.update(b"blob\0");
                hasher.update(value.as_bytes());
            }
            Self::ArtifactRef(value) => {
                hasher.update(b"artifact\0");
                hasher.update(value.as_bytes());
            }
        }
        *hasher.finalize().as_bytes()
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct JobCheckpoint {
    pub bytes: Vec<u8>,
    pub completed_units: u64,
    pub total_units: Option<u64>,
}
impl JobCheckpoint {
    pub fn verify(&self) -> Result<(), JobError> {
        if self.bytes.len() > MAX_CHECKPOINT_BYTES {
            return Err(JobError::CheckpointTooLarge);
        }
        if self
            .total_units
            .is_some_and(|total| self.completed_units > total)
        {
            return Err(JobError::InvalidProgress);
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum JobEpochPolicy {
    Continue,
    Revalidate,
    Reconcile,
    Cancel,
    Manual,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum JobExecutionRegionPolicy {
    AuthorityOnly,
    AnyAllowedRegion,
    SpecificRegion(RegionId),
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct JobRecord {
    pub job_id: JobId,
    pub tenant_id: TenantId,
    pub kind: JobKind,
    pub state: JobState,
    pub priority: WorkClass,
    pub payload_schema_version: JobPayloadSchemaVersion,
    pub payload: JobPayloadRef,
    pub payload_digest: [u8; 32],
    pub checkpoint: Option<JobCheckpoint>,
    pub workflow_id: Option<WorkflowId>,
    pub lineage: aequora_types::LineageContext,
    pub attempt_count: u32,
    pub next_run_at_unix_ms: Option<u64>,
    pub deadline_unix_ms: Option<u64>,
    pub created_at_unix_ms: u64,
    pub updated_at_unix_ms: u64,
    pub created_under_epoch: Option<AuthorityEpoch>,
    pub epoch_policy: JobEpochPolicy,
    pub region_policy: JobExecutionRegionPolicy,
    pub governance: JobGovernanceMetadata,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct JobGovernanceMetadata {
    pub retention_class: u16,
    pub contains_personal_data: bool,
    pub legal_hold_eligible: bool,
    pub subject_references: Vec<String>,
    pub artifact_references: Vec<String>,
}
impl JobGovernanceMetadata {
    pub fn verify(&self) -> Result<(), JobError> {
        if self.retention_class == 0
            || self
                .subject_references
                .iter()
                .chain(&self.artifact_references)
                .any(|reference| reference.is_empty() || reference.len() > 1_024)
        {
            return Err(JobError::InvalidGovernanceMetadata);
        }
        Ok(())
    }
}
impl JobRecord {
    pub fn verify(&self) -> Result<(), JobError> {
        self.payload.verify()?;
        if self.payload_digest != self.payload.digest() {
            return Err(JobError::PayloadDigestMismatch);
        }
        if self.updated_at_unix_ms < self.created_at_unix_ms {
            return Err(JobError::InvalidTimestampOrder);
        }
        if self
            .deadline_unix_ms
            .is_some_and(|deadline| deadline < self.created_at_unix_ms)
        {
            return Err(JobError::InvalidTimestampOrder);
        }
        if let Some(checkpoint) = &self.checkpoint {
            checkpoint.verify()?;
        }
        self.governance.verify()?;
        Ok(())
    }
}

#[derive(Clone, Debug)]
pub struct NewJob {
    pub job: JobRecord,
    pub dependencies: Vec<JobDependency>,
    pub required_capabilities: BTreeSet<WorkerCapability>,
}
impl NewJob {
    pub fn verify(&self) -> Result<(), JobError> {
        self.job.verify()?;
        if !matches!(self.job.state, JobState::Pending | JobState::Ready) {
            return Err(JobError::InvalidInitialState);
        }
        if self.dependencies.len() > MAX_DEPENDENCIES {
            return Err(JobError::DependencyLimit);
        }
        if self
            .dependencies
            .iter()
            .any(|edge| edge.job_id != self.job.job_id || edge.job_id == edge.depends_on_job_id)
        {
            return Err(JobError::InvalidDependency);
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct JobLease {
    pub job_id: JobId,
    pub worker_id: WorkerId,
    pub fencing_token: FencingToken,
    pub expires_at_unix_ms: u64,
}
impl JobLease {
    #[must_use]
    pub const fn is_current_at(self, now_unix_ms: u64) -> bool {
        now_unix_ms < self.expires_at_unix_ms
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ClaimedJob {
    pub job: JobRecord,
    pub lease: JobLease,
}

#[derive(Clone, Debug)]
pub struct ClaimRequest {
    pub worker_id: WorkerId,
    pub supported_kinds: BTreeSet<JobKind>,
    pub capabilities: BTreeSet<WorkerCapability>,
    pub allowed_regions: BTreeSet<RegionId>,
    pub now_unix_ms: u64,
    pub lease_duration_ms: u64,
    pub limit: usize,
}
impl ClaimRequest {
    pub fn verify(&self) -> Result<(), JobError> {
        if self.supported_kinds.is_empty() || self.lease_duration_ms == 0 {
            return Err(JobError::InvalidClaim);
        }
        if self.limit == 0 || self.limit > MAX_CLAIM_BATCH {
            return Err(JobError::ClaimLimit);
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum JobAttemptOutcome {
    Completed,
    RetryableFailure,
    TerminalFailure,
    Waiting,
    Canceled,
    Quarantined,
    LeaseLost,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct JobAttemptRecord {
    pub job_id: JobId,
    pub attempt_no: u32,
    pub worker_id: WorkerId,
    pub fencing_token: FencingToken,
    pub started_at_unix_ms: u64,
    pub finished_at_unix_ms: Option<u64>,
    pub outcome: Option<JobAttemptOutcome>,
    pub error_code: Option<u32>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
pub enum ErrorClass {
    InfrastructureTransient,
    ProviderTransient,
    RateLimited,
    InvalidPayload,
    AuthorizationRevoked,
    ProviderPermanent,
    InvariantViolation,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum JitterPolicy {
    None,
    Full,
    Equal,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct RetryPolicy {
    pub max_attempts: Option<u32>,
    pub base_delay_ms: u64,
    pub max_delay_ms: u64,
    pub jitter: JitterPolicy,
    pub retryable_errors: BTreeSet<ErrorClass>,
}
impl RetryPolicy {
    pub fn verify(&self) -> Result<(), JobError> {
        if self.base_delay_ms == 0 || self.max_delay_ms < self.base_delay_ms {
            return Err(JobError::InvalidRetryPolicy);
        }
        if self.max_attempts == Some(0) || self.retryable_errors.is_empty() {
            return Err(JobError::InvalidRetryPolicy);
        }
        if self.jitter == JitterPolicy::None {
            return Err(JobError::JitterRequired);
        }
        Ok(())
    }

    #[must_use]
    pub fn permits(&self, attempt: u32, class: ErrorClass) -> bool {
        self.retryable_errors.contains(&class)
            && self.max_attempts.is_none_or(|maximum| attempt < maximum)
    }

    pub fn delay_ms(&self, attempt: u32, entropy: u64) -> Result<u64, JobError> {
        self.verify()?;
        let exponent = attempt.saturating_sub(1).min(63);
        let cap = self
            .base_delay_ms
            .saturating_mul(1_u64 << exponent)
            .min(self.max_delay_ms);
        let delay = match self.jitter {
            JitterPolicy::None => cap,
            JitterPolicy::Full => entropy % cap.saturating_add(1),
            JitterPolicy::Equal => {
                let half = cap / 2;
                half.saturating_add(entropy % cap.saturating_sub(half).saturating_add(1))
            }
        };
        Ok(delay.max(1))
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum JobRunOutcome {
    Completed {
        result: Option<JobPayloadRef>,
    },
    Retryable {
        error: ErrorClass,
        retry_after_ms: Option<u64>,
    },
    TerminalFailure {
        error_code: u32,
    },
    Waiting(WaitCondition),
    Quarantined {
        reason_code: u32,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum WaitCondition {
    Until(u64),
    Job(JobId),
    ExternalCallback(String),
    ManualApproval,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
pub enum DependencyCondition {
    Completed,
    CompletedOrSkipped,
    Terminal,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
pub struct JobDependency {
    pub job_id: JobId,
    pub depends_on_job_id: JobId,
    pub condition: DependencyCondition,
}

pub fn verify_dependency_dag(edges: &[JobDependency]) -> Result<(), JobError> {
    if edges.len() > MAX_FAN_OUT {
        return Err(JobError::DependencyLimit);
    }
    let mut outgoing: BTreeMap<JobId, Vec<JobId>> = BTreeMap::new();
    let mut nodes = BTreeSet::new();
    for edge in edges {
        if edge.job_id == edge.depends_on_job_id {
            return Err(JobError::DependencyCycle);
        }
        outgoing
            .entry(edge.depends_on_job_id)
            .or_default()
            .push(edge.job_id);
        nodes.insert(edge.job_id);
        nodes.insert(edge.depends_on_job_id);
    }
    let mut indegree = nodes
        .iter()
        .map(|node| (*node, 0_usize))
        .collect::<BTreeMap<_, _>>();
    for edge in edges {
        if let Some(value) = indegree.get_mut(&edge.job_id) {
            *value = value.saturating_add(1);
        }
    }
    let mut ready = indegree
        .iter()
        .filter_map(|(node, degree)| (*degree == 0).then_some(*node))
        .collect::<Vec<_>>();
    let mut visited = 0_usize;
    while let Some(node) = ready.pop() {
        visited = visited.saturating_add(1);
        if let Some(children) = outgoing.get(&node) {
            for child in children {
                if let Some(degree) = indegree.get_mut(child) {
                    *degree = degree.saturating_sub(1);
                    if *degree == 0 {
                        ready.push(*child);
                    }
                }
            }
        }
    }
    if visited == nodes.len() {
        Ok(())
    } else {
        Err(JobError::DependencyCycle)
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
pub enum ConcurrencyClass {
    DatabaseLight,
    CpuHeavy,
    ExternalHttp,
    SnapshotBuild,
    PaymentProvider,
    EmailProvider,
    Custom(u16),
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct JobDescriptor {
    pub kind: JobKind,
    pub payload_versions: BTreeSet<JobPayloadSchemaVersion>,
    pub retry_policy: RetryPolicy,
    pub attempt_timeout_ms: u64,
    pub concurrency_class: ConcurrencyClass,
    pub required_capabilities: BTreeSet<WorkerCapability>,
    pub requires_external_idempotency: bool,
}
impl JobDescriptor {
    pub fn verify(&self) -> Result<(), JobError> {
        self.retry_policy.verify()?;
        if self.payload_versions.is_empty() || self.attempt_timeout_ms == 0 {
            return Err(JobError::InvalidDescriptor);
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Default)]
pub struct JobRegistry {
    descriptors: BTreeMap<JobKind, JobDescriptor>,
}
impl JobRegistry {
    #[must_use]
    pub const fn new() -> Self {
        Self {
            descriptors: BTreeMap::new(),
        }
    }
    pub fn register(&mut self, descriptor: JobDescriptor) -> Result<(), JobError> {
        descriptor.verify()?;
        if self
            .descriptors
            .insert(descriptor.kind, descriptor)
            .is_some()
        {
            return Err(JobError::DuplicateKind);
        }
        Ok(())
    }
    #[must_use]
    pub fn descriptor(&self, kind: JobKind) -> Option<&JobDescriptor> {
        self.descriptors.get(&kind)
    }
    #[must_use]
    pub fn supported_kinds(&self) -> BTreeSet<JobKind> {
        self.descriptors.keys().copied().collect()
    }
}

#[derive(Clone, Debug)]
pub struct JobHandlerContext {
    pub job_id: JobId,
    pub tenant_id: TenantId,
    pub attempt: u32,
    pub fence: FencingToken,
    pub correlation_id: CorrelationId,
    pub caused_by: Option<LineageRef>,
    pub cancellation_requested: bool,
}

#[async_trait]
pub trait JobHandler: Send + Sync {
    async fn run(
        &self,
        context: &JobHandlerContext,
        payload: &JobPayloadRef,
        checkpoint: Option<&JobCheckpoint>,
    ) -> Result<JobRunOutcome, JobError>;
}

pub trait JobPayloadUpcaster: Send + Sync {
    fn kind(&self) -> JobKind;
    fn source_version(&self) -> JobPayloadSchemaVersion;
    fn target_version(&self) -> JobPayloadSchemaVersion;
    fn upcast(&self, payload: &[u8]) -> Result<Vec<u8>, JobError>;
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TenantFairnessPolicy {
    pub maximum_claims_per_tenant: usize,
    pub maximum_consecutive_claims: usize,
}
impl TenantFairnessPolicy {
    pub fn verify(self) -> Result<(), JobError> {
        if self.maximum_claims_per_tenant == 0 || self.maximum_consecutive_claims == 0 {
            Err(JobError::InvalidFairnessPolicy)
        } else {
            Ok(())
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FencedUpdate {
    pub job_id: JobId,
    pub worker_id: WorkerId,
    pub fencing_token: FencingToken,
    pub expected_state: JobState,
    pub now_unix_ms: u64,
}

pub fn validate_fenced_update(
    job: &JobRecord,
    lease: JobLease,
    update: FencedUpdate,
) -> Result<(), JobStoreError> {
    if job.job_id != update.job_id || lease.job_id != update.job_id {
        return Err(JobStoreError::NotFound);
    }
    if job.state != update.expected_state {
        return Err(JobStoreError::StateConflict);
    }
    if lease.worker_id != update.worker_id {
        return Err(JobStoreError::LeaseLost);
    }
    if lease.fencing_token != update.fencing_token {
        return Err(JobStoreError::StaleFence);
    }
    if !lease.is_current_at(update.now_unix_ms) {
        return Err(JobStoreError::LeaseLost);
    }
    Ok(())
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EpochRecoveryAction {
    Continue,
    Revalidate,
    ReconcileExternal,
    CancelDerived,
    ManualReview,
}

#[must_use]
pub const fn epoch_recovery_action(
    created_under: Option<AuthorityEpoch>,
    current: AuthorityEpoch,
    policy: JobEpochPolicy,
) -> EpochRecoveryAction {
    if matches!(created_under, Some(epoch) if epoch.get() == current.get()) {
        return EpochRecoveryAction::Continue;
    }
    match policy {
        JobEpochPolicy::Continue => EpochRecoveryAction::Continue,
        JobEpochPolicy::Revalidate => EpochRecoveryAction::Revalidate,
        JobEpochPolicy::Reconcile => EpochRecoveryAction::ReconcileExternal,
        JobEpochPolicy::Cancel => EpochRecoveryAction::CancelDerived,
        JobEpochPolicy::Manual => EpochRecoveryAction::ManualReview,
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct JobStatus {
    pub job_id: JobId,
    pub tenant_id: TenantId,
    pub kind: JobKind,
    pub state: JobState,
    pub attempt_count: u32,
    pub progress: Option<JobProgress>,
    pub next_run_at_unix_ms: Option<u64>,
    pub result: Option<JobPayloadRef>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct JobProgress {
    pub completed_units: u64,
    pub total_units: Option<u64>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AdminAction {
    Retry,
    Cancel,
    Quarantine,
    Release,
}

#[async_trait]
pub trait JobStore: Send + Sync {
    async fn insert(&self, job: NewJob) -> Result<(), JobStoreError>;
    async fn claim_ready(&self, request: ClaimRequest) -> Result<Vec<ClaimedJob>, JobStoreError>;
    async fn renew_lease(
        &self,
        update: FencedUpdate,
        expires_at_unix_ms: u64,
    ) -> Result<JobLease, JobStoreError>;
    async fn checkpoint(
        &self,
        update: FencedUpdate,
        checkpoint: JobCheckpoint,
    ) -> Result<(), JobStoreError>;
    async fn finish(
        &self,
        update: FencedUpdate,
        outcome: JobRunOutcome,
    ) -> Result<(), JobStoreError>;
    async fn request_cancel(&self, job_id: JobId, tenant_id: TenantId)
    -> Result<(), JobStoreError>;
    async fn admin_transition(
        &self,
        job_id: JobId,
        tenant_id: TenantId,
        action: AdminAction,
        reason_code: u32,
    ) -> Result<(), JobStoreError>;
    async fn status(&self, job_id: JobId, tenant_id: TenantId) -> Result<JobStatus, JobStoreError>;
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum MissedOccurrencePolicy {
    RunImmediately,
    Skip,
    Coalesce,
    RunAll { maximum: u16 },
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ScheduleRule {
    pub interval_ms: u64,
    pub timezone_id: String,
    pub missed: MissedOccurrencePolicy,
}
impl ScheduleRule {
    pub fn verify(&self) -> Result<(), JobError> {
        if self.interval_ms == 0 || self.timezone_id.is_empty() || self.timezone_id.len() > 128 {
            return Err(JobError::InvalidSchedule);
        }
        if matches!(self.missed, MissedOccurrencePolicy::RunAll { maximum: 0 }) {
            return Err(JobError::InvalidSchedule);
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct RecurringSchedule {
    pub schedule_id: ScheduleId,
    pub tenant_id: TenantId,
    pub kind: JobKind,
    pub payload_schema_version: JobPayloadSchemaVersion,
    pub rule: ScheduleRule,
    pub enabled: bool,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
pub struct OccurrenceId([u8; 32]);
impl OccurrenceId {
    #[must_use]
    pub fn derive(schedule_id: ScheduleId, scheduled_at_unix_ms: u64) -> Self {
        let mut hasher = blake3::Hasher::new();
        hasher.update(schedule_id.as_uuid().as_bytes());
        hasher.update(&scheduled_at_unix_ms.to_be_bytes());
        Self(*hasher.finalize().as_bytes())
    }
    #[must_use]
    pub const fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }
}

#[async_trait]
pub trait ScheduleStore: Send + Sync {
    async fn put(&self, schedule: RecurringSchedule) -> Result<(), JobStoreError>;
    async fn due(
        &self,
        now_unix_ms: u64,
        limit: usize,
    ) -> Result<Vec<RecurringSchedule>, JobStoreError>;
    async fn materialize_occurrence(
        &self,
        occurrence: OccurrenceId,
        scheduled_at_unix_ms: u64,
        job: NewJob,
    ) -> Result<bool, JobStoreError>;
}

#[derive(Clone, Debug, Error, Eq, PartialEq)]
pub enum JobStoreError {
    #[error("job already exists")]
    Duplicate,
    #[error("job was not found in this tenant")]
    NotFound,
    #[error("job state compare-and-swap failed")]
    StateConflict,
    #[error("job lease is missing, expired, or owned by another worker")]
    LeaseLost,
    #[error("stale job fencing token")]
    StaleFence,
    #[error("job storage is temporarily unavailable")]
    Unavailable,
    #[error("job storage rejected invalid data: {0}")]
    Invalid(JobError),
    #[error("job storage adapter failure: {0}")]
    Adapter(String),
}

#[derive(Clone, Copy, Debug, Error, Eq, PartialEq)]
pub enum JobError {
    #[error("persistent numeric identity must be non-zero")]
    ZeroIdentity,
    #[error("inline job payload exceeds its hard bound")]
    PayloadTooLarge,
    #[error("job payload reference is empty or too large")]
    InvalidReference,
    #[error("job payload digest does not match payload")]
    PayloadDigestMismatch,
    #[error("job checkpoint exceeds its hard bound")]
    CheckpointTooLarge,
    #[error("job progress exceeds its declared total")]
    InvalidProgress,
    #[error("job timestamp order is invalid")]
    InvalidTimestampOrder,
    #[error("job has an invalid initial state")]
    InvalidInitialState,
    #[error("job dependency is invalid")]
    InvalidDependency,
    #[error("job dependency graph exceeds its hard bound")]
    DependencyLimit,
    #[error("job dependency graph contains a cycle")]
    DependencyCycle,
    #[error("job claim request is invalid")]
    InvalidClaim,
    #[error("job claim batch exceeds its hard bound")]
    ClaimLimit,
    #[error("retry policy is invalid")]
    InvalidRetryPolicy,
    #[error("fleet/provider retries require jitter")]
    JitterRequired,
    #[error("job descriptor is invalid")]
    InvalidDescriptor,
    #[error("job kind is already registered")]
    DuplicateKind,
    #[error("recurring schedule is invalid")]
    InvalidSchedule,
    #[error("job governance metadata is invalid")]
    InvalidGovernanceMetadata,
    #[error("job tenant-fairness policy is invalid")]
    InvalidFairnessPolicy,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn terminal_jobs_cannot_reenter_running_implicitly() {
        assert!(!JobState::Completed.can_transition_to(JobState::Running));
        assert!(JobState::Failed.can_transition_to(JobState::Ready));
    }

    #[test]
    fn dependency_cycle_is_rejected() {
        let a = JobId::new();
        let b = JobId::new();
        let edges = [
            JobDependency {
                job_id: a,
                depends_on_job_id: b,
                condition: DependencyCondition::Completed,
            },
            JobDependency {
                job_id: b,
                depends_on_job_id: a,
                condition: DependencyCondition::Completed,
            },
        ];
        assert_eq!(
            verify_dependency_dag(&edges),
            Err(JobError::DependencyCycle)
        );
    }

    #[test]
    fn retry_delay_is_bounded_and_jittered() {
        let policy = RetryPolicy {
            max_attempts: Some(10),
            base_delay_ms: 100,
            max_delay_ms: 1_000,
            jitter: JitterPolicy::Full,
            retryable_errors: [ErrorClass::ProviderTransient].into_iter().collect(),
        };
        assert!(
            policy
                .delay_ms(20, u64::MAX)
                .is_ok_and(|value| value > 0 && value <= 1_000)
        );
        assert!(policy.delay_ms(20, 10).is_ok_and(|value| value <= 1_000));
    }

    #[test]
    fn occurrence_identity_is_deterministic() {
        let schedule = ScheduleId::new();
        assert_eq!(
            OccurrenceId::derive(schedule, 42),
            OccurrenceId::derive(schedule, 42)
        );
        assert_ne!(
            OccurrenceId::derive(schedule, 42),
            OccurrenceId::derive(schedule, 43)
        );
    }
}
