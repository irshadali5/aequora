//! Runtime-, transport-, and database-neutral operational control-plane contracts.
//!
//! Mutations enter through [`AdminService`], which binds authorization, stable idempotency,
//! reviewed plans, approvals, subsystem execution, postcondition verification, and audit. Storage
//! adapters implement [`AdminStore`]; HTTP, CLI, and UI layers only map typed DTOs to this API.

#![allow(clippy::missing_errors_doc)]

use aequora_types::{AuthorityEpoch, JobId, TenantId};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::{Arc, Mutex},
};
use thiserror::Error;
use uuid::Uuid;

pub const ADMIN_API_VERSION: u16 = 1;
pub const MAX_REASON_NOTE_BYTES: usize = 4_096;
pub const MAX_DYNAMIC_POLICY_BYTES: usize = 256 * 1024;
pub const MAX_STEP_UP_AGE_MS: u64 = 5 * 60 * 1_000;

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
    AdminOperationId,
    "Stable idempotency identity of one privileged mutation."
);
uuid_id!(PlanId, "Stable identity of one reviewed operational plan.");
uuid_id!(ApprovalId, "Stable identity of one exact-action approval.");
uuid_id!(MaintenanceId, "Stable identity of one maintenance window.");
uuid_id!(
    BreakGlassSessionId,
    "Audited identity of one emergency elevation session."
);

#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[repr(u16)]
pub enum PermissionId {
    AuthorityView = 1,
    AuthorityPromote = 2,
    AuthorityForcePromote = 3,
    JobsView = 10,
    JobsRetry = 11,
    JobsReconcile = 12,
    JobsCancel = 13,
    JobsQuarantine = 14,
    SnapshotView = 20,
    SnapshotBuild = 21,
    SnapshotVerify = 22,
    SnapshotExpire = 23,
    IntegrityScan = 30,
    IntegrityRepair = 31,
    GovernancePlan = 40,
    GovernanceExecute = 41,
    LegalHoldManage = 42,
    CryptoView = 50,
    CryptoRotate = 51,
    CryptoRevoke = 52,
    CryptoDestroy = 53,
    CompatibilityView = 60,
    CompatibilityUpdate = 61,
    RegionView = 70,
    RegionDrain = 71,
    MaintenanceManage = 80,
    TenantManage = 90,
    DeviceManage = 91,
    ScopeManage = 92,
    DiagnosticsView = 100,
    DiagnosticsExport = 101,
    ConfigView = 110,
    ConfigUpdate = 111,
    OperationsView = 120,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[repr(u16)]
pub enum AdminActionKind {
    PromoteAuthority = 1,
    ForcePromoteAuthority = 2,
    RetryJob = 10,
    ReconcileJob = 11,
    CancelJob = 12,
    QuarantineJob = 13,
    BuildSnapshot = 20,
    VerifySnapshot = 21,
    ExpireSnapshot = 22,
    ScanIntegrity = 30,
    RepairIntegrity = 31,
    ExecuteGovernancePlan = 40,
    CreateLegalHold = 41,
    ReleaseLegalHold = 42,
    RotateKey = 50,
    RevokeKey = 51,
    DestroyKey = 52,
    UpdateCompatibility = 60,
    DrainRegion = 70,
    SetMaintenanceMode = 80,
    EmergencyStopWrites = 81,
    SuspendTenant = 90,
    SetTenantReadOnly = 91,
    RevokeDevice = 92,
    ForceRebootstrap = 93,
    BumpScopeGeneration = 94,
    CreateExport = 100,
    CreateIncidentBundle = 101,
    UpdateDynamicConfig = 110,
    RollbackDynamicConfig = 111,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[repr(u16)]
pub enum AdminReasonCode {
    PlannedMaintenance = 1,
    IncidentResponse = 2,
    CustomerRequest = 3,
    SecurityCompromise = 4,
    Migration = 5,
    GovernanceRequirement = 6,
    CapacityManagement = 7,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
pub enum AssuranceLevel {
    Normal,
    Mfa,
    HardwareBacked,
    BreakGlass,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
pub enum RiskClass {
    Routine,
    High,
    Destructive,
    Override,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct AdminPrincipal {
    pub principal_id: String,
    pub tenant_scope: Option<TenantId>,
    pub permissions: BTreeSet<PermissionId>,
    pub assurance: AssuranceLevel,
    pub authenticated_at_unix_ms: u64,
    pub expires_at_unix_ms: u64,
    pub break_glass_session_id: Option<BreakGlassSessionId>,
}

impl AdminPrincipal {
    pub fn validate(&self, now_unix_ms: u64) -> Result<(), AdminError> {
        if self.principal_id.trim().is_empty()
            || self.authenticated_at_unix_ms > now_unix_ms
            || now_unix_ms >= self.expires_at_unix_ms
        {
            return Err(AdminError::Unauthorized);
        }
        if self.assurance == AssuranceLevel::BreakGlass && self.break_glass_session_id.is_none() {
            return Err(AdminError::InvalidBreakGlassSession);
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct AdminTarget {
    pub kind: String,
    pub id: String,
    pub tenant_id: Option<TenantId>,
}

impl AdminTarget {
    pub fn validate(&self) -> Result<(), AdminError> {
        if self.kind.trim().is_empty()
            || self.kind.len() > 128
            || self.id.trim().is_empty()
            || self.id.len() > 1_024
        {
            return Err(AdminError::InvalidTarget);
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum MaintenanceMode {
    Normal,
    ReadOnly,
    Drain,
    Recovery,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ConfigGeneration(u64);

impl ConfigGeneration {
    pub const INITIAL: Self = Self(1);

    pub const fn new(value: u64) -> Result<Self, AdminError> {
        if value == 0 {
            Err(AdminError::InvalidGeneration)
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
pub enum DynamicPolicy {
    Admission {
        max_in_flight: u32,
        max_queue_depth: u32,
    },
    FeatureGate {
        feature_id: u32,
        enabled: bool,
    },
    Compatibility {
        policy_generation: u64,
        policy_digest: [u8; 32],
    },
    Maintenance(MaintenanceMode),
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum AdminCommand {
    PromoteAuthority {
        target: AdminTarget,
    },
    ForcePromoteAuthority {
        target: AdminTarget,
    },
    RetryJob {
        target: AdminTarget,
        job_id: JobId,
    },
    ReconcileJob {
        target: AdminTarget,
        job_id: JobId,
    },
    CancelJob {
        target: AdminTarget,
        job_id: JobId,
    },
    QuarantineJob {
        target: AdminTarget,
        job_id: JobId,
    },
    BuildSnapshot {
        target: AdminTarget,
    },
    VerifySnapshot {
        target: AdminTarget,
    },
    ExpireSnapshot {
        target: AdminTarget,
    },
    ScanIntegrity {
        target: AdminTarget,
    },
    RepairIntegrity {
        target: AdminTarget,
    },
    ExecuteGovernancePlan {
        target: AdminTarget,
    },
    CreateLegalHold {
        target: AdminTarget,
    },
    ReleaseLegalHold {
        target: AdminTarget,
    },
    RotateKey {
        target: AdminTarget,
    },
    RevokeKey {
        target: AdminTarget,
    },
    DestroyKey {
        target: AdminTarget,
    },
    UpdateCompatibility {
        target: AdminTarget,
        policy_digest: [u8; 32],
    },
    DrainRegion {
        target: AdminTarget,
    },
    SetMaintenanceMode {
        target: AdminTarget,
        mode: MaintenanceMode,
    },
    EmergencyStopWrites {
        target: AdminTarget,
    },
    SuspendTenant {
        target: AdminTarget,
    },
    SetTenantReadOnly {
        target: AdminTarget,
        read_only: bool,
    },
    RevokeDevice {
        target: AdminTarget,
    },
    ForceRebootstrap {
        target: AdminTarget,
    },
    BumpScopeGeneration {
        target: AdminTarget,
    },
    CreateExport {
        target: AdminTarget,
    },
    CreateIncidentBundle {
        target: AdminTarget,
    },
    UpdateDynamicConfig {
        target: AdminTarget,
        expected_generation: ConfigGeneration,
        policy: DynamicPolicy,
    },
    RollbackDynamicConfig {
        target: AdminTarget,
        expected_generation: ConfigGeneration,
        restore_generation: ConfigGeneration,
    },
}

impl AdminCommand {
    #[must_use]
    pub const fn kind(&self) -> AdminActionKind {
        match self {
            Self::PromoteAuthority { .. } => AdminActionKind::PromoteAuthority,
            Self::ForcePromoteAuthority { .. } => AdminActionKind::ForcePromoteAuthority,
            Self::RetryJob { .. } => AdminActionKind::RetryJob,
            Self::ReconcileJob { .. } => AdminActionKind::ReconcileJob,
            Self::CancelJob { .. } => AdminActionKind::CancelJob,
            Self::QuarantineJob { .. } => AdminActionKind::QuarantineJob,
            Self::BuildSnapshot { .. } => AdminActionKind::BuildSnapshot,
            Self::VerifySnapshot { .. } => AdminActionKind::VerifySnapshot,
            Self::ExpireSnapshot { .. } => AdminActionKind::ExpireSnapshot,
            Self::ScanIntegrity { .. } => AdminActionKind::ScanIntegrity,
            Self::RepairIntegrity { .. } => AdminActionKind::RepairIntegrity,
            Self::ExecuteGovernancePlan { .. } => AdminActionKind::ExecuteGovernancePlan,
            Self::CreateLegalHold { .. } => AdminActionKind::CreateLegalHold,
            Self::ReleaseLegalHold { .. } => AdminActionKind::ReleaseLegalHold,
            Self::RotateKey { .. } => AdminActionKind::RotateKey,
            Self::RevokeKey { .. } => AdminActionKind::RevokeKey,
            Self::DestroyKey { .. } => AdminActionKind::DestroyKey,
            Self::UpdateCompatibility { .. } => AdminActionKind::UpdateCompatibility,
            Self::DrainRegion { .. } => AdminActionKind::DrainRegion,
            Self::SetMaintenanceMode { .. } => AdminActionKind::SetMaintenanceMode,
            Self::EmergencyStopWrites { .. } => AdminActionKind::EmergencyStopWrites,
            Self::SuspendTenant { .. } => AdminActionKind::SuspendTenant,
            Self::SetTenantReadOnly { .. } => AdminActionKind::SetTenantReadOnly,
            Self::RevokeDevice { .. } => AdminActionKind::RevokeDevice,
            Self::ForceRebootstrap { .. } => AdminActionKind::ForceRebootstrap,
            Self::BumpScopeGeneration { .. } => AdminActionKind::BumpScopeGeneration,
            Self::CreateExport { .. } => AdminActionKind::CreateExport,
            Self::CreateIncidentBundle { .. } => AdminActionKind::CreateIncidentBundle,
            Self::UpdateDynamicConfig { .. } => AdminActionKind::UpdateDynamicConfig,
            Self::RollbackDynamicConfig { .. } => AdminActionKind::RollbackDynamicConfig,
        }
    }

    #[must_use]
    pub const fn target(&self) -> &AdminTarget {
        match self {
            Self::PromoteAuthority { target }
            | Self::ForcePromoteAuthority { target }
            | Self::RetryJob { target, .. }
            | Self::ReconcileJob { target, .. }
            | Self::CancelJob { target, .. }
            | Self::QuarantineJob { target, .. }
            | Self::BuildSnapshot { target }
            | Self::VerifySnapshot { target }
            | Self::ExpireSnapshot { target }
            | Self::ScanIntegrity { target }
            | Self::RepairIntegrity { target }
            | Self::ExecuteGovernancePlan { target }
            | Self::CreateLegalHold { target }
            | Self::ReleaseLegalHold { target }
            | Self::RotateKey { target }
            | Self::RevokeKey { target }
            | Self::DestroyKey { target }
            | Self::UpdateCompatibility { target, .. }
            | Self::DrainRegion { target }
            | Self::SetMaintenanceMode { target, .. }
            | Self::EmergencyStopWrites { target }
            | Self::SuspendTenant { target }
            | Self::SetTenantReadOnly { target, .. }
            | Self::RevokeDevice { target }
            | Self::ForceRebootstrap { target }
            | Self::BumpScopeGeneration { target }
            | Self::CreateExport { target }
            | Self::CreateIncidentBundle { target }
            | Self::UpdateDynamicConfig { target, .. }
            | Self::RollbackDynamicConfig { target, .. } => target,
        }
    }

    #[must_use]
    pub const fn risk(&self) -> RiskClass {
        match self {
            Self::ForcePromoteAuthority { .. } | Self::EmergencyStopWrites { .. } => {
                RiskClass::Override
            }
            Self::DestroyKey { .. }
            | Self::ReleaseLegalHold { .. }
            | Self::ExecuteGovernancePlan { .. }
            | Self::BumpScopeGeneration { .. }
            | Self::ExpireSnapshot { .. } => RiskClass::Destructive,
            Self::PromoteAuthority { .. }
            | Self::RepairIntegrity { .. }
            | Self::RotateKey { .. }
            | Self::RevokeKey { .. }
            | Self::UpdateCompatibility { .. }
            | Self::DrainRegion { .. }
            | Self::SetMaintenanceMode { .. }
            | Self::SuspendTenant { .. }
            | Self::RevokeDevice { .. }
            | Self::ForceRebootstrap { .. }
            | Self::UpdateDynamicConfig { .. }
            | Self::RollbackDynamicConfig { .. } => RiskClass::High,
            _ => RiskClass::Routine,
        }
    }

    #[must_use]
    pub const fn permission(&self) -> PermissionId {
        match self {
            Self::PromoteAuthority { .. } => PermissionId::AuthorityPromote,
            Self::ForcePromoteAuthority { .. } => PermissionId::AuthorityForcePromote,
            Self::RetryJob { .. } => PermissionId::JobsRetry,
            Self::ReconcileJob { .. } => PermissionId::JobsReconcile,
            Self::CancelJob { .. } => PermissionId::JobsCancel,
            Self::QuarantineJob { .. } => PermissionId::JobsQuarantine,
            Self::BuildSnapshot { .. } => PermissionId::SnapshotBuild,
            Self::VerifySnapshot { .. } => PermissionId::SnapshotVerify,
            Self::ExpireSnapshot { .. } => PermissionId::SnapshotExpire,
            Self::ScanIntegrity { .. } => PermissionId::IntegrityScan,
            Self::RepairIntegrity { .. } => PermissionId::IntegrityRepair,
            Self::ExecuteGovernancePlan { .. } => PermissionId::GovernanceExecute,
            Self::CreateLegalHold { .. } | Self::ReleaseLegalHold { .. } => {
                PermissionId::LegalHoldManage
            }
            Self::RotateKey { .. } => PermissionId::CryptoRotate,
            Self::RevokeKey { .. } => PermissionId::CryptoRevoke,
            Self::DestroyKey { .. } => PermissionId::CryptoDestroy,
            Self::UpdateCompatibility { .. } => PermissionId::CompatibilityUpdate,
            Self::DrainRegion { .. } => PermissionId::RegionDrain,
            Self::SetMaintenanceMode { .. } | Self::EmergencyStopWrites { .. } => {
                PermissionId::MaintenanceManage
            }
            Self::SuspendTenant { .. } | Self::SetTenantReadOnly { .. } => {
                PermissionId::TenantManage
            }
            Self::RevokeDevice { .. } | Self::ForceRebootstrap { .. } => PermissionId::DeviceManage,
            Self::BumpScopeGeneration { .. } => PermissionId::ScopeManage,
            Self::CreateExport { .. } => PermissionId::GovernancePlan,
            Self::CreateIncidentBundle { .. } => PermissionId::DiagnosticsExport,
            Self::UpdateDynamicConfig { .. } | Self::RollbackDynamicConfig { .. } => {
                PermissionId::ConfigUpdate
            }
        }
    }

    #[must_use]
    pub const fn required_assurance(&self) -> AssuranceLevel {
        match self.risk() {
            RiskClass::Routine => AssuranceLevel::Normal,
            RiskClass::High | RiskClass::Destructive => AssuranceLevel::Mfa,
            RiskClass::Override => AssuranceLevel::HardwareBacked,
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct SafetySnapshot {
    pub authority_epoch: AuthorityEpoch,
    pub policy_generation: ConfigGeneration,
    pub governance_generation: u64,
    pub target_version: u64,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct AdminPlan {
    pub plan_id: PlanId,
    pub action_digest: [u8; 32],
    pub safety_snapshot: SafetySnapshot,
    pub created_by: String,
    pub created_at_unix_ms: u64,
    pub expires_at_unix_ms: u64,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct PlanReference {
    pub plan_id: PlanId,
    pub action_digest: [u8; 32],
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct AdminAction {
    pub admin_operation_id: AdminOperationId,
    pub actor: AdminPrincipal,
    pub reason: AdminReasonCode,
    pub reason_note: Option<String>,
    pub command: AdminCommand,
    pub plan: Option<PlanReference>,
}

impl AdminAction {
    pub fn validate(&self, now_unix_ms: u64) -> Result<(), AdminError> {
        self.actor.validate(now_unix_ms)?;
        self.command.target().validate()?;
        if self
            .reason_note
            .as_ref()
            .is_some_and(|note| note.len() > MAX_REASON_NOTE_BYTES)
        {
            return Err(AdminError::ReasonTooLong);
        }
        if matches!(
            self.command.risk(),
            RiskClass::Destructive | RiskClass::Override
        ) && self
            .reason_note
            .as_ref()
            .is_none_or(|note| note.trim().is_empty())
        {
            return Err(AdminError::ReasonRequired);
        }
        Ok(())
    }

    pub fn digest(&self) -> Result<[u8; 32], AdminError> {
        let canonical = postcard::to_stdvec(&(
            self.admin_operation_id,
            &self.actor.principal_id,
            self.actor.tenant_scope,
            self.reason,
            &self.reason_note,
            &self.command,
            &self.plan,
        ))
        .map_err(|_| AdminError::CanonicalEncoding)?;
        Ok(*blake3::hash(&canonical).as_bytes())
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum AdminOperationStatus {
    Pending,
    AwaitingApproval,
    Approved,
    Executing,
    Completed,
    Rejected,
    Failed,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum ApprovalStatus {
    AwaitingApproval,
    Approved,
    Rejected,
    Expired,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct AdminApproval {
    pub approval_id: ApprovalId,
    pub admin_operation_id: AdminOperationId,
    pub requestor: String,
    pub approver: Option<String>,
    pub action_digest: [u8; 32],
    pub status: ApprovalStatus,
    pub expires_at_unix_ms: u64,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct AdminOperationRecord {
    pub action: AdminAction,
    pub payload_digest: [u8; 32],
    pub status: AdminOperationStatus,
    pub created_at_unix_ms: u64,
    pub completed_at_unix_ms: Option<u64>,
    pub result_code: Option<AdminResultCode>,
    pub job_id: Option<JobId>,
    pub approval_id: Option<ApprovalId>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum AdminResultCode {
    Applied,
    AcceptedAsJob,
    IdempotentReplay,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ExecutionResult {
    pub result_code: AdminResultCode,
    pub job_id: Option<JobId>,
    pub postcondition_verified: bool,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum AdminAuditEventKind {
    Requested,
    Denied,
    ApprovalRequested,
    Approved,
    Executing,
    Completed,
    Failed,
    BreakGlassUsed,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct AdminAuditEvent {
    pub admin_operation_id: AdminOperationId,
    pub actor_id: String,
    pub action_kind: AdminActionKind,
    pub event_kind: AdminAuditEventKind,
    pub at_unix_ms: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BeginOperation {
    Inserted,
    Existing,
}

pub trait AdminStore: Send + Sync {
    fn begin_operation(&self, record: AdminOperationRecord) -> Result<BeginOperation, AdminError>;
    fn operation(&self, id: AdminOperationId) -> Result<Option<AdminOperationRecord>, AdminError>;
    fn update_operation(&self, record: AdminOperationRecord) -> Result<(), AdminError>;
    fn put_plan(&self, plan: AdminPlan) -> Result<(), AdminError>;
    fn plan(&self, id: PlanId) -> Result<Option<AdminPlan>, AdminError>;
    fn put_approval(&self, approval: AdminApproval) -> Result<(), AdminError>;
    fn approval(&self, id: ApprovalId) -> Result<Option<AdminApproval>, AdminError>;
}

pub trait AdminCommandExecutor: Send + Sync {
    fn safety_snapshot(&self, command: &AdminCommand) -> Result<SafetySnapshot, AdminError>;
    fn execute(&self, action: &AdminAction) -> Result<ExecutionResult, AdminError>;
}

/// Durable Part 13 security-audit boundary. Success means the event is durably accepted.
pub trait AdminAuditSink: Send + Sync {
    fn record(&self, event: AdminAuditEvent) -> Result<(), AdminError>;
}

pub struct AdminService<S, E, A> {
    store: Arc<S>,
    executor: Arc<E>,
    audit: Arc<A>,
    approval_ttl_ms: u64,
    plan_ttl_ms: u64,
}

impl<S, E, A> AdminService<S, E, A>
where
    S: AdminStore,
    E: AdminCommandExecutor,
    A: AdminAuditSink,
{
    #[must_use]
    pub fn new(store: Arc<S>, executor: Arc<E>, audit: Arc<A>) -> Self {
        Self {
            store,
            executor,
            audit,
            approval_ttl_ms: 15 * 60 * 1_000,
            plan_ttl_ms: 15 * 60 * 1_000,
        }
    }

    pub fn plan(
        &self,
        actor: &AdminPrincipal,
        command: &AdminCommand,
        now_unix_ms: u64,
    ) -> Result<AdminPlan, AdminError> {
        actor.validate(now_unix_ms)?;
        authorize_at(actor, command, now_unix_ms)?;
        let action_digest = command_digest(command)?;
        let plan = AdminPlan {
            plan_id: PlanId::new(),
            action_digest,
            safety_snapshot: self.executor.safety_snapshot(command)?,
            created_by: actor.principal_id.clone(),
            created_at_unix_ms: now_unix_ms,
            expires_at_unix_ms: now_unix_ms.saturating_add(self.plan_ttl_ms),
        };
        self.store.put_plan(plan.clone())?;
        Ok(plan)
    }

    pub fn submit(
        &self,
        action: AdminAction,
        now_unix_ms: u64,
    ) -> Result<AdminOperationRecord, AdminError> {
        action.validate(now_unix_ms)?;
        if let Err(error) = authorize_at(&action.actor, &action.command, now_unix_ms) {
            self.audit_event(&action, AdminAuditEventKind::Denied, now_unix_ms)?;
            return Err(error);
        }
        self.validate_plan(&action, now_unix_ms)?;
        let operation_id = action.admin_operation_id;
        let digest = action.digest()?;
        let needs_approval = requires_approval(&action.command);
        let approval_id = needs_approval.then(ApprovalId::new);
        let record = AdminOperationRecord {
            action,
            payload_digest: digest,
            status: if needs_approval {
                AdminOperationStatus::AwaitingApproval
            } else {
                AdminOperationStatus::Pending
            },
            created_at_unix_ms: now_unix_ms,
            completed_at_unix_ms: None,
            result_code: None,
            job_id: None,
            approval_id,
        };
        match self.store.begin_operation(record.clone())? {
            BeginOperation::Existing => {
                let existing = self
                    .store
                    .operation(operation_id)?
                    .ok_or(AdminError::StoreContract)?;
                if existing.payload_digest != digest {
                    return Err(AdminError::IdempotencyPayloadMismatch);
                }
                return Ok(existing);
            }
            BeginOperation::Inserted => {}
        }
        self.audit_event(&record.action, AdminAuditEventKind::Requested, now_unix_ms)?;
        if record.action.actor.break_glass_session_id.is_some() {
            self.audit_event(
                &record.action,
                AdminAuditEventKind::BreakGlassUsed,
                now_unix_ms,
            )?;
        }
        if let Some(approval_id) = approval_id {
            self.store.put_approval(AdminApproval {
                approval_id,
                admin_operation_id: operation_id,
                requestor: record.action.actor.principal_id.clone(),
                approver: None,
                action_digest: digest,
                status: ApprovalStatus::AwaitingApproval,
                expires_at_unix_ms: now_unix_ms.saturating_add(self.approval_ttl_ms),
            })?;
            self.audit_event(
                &record.action,
                AdminAuditEventKind::ApprovalRequested,
                now_unix_ms,
            )?;
            Ok(record)
        } else {
            self.execute_record(record, now_unix_ms)
        }
    }

    pub fn approve(
        &self,
        operation_id: AdminOperationId,
        approver: &AdminPrincipal,
        now_unix_ms: u64,
    ) -> Result<AdminOperationRecord, AdminError> {
        approver.validate(now_unix_ms)?;
        let mut record = self
            .store
            .operation(operation_id)?
            .ok_or(AdminError::OperationNotFound)?;
        authorize_at(approver, &record.action.command, now_unix_ms)?;
        if approver.assurance < record.action.command.required_assurance() {
            return Err(AdminError::InsufficientAssurance);
        }
        let approval_id = record.approval_id.ok_or(AdminError::ApprovalNotRequired)?;
        let mut approval = self
            .store
            .approval(approval_id)?
            .ok_or(AdminError::ApprovalNotFound)?;
        if approval.requestor == approver.principal_id {
            return Err(AdminError::SeparationOfDuties);
        }
        if now_unix_ms >= approval.expires_at_unix_ms {
            approval.status = ApprovalStatus::Expired;
            self.store.put_approval(approval)?;
            return Err(AdminError::ApprovalExpired);
        }
        if approval.action_digest != record.payload_digest {
            return Err(AdminError::ApprovalDigestMismatch);
        }
        self.validate_plan(&record.action, now_unix_ms)?;
        approval.approver = Some(approver.principal_id.clone());
        approval.status = ApprovalStatus::Approved;
        self.store.put_approval(approval)?;
        record.status = AdminOperationStatus::Approved;
        self.store.update_operation(record.clone())?;
        self.audit_event(&record.action, AdminAuditEventKind::Approved, now_unix_ms)?;
        self.execute_record(record, now_unix_ms)
    }

    fn validate_plan(&self, action: &AdminAction, now_unix_ms: u64) -> Result<(), AdminError> {
        if !requires_plan(&action.command) {
            return Ok(());
        }
        let reference = action.plan.ok_or(AdminError::PlanRequired)?;
        let plan = self
            .store
            .plan(reference.plan_id)?
            .ok_or(AdminError::PlanNotFound)?;
        if now_unix_ms >= plan.expires_at_unix_ms {
            return Err(AdminError::PlanExpired);
        }
        let actual_digest = command_digest(&action.command)?;
        if reference.action_digest != plan.action_digest || plan.action_digest != actual_digest {
            return Err(AdminError::PlanDigestMismatch);
        }
        if self.executor.safety_snapshot(&action.command)? != plan.safety_snapshot {
            return Err(AdminError::PlanStale);
        }
        Ok(())
    }

    fn execute_record(
        &self,
        mut record: AdminOperationRecord,
        now_unix_ms: u64,
    ) -> Result<AdminOperationRecord, AdminError> {
        record.status = AdminOperationStatus::Executing;
        self.store.update_operation(record.clone())?;
        self.audit_event(&record.action, AdminAuditEventKind::Executing, now_unix_ms)?;
        let result = match self.executor.execute(&record.action) {
            Ok(result) => result,
            Err(error) => {
                record.status = AdminOperationStatus::Failed;
                self.store.update_operation(record.clone())?;
                self.audit_event(&record.action, AdminAuditEventKind::Failed, now_unix_ms)?;
                return Err(error);
            }
        };
        if record.action.command.risk() >= RiskClass::High
            && result.job_id.is_none()
            && !result.postcondition_verified
        {
            record.status = AdminOperationStatus::Failed;
            self.store.update_operation(record.clone())?;
            self.audit_event(&record.action, AdminAuditEventKind::Failed, now_unix_ms)?;
            return Err(AdminError::PostconditionNotVerified);
        }
        record.status = if result.job_id.is_some() {
            AdminOperationStatus::Executing
        } else {
            AdminOperationStatus::Completed
        };
        record.job_id = result.job_id;
        record.result_code = Some(result.result_code);
        record.completed_at_unix_ms =
            (record.status == AdminOperationStatus::Completed).then_some(now_unix_ms);
        self.store.update_operation(record.clone())?;
        if record.status == AdminOperationStatus::Completed {
            self.audit_event(&record.action, AdminAuditEventKind::Completed, now_unix_ms)?;
        }
        Ok(record)
    }

    fn audit_event(
        &self,
        action: &AdminAction,
        event_kind: AdminAuditEventKind,
        at_unix_ms: u64,
    ) -> Result<(), AdminError> {
        self.audit.record(AdminAuditEvent {
            admin_operation_id: action.admin_operation_id,
            actor_id: action.actor.principal_id.clone(),
            action_kind: action.command.kind(),
            event_kind,
            at_unix_ms,
        })
    }
}

fn command_digest(command: &AdminCommand) -> Result<[u8; 32], AdminError> {
    let bytes = postcard::to_stdvec(command).map_err(|_| AdminError::CanonicalEncoding)?;
    Ok(*blake3::hash(&bytes).as_bytes())
}

fn authorize(actor: &AdminPrincipal, command: &AdminCommand) -> Result<(), AdminError> {
    if !actor.permissions.contains(&command.permission()) {
        return Err(AdminError::Forbidden);
    }
    if actor.assurance < command.required_assurance()
        && actor.assurance != AssuranceLevel::BreakGlass
    {
        return Err(AdminError::InsufficientAssurance);
    }
    if let Some(scope) = actor.tenant_scope {
        if command.target().tenant_id != Some(scope) {
            return Err(AdminError::TenantScopeViolation);
        }
    }
    Ok(())
}

fn authorize_at(
    actor: &AdminPrincipal,
    command: &AdminCommand,
    now_unix_ms: u64,
) -> Result<(), AdminError> {
    authorize(actor, command)?;
    if command.risk() >= RiskClass::High
        && now_unix_ms.saturating_sub(actor.authenticated_at_unix_ms) > MAX_STEP_UP_AGE_MS
    {
        return Err(AdminError::StaleStepUpAuthentication);
    }
    Ok(())
}

#[must_use]
pub const fn requires_plan(command: &AdminCommand) -> bool {
    matches!(
        command.risk(),
        RiskClass::High | RiskClass::Destructive | RiskClass::Override
    )
}

#[must_use]
pub const fn requires_approval(command: &AdminCommand) -> bool {
    matches!(command.risk(), RiskClass::Destructive | RiskClass::Override)
}

#[derive(Default)]
pub struct InMemoryAdminStore {
    state: Mutex<InMemoryAdminState>,
}

#[derive(Default)]
struct InMemoryAdminState {
    operations: BTreeMap<AdminOperationId, AdminOperationRecord>,
    plans: BTreeMap<PlanId, AdminPlan>,
    approvals: BTreeMap<ApprovalId, AdminApproval>,
}

impl AdminStore for InMemoryAdminStore {
    fn begin_operation(&self, record: AdminOperationRecord) -> Result<BeginOperation, AdminError> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| AdminError::StoreUnavailable)?;
        if state
            .operations
            .contains_key(&record.action.admin_operation_id)
        {
            return Ok(BeginOperation::Existing);
        }
        state
            .operations
            .insert(record.action.admin_operation_id, record);
        Ok(BeginOperation::Inserted)
    }

    fn operation(&self, id: AdminOperationId) -> Result<Option<AdminOperationRecord>, AdminError> {
        Ok(self
            .state
            .lock()
            .map_err(|_| AdminError::StoreUnavailable)?
            .operations
            .get(&id)
            .cloned())
    }

    fn update_operation(&self, record: AdminOperationRecord) -> Result<(), AdminError> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| AdminError::StoreUnavailable)?;
        if !state
            .operations
            .contains_key(&record.action.admin_operation_id)
        {
            return Err(AdminError::OperationNotFound);
        }
        state
            .operations
            .insert(record.action.admin_operation_id, record);
        Ok(())
    }

    fn put_plan(&self, plan: AdminPlan) -> Result<(), AdminError> {
        self.state
            .lock()
            .map_err(|_| AdminError::StoreUnavailable)?
            .plans
            .insert(plan.plan_id, plan);
        Ok(())
    }

    fn plan(&self, id: PlanId) -> Result<Option<AdminPlan>, AdminError> {
        Ok(self
            .state
            .lock()
            .map_err(|_| AdminError::StoreUnavailable)?
            .plans
            .get(&id)
            .cloned())
    }

    fn put_approval(&self, approval: AdminApproval) -> Result<(), AdminError> {
        self.state
            .lock()
            .map_err(|_| AdminError::StoreUnavailable)?
            .approvals
            .insert(approval.approval_id, approval);
        Ok(())
    }

    fn approval(&self, id: ApprovalId) -> Result<Option<AdminApproval>, AdminError> {
        Ok(self
            .state
            .lock()
            .map_err(|_| AdminError::StoreUnavailable)?
            .approvals
            .get(&id)
            .cloned())
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct DynamicConfigRecord {
    pub generation: ConfigGeneration,
    pub policy: DynamicPolicy,
    pub digest: [u8; 32],
    pub created_by: String,
    pub created_at_unix_ms: u64,
}

pub struct DynamicConfigRegistry {
    state: Mutex<Vec<DynamicConfigRecord>>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct MaintenanceState {
    pub maintenance_id: MaintenanceId,
    pub mode: MaintenanceMode,
    pub generation: ConfigGeneration,
    pub reason: AdminReasonCode,
    pub effective_at_unix_ms: u64,
}

pub trait MaintenanceStateStore: Send + Sync {
    fn current(&self) -> Result<MaintenanceState, AdminError>;
    fn compare_and_swap(
        &self,
        expected_generation: ConfigGeneration,
        next: MaintenanceState,
    ) -> Result<MaintenanceState, AdminError>;
}

pub struct InMemoryMaintenanceStateStore {
    state: Mutex<MaintenanceState>,
}

impl InMemoryMaintenanceStateStore {
    #[must_use]
    pub const fn new(initial: MaintenanceState) -> Self {
        Self {
            state: Mutex::new(initial),
        }
    }
}

impl MaintenanceStateStore for InMemoryMaintenanceStateStore {
    fn current(&self) -> Result<MaintenanceState, AdminError> {
        self.state
            .lock()
            .map(|state| *state)
            .map_err(|_| AdminError::StoreUnavailable)
    }

    fn compare_and_swap(
        &self,
        expected_generation: ConfigGeneration,
        mut next: MaintenanceState,
    ) -> Result<MaintenanceState, AdminError> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| AdminError::StoreUnavailable)?;
        if state.generation != expected_generation {
            return Err(AdminError::ConfigGenerationConflict);
        }
        next.generation = expected_generation
            .next()
            .ok_or(AdminError::GenerationExhausted)?;
        *state = next;
        Ok(next)
    }
}

impl DynamicConfigRegistry {
    #[must_use]
    pub fn new(initial: DynamicConfigRecord) -> Self {
        Self {
            state: Mutex::new(vec![initial]),
        }
    }

    pub fn current(&self) -> Result<DynamicConfigRecord, AdminError> {
        self.state
            .lock()
            .map_err(|_| AdminError::StoreUnavailable)?
            .last()
            .cloned()
            .ok_or(AdminError::StoreContract)
    }

    pub fn compare_and_swap(
        &self,
        expected: ConfigGeneration,
        policy: DynamicPolicy,
        actor: String,
        now_unix_ms: u64,
    ) -> Result<DynamicConfigRecord, AdminError> {
        let encoded = postcard::to_stdvec(&policy).map_err(|_| AdminError::CanonicalEncoding)?;
        if encoded.len() > MAX_DYNAMIC_POLICY_BYTES {
            return Err(AdminError::DynamicPolicyTooLarge);
        }
        let mut state = self
            .state
            .lock()
            .map_err(|_| AdminError::StoreUnavailable)?;
        let current = state.last().ok_or(AdminError::StoreContract)?;
        if current.generation != expected {
            return Err(AdminError::ConfigGenerationConflict);
        }
        let generation = expected.next().ok_or(AdminError::GenerationExhausted)?;
        let record = DynamicConfigRecord {
            generation,
            policy,
            digest: *blake3::hash(&encoded).as_bytes(),
            created_by: actor,
            created_at_unix_ms: now_unix_ms,
        };
        state.push(record.clone());
        Ok(record)
    }

    pub fn rollback(
        &self,
        expected: ConfigGeneration,
        restore: ConfigGeneration,
        actor: String,
        now_unix_ms: u64,
    ) -> Result<DynamicConfigRecord, AdminError> {
        let policy = {
            let state = self
                .state
                .lock()
                .map_err(|_| AdminError::StoreUnavailable)?;
            let current = state.last().ok_or(AdminError::StoreContract)?;
            if current.generation != expected {
                return Err(AdminError::ConfigGenerationConflict);
            }
            state
                .iter()
                .find(|record| record.generation == restore)
                .map(|record| record.policy.clone())
                .ok_or(AdminError::ConfigGenerationNotFound)?
        };
        self.compare_and_swap(expected, policy, actor, now_unix_ms)
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum AdminBindScope {
    Loopback,
    PrivateNetwork,
    Public,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum AdminListenerState {
    Disabled,
    Enabled,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum AdminAuthConfiguration {
    Unconfigured,
    StrongAuthAndPermissionRegistry,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum DestructiveActionPolicy {
    Disabled,
    Enabled,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum BrowserSessionPolicy {
    NoCookieAuthentication,
    CookieWithCsrfProtection,
    CookieWithoutCsrfProtection,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct AdminListenerPolicy {
    pub state: AdminListenerState,
    pub bind_scope: AdminBindScope,
    pub auth: AdminAuthConfiguration,
    pub destructive_actions: DestructiveActionPolicy,
    pub allowed_origins: BTreeSet<String>,
    pub browser_sessions: BrowserSessionPolicy,
}

impl Default for AdminListenerPolicy {
    fn default() -> Self {
        Self {
            state: AdminListenerState::Disabled,
            bind_scope: AdminBindScope::Loopback,
            auth: AdminAuthConfiguration::Unconfigured,
            destructive_actions: DestructiveActionPolicy::Disabled,
            allowed_origins: BTreeSet::new(),
            browser_sessions: BrowserSessionPolicy::NoCookieAuthentication,
        }
    }
}

impl AdminListenerPolicy {
    pub fn validate(&self) -> Result<(), AdminError> {
        if self.state == AdminListenerState::Disabled {
            return Ok(());
        }
        if self.auth != AdminAuthConfiguration::StrongAuthAndPermissionRegistry {
            return Err(AdminError::AdminAuthMisconfigured);
        }
        if self.bind_scope == AdminBindScope::Public {
            return Err(AdminError::PublicAdminListenerForbidden);
        }
        if self.browser_sessions == BrowserSessionPolicy::CookieWithoutCsrfProtection {
            return Err(AdminError::CsrfProtectionRequired);
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct AdminCapabilities {
    pub api_version: u16,
    pub action_kinds: BTreeSet<AdminActionKind>,
    pub wire_formats: BTreeSet<AdminWireFormat>,
    pub destructive_actions_enabled: bool,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
pub enum AdminWireFormat {
    Postcard,
    Json,
    Ron,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum AdminQuery {
    Capabilities,
    AuthorityStatus,
    Jobs { tenant_id: Option<TenantId> },
    Snapshots { tenant_id: Option<TenantId> },
    IntegrityStatus { tenant_id: Option<TenantId> },
    GovernanceStatus { tenant_id: Option<TenantId> },
    CompatibilityStatus,
    CryptoKeys,
    Regions,
    DeepHealth,
    RuntimeDiagnostics,
    Operation { target: AdminTarget },
}

impl AdminQuery {
    #[must_use]
    pub const fn permission(&self) -> PermissionId {
        match self {
            Self::Capabilities | Self::RuntimeDiagnostics | Self::DeepHealth => {
                PermissionId::DiagnosticsView
            }
            Self::AuthorityStatus => PermissionId::AuthorityView,
            Self::Jobs { .. } => PermissionId::JobsView,
            Self::Snapshots { .. } => PermissionId::SnapshotView,
            Self::IntegrityStatus { .. } => PermissionId::IntegrityScan,
            Self::GovernanceStatus { .. } => PermissionId::GovernancePlan,
            Self::CompatibilityStatus => PermissionId::CompatibilityView,
            Self::CryptoKeys => PermissionId::CryptoView,
            Self::Regions => PermissionId::RegionView,
            Self::Operation { .. } => PermissionId::OperationsView,
        }
    }

    #[must_use]
    pub const fn tenant_id(&self) -> Option<TenantId> {
        match self {
            Self::Jobs { tenant_id }
            | Self::Snapshots { tenant_id }
            | Self::IntegrityStatus { tenant_id }
            | Self::GovernanceStatus { tenant_id } => *tenant_id,
            Self::Operation { target } => target.tenant_id,
            _ => None,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct AuthorityStatusView {
    pub authority_id: String,
    pub epoch: AuthorityEpoch,
    pub role: String,
    pub fence: u64,
    pub current_sequence: u64,
    pub promotion_ready: bool,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct JobStatusView {
    pub job_id: JobId,
    pub kind: u32,
    pub state: String,
    pub attempts: u32,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct SnapshotStatusView {
    pub snapshot_id: String,
    pub state: String,
    pub root_digest: [u8; 32],
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct CryptoKeyView {
    pub key_id: String,
    pub purpose: String,
    pub status: String,
    pub public_key: Option<Vec<u8>>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct RegionStatusView {
    pub region_id: String,
    pub state: String,
    pub watermark: u64,
    pub config_generation: ConfigGeneration,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct HealthCheckView {
    pub component: String,
    pub healthy: bool,
    pub reason_code: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum AdminView {
    Capabilities(AdminCapabilities),
    Authority(AuthorityStatusView),
    Jobs(Vec<JobStatusView>),
    Snapshots(Vec<SnapshotStatusView>),
    Integrity { healthy: bool, generation: u64 },
    Governance { generation: u64, blocked: bool },
    Compatibility { generation: u64, digest: [u8; 32] },
    CryptoKeys(Vec<CryptoKeyView>),
    Regions(Vec<RegionStatusView>),
    Health(Vec<HealthCheckView>),
    RuntimeDiagnostics(BTreeMap<String, u64>),
    Operation(Box<AdminOperationRecord>),
}

pub trait AdminReadProvider: Send + Sync {
    fn read(&self, query: &AdminQuery) -> Result<AdminView, AdminError>;
}

pub struct AdminReadService<P> {
    provider: Arc<P>,
}

impl<P: AdminReadProvider> AdminReadService<P> {
    #[must_use]
    pub const fn new(provider: Arc<P>) -> Self {
        Self { provider }
    }

    pub fn read(
        &self,
        actor: &AdminPrincipal,
        query: &AdminQuery,
        now_unix_ms: u64,
    ) -> Result<AdminView, AdminError> {
        actor.validate(now_unix_ms)?;
        if !actor.permissions.contains(&query.permission()) {
            return Err(AdminError::Forbidden);
        }
        if let Some(scope) = actor.tenant_scope {
            if query.tenant_id() != Some(scope) {
                return Err(AdminError::TenantScopeViolation);
            }
        }
        self.provider.read(query)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AdminErrorCode {
    Unauthorized,
    Forbidden,
    ApprovalRequired,
    PlanExpired,
    PlanStale,
    UnsafeOperation,
    AuthorityNotPrimary,
    JobNotRetryable,
    LegalHoldBlocks,
    FleetIncompatible,
    Conflict,
    InvalidAction,
    DependencyUnavailable,
}

#[derive(Debug, Error, Clone, Eq, PartialEq)]
pub enum AdminError {
    #[error("admin authentication is missing, invalid, or expired")]
    Unauthorized,
    #[error("admin principal lacks the required permission")]
    Forbidden,
    #[error("admin session assurance is insufficient")]
    InsufficientAssurance,
    #[error("step-up authentication is too old for this high-risk action")]
    StaleStepUpAuthentication,
    #[error("break-glass assurance requires a tracked session")]
    InvalidBreakGlassSession,
    #[error("admin target is invalid")]
    InvalidTarget,
    #[error("admin reason note exceeds the allowed bound")]
    ReasonTooLong,
    #[error("a non-empty human reason is required for this action")]
    ReasonRequired,
    #[error("admin principal tenant scope does not include the target")]
    TenantScopeViolation,
    #[error("admin action could not be canonically encoded")]
    CanonicalEncoding,
    #[error("the same AdminOperationId was reused with a different payload")]
    IdempotencyPayloadMismatch,
    #[error("a reviewed plan is required")]
    PlanRequired,
    #[error("admin plan was not found")]
    PlanNotFound,
    #[error("admin plan has expired")]
    PlanExpired,
    #[error("admin plan digest does not match the exact command")]
    PlanDigestMismatch,
    #[error("safety-critical state changed after the plan was reviewed")]
    PlanStale,
    #[error("admin operation was not found")]
    OperationNotFound,
    #[error("admin approval was not found")]
    ApprovalNotFound,
    #[error("this operation does not require approval")]
    ApprovalNotRequired,
    #[error("requestor cannot approve their own high-risk operation")]
    SeparationOfDuties,
    #[error("admin approval has expired")]
    ApprovalExpired,
    #[error("admin approval does not bind the operation digest")]
    ApprovalDigestMismatch,
    #[error("a high-risk action did not verify its postcondition")]
    PostconditionNotVerified,
    #[error("dynamic configuration generation must be non-zero")]
    InvalidGeneration,
    #[error("dynamic configuration generation was exhausted")]
    GenerationExhausted,
    #[error("dynamic configuration update lost a compare-and-swap race")]
    ConfigGenerationConflict,
    #[error("requested dynamic configuration generation was not found")]
    ConfigGenerationNotFound,
    #[error("dynamic policy exceeds its encoded size bound")]
    DynamicPolicyTooLarge,
    #[error("admin authentication or permission registry is misconfigured")]
    AdminAuthMisconfigured,
    #[error("the reusable control plane forbids a public admin listener")]
    PublicAdminListenerForbidden,
    #[error("cookie-authenticated admin UI requires CSRF protection")]
    CsrfProtectionRequired,
    #[error("admin durable store is unavailable")]
    StoreUnavailable,
    #[error("admin store violated its required contract")]
    StoreContract,
    #[error("subsystem guard rejected an unsafe operation")]
    UnsafeOperation,
    #[error("admin mutation must be routed to the active authority")]
    AuthorityNotPrimary,
    #[error("job kind is not safely retryable; reconciliation is required")]
    JobNotRetryable,
    #[error("active legal hold blocks this governance action")]
    LegalHoldBlocks,
    #[error("fleet capabilities do not permit this policy activation")]
    FleetIncompatible,
}

impl AdminError {
    #[must_use]
    pub const fn code(&self) -> AdminErrorCode {
        match self {
            Self::Unauthorized | Self::InvalidBreakGlassSession => AdminErrorCode::Unauthorized,
            Self::Forbidden
            | Self::InsufficientAssurance
            | Self::StaleStepUpAuthentication
            | Self::TenantScopeViolation
            | Self::SeparationOfDuties => AdminErrorCode::Forbidden,
            Self::ApprovalNotFound
            | Self::ApprovalNotRequired
            | Self::ApprovalExpired
            | Self::ApprovalDigestMismatch => AdminErrorCode::ApprovalRequired,
            Self::PlanExpired => AdminErrorCode::PlanExpired,
            Self::PlanStale => AdminErrorCode::PlanStale,
            Self::UnsafeOperation | Self::PostconditionNotVerified => {
                AdminErrorCode::UnsafeOperation
            }
            Self::AuthorityNotPrimary => AdminErrorCode::AuthorityNotPrimary,
            Self::JobNotRetryable => AdminErrorCode::JobNotRetryable,
            Self::LegalHoldBlocks => AdminErrorCode::LegalHoldBlocks,
            Self::FleetIncompatible => AdminErrorCode::FleetIncompatible,
            Self::IdempotencyPayloadMismatch
            | Self::ConfigGenerationConflict
            | Self::OperationNotFound
            | Self::ConfigGenerationNotFound => AdminErrorCode::Conflict,
            Self::StoreUnavailable | Self::StoreContract => AdminErrorCode::DependencyUnavailable,
            _ => AdminErrorCode::InvalidAction,
        }
    }

    #[must_use]
    pub const fn http_status(&self) -> u16 {
        match self.code() {
            AdminErrorCode::Unauthorized => 401,
            AdminErrorCode::Forbidden => 403,
            AdminErrorCode::Conflict
            | AdminErrorCode::AuthorityNotPrimary
            | AdminErrorCode::JobNotRetryable
            | AdminErrorCode::LegalHoldBlocks
            | AdminErrorCode::FleetIncompatible => 409,
            AdminErrorCode::ApprovalRequired
            | AdminErrorCode::PlanExpired
            | AdminErrorCode::PlanStale => 412,
            AdminErrorCode::InvalidAction | AdminErrorCode::UnsafeOperation => 422,
            AdminErrorCode::DependencyUnavailable => 503,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    struct Executor {
        generation: AtomicU64,
        executions: AtomicU64,
        verified: bool,
    }

    impl Executor {
        fn new(verified: bool) -> Self {
            Self {
                generation: AtomicU64::new(1),
                executions: AtomicU64::new(0),
                verified,
            }
        }
    }

    impl AdminCommandExecutor for Executor {
        fn safety_snapshot(&self, _command: &AdminCommand) -> Result<SafetySnapshot, AdminError> {
            Ok(SafetySnapshot {
                authority_epoch: AuthorityEpoch::INITIAL,
                policy_generation: ConfigGeneration::new(self.generation.load(Ordering::SeqCst))?,
                governance_generation: 1,
                target_version: 1,
            })
        }

        fn execute(&self, _action: &AdminAction) -> Result<ExecutionResult, AdminError> {
            self.executions.fetch_add(1, Ordering::SeqCst);
            Ok(ExecutionResult {
                result_code: AdminResultCode::Applied,
                job_id: None,
                postcondition_verified: self.verified,
            })
        }
    }

    #[derive(Default)]
    struct Audit(Mutex<Vec<AdminAuditEvent>>);

    impl AdminAuditSink for Audit {
        fn record(&self, event: AdminAuditEvent) -> Result<(), AdminError> {
            self.0
                .lock()
                .map_err(|_| AdminError::StoreUnavailable)?
                .push(event);
            Ok(())
        }
    }

    fn principal(id: &str, permission: PermissionId, assurance: AssuranceLevel) -> AdminPrincipal {
        AdminPrincipal {
            principal_id: id.to_owned(),
            tenant_scope: None,
            permissions: BTreeSet::from([permission]),
            assurance,
            authenticated_at_unix_ms: 1,
            expires_at_unix_ms: 10_000,
            break_glass_session_id: None,
        }
    }

    fn target() -> AdminTarget {
        AdminTarget {
            kind: "cluster".to_owned(),
            id: "primary".to_owned(),
            tenant_id: None,
        }
    }

    fn action(id: AdminOperationId, actor: AdminPrincipal, command: AdminCommand) -> AdminAction {
        AdminAction {
            admin_operation_id: id,
            actor,
            reason: AdminReasonCode::PlannedMaintenance,
            reason_note: Some("approved maintenance window".to_owned()),
            command,
            plan: None,
        }
    }

    #[test]
    fn authorization_and_tenant_scope_fail_closed() {
        let mut actor = principal("support", PermissionId::JobsView, AssuranceLevel::Normal);
        let command = AdminCommand::RetryJob {
            target: target(),
            job_id: JobId::new(),
        };
        assert_eq!(authorize(&actor, &command), Err(AdminError::Forbidden));

        actor.permissions.insert(PermissionId::JobsRetry);
        actor.tenant_scope = Some(TenantId::new());
        assert_eq!(
            authorize(&actor, &command),
            Err(AdminError::TenantScopeViolation)
        );
    }

    #[test]
    fn idempotency_replays_same_action_and_rejects_payload_drift() {
        let store = Arc::new(InMemoryAdminStore::default());
        let executor = Arc::new(Executor::new(true));
        let service = AdminService::new(store, executor.clone(), Arc::new(Audit::default()));
        let id = AdminOperationId::new();
        let actor = principal("sre", PermissionId::JobsRetry, AssuranceLevel::Normal);
        let original = action(
            id,
            actor.clone(),
            AdminCommand::RetryJob {
                target: target(),
                job_id: JobId::new(),
            },
        );
        assert_eq!(
            service.submit(original.clone(), 100).map(|r| r.status),
            Ok(AdminOperationStatus::Completed)
        );
        assert_eq!(
            service.submit(original, 101).map(|r| r.status),
            Ok(AdminOperationStatus::Completed)
        );
        assert_eq!(executor.executions.load(Ordering::SeqCst), 1);

        let changed = action(
            id,
            actor,
            AdminCommand::RetryJob {
                target: target(),
                job_id: JobId::new(),
            },
        );
        assert_eq!(
            service.submit(changed, 102),
            Err(AdminError::IdempotencyPayloadMismatch)
        );
    }

    #[test]
    fn stale_plan_and_unverified_high_risk_action_are_rejected() {
        let store = Arc::new(InMemoryAdminStore::default());
        let executor = Arc::new(Executor::new(false));
        let service = AdminService::new(store, executor.clone(), Arc::new(Audit::default()));
        let actor = principal("sre", PermissionId::RegionDrain, AssuranceLevel::Mfa);
        let command = AdminCommand::DrainRegion { target: target() };
        let plan = service
            .plan(&actor, &command, 100)
            .unwrap_or_else(|error| panic!("{error}"));
        executor.generation.store(2, Ordering::SeqCst);
        let mut request = action(AdminOperationId::new(), actor.clone(), command.clone());
        request.plan = Some(PlanReference {
            plan_id: plan.plan_id,
            action_digest: plan.action_digest,
        });
        assert_eq!(service.submit(request, 101), Err(AdminError::PlanStale));

        let fresh = service
            .plan(&actor, &command, 102)
            .unwrap_or_else(|error| panic!("{error}"));
        let mut request = action(AdminOperationId::new(), actor, command);
        request.plan = Some(PlanReference {
            plan_id: fresh.plan_id,
            action_digest: fresh.action_digest,
        });
        assert_eq!(
            service.submit(request, 103),
            Err(AdminError::PostconditionNotVerified)
        );
    }

    #[test]
    fn destructive_action_requires_exact_plan_and_second_person_approval() {
        let store = Arc::new(InMemoryAdminStore::default());
        let executor = Arc::new(Executor::new(true));
        let audit = Arc::new(Audit::default());
        let service = AdminService::new(store, executor.clone(), audit.clone());
        let requester = principal(
            "security-a",
            PermissionId::CryptoDestroy,
            AssuranceLevel::Mfa,
        );
        let command = AdminCommand::DestroyKey { target: target() };
        let plan = service
            .plan(&requester, &command, 100)
            .unwrap_or_else(|error| panic!("{error}"));
        let id = AdminOperationId::new();
        let mut request = action(id, requester.clone(), command);
        request.plan = Some(PlanReference {
            plan_id: plan.plan_id,
            action_digest: plan.action_digest,
        });
        let pending = service
            .submit(request, 101)
            .unwrap_or_else(|error| panic!("{error}"));
        assert_eq!(pending.status, AdminOperationStatus::AwaitingApproval);
        assert_eq!(
            service.approve(id, &requester, 102),
            Err(AdminError::SeparationOfDuties)
        );
        let approver = principal(
            "security-b",
            PermissionId::CryptoDestroy,
            AssuranceLevel::Mfa,
        );
        let complete = service
            .approve(id, &approver, 103)
            .unwrap_or_else(|error| panic!("{error}"));
        assert_eq!(complete.status, AdminOperationStatus::Completed);
        assert_eq!(executor.executions.load(Ordering::SeqCst), 1);
        assert!(audit.0.lock().is_ok_and(|events| events.len() >= 5));
    }

    #[test]
    fn approval_rechecks_plan_after_safety_state_changes() {
        let store = Arc::new(InMemoryAdminStore::default());
        let executor = Arc::new(Executor::new(true));
        let service = AdminService::new(store, executor.clone(), Arc::new(Audit::default()));
        let requester = principal(
            "security-a",
            PermissionId::CryptoDestroy,
            AssuranceLevel::Mfa,
        );
        let command = AdminCommand::DestroyKey { target: target() };
        let plan = service
            .plan(&requester, &command, 100)
            .unwrap_or_else(|error| panic!("{error}"));
        let id = AdminOperationId::new();
        let mut request = action(id, requester, command);
        request.plan = Some(PlanReference {
            plan_id: plan.plan_id,
            action_digest: plan.action_digest,
        });
        service
            .submit(request, 101)
            .unwrap_or_else(|error| panic!("{error}"));
        executor.generation.store(2, Ordering::SeqCst);
        let approver = principal(
            "security-b",
            PermissionId::CryptoDestroy,
            AssuranceLevel::Mfa,
        );
        assert_eq!(
            service.approve(id, &approver, 102),
            Err(AdminError::PlanStale)
        );
        assert_eq!(executor.executions.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn config_updates_use_generation_compare_and_swap_and_rollback() {
        let initial_policy = DynamicPolicy::Admission {
            max_in_flight: 10,
            max_queue_depth: 20,
        };
        let bytes = postcard::to_stdvec(&initial_policy).unwrap_or_else(|error| panic!("{error}"));
        let registry = DynamicConfigRegistry::new(DynamicConfigRecord {
            generation: ConfigGeneration::INITIAL,
            policy: initial_policy,
            digest: *blake3::hash(&bytes).as_bytes(),
            created_by: "bootstrap".to_owned(),
            created_at_unix_ms: 1,
        });
        let second = registry
            .compare_and_swap(
                ConfigGeneration::INITIAL,
                DynamicPolicy::Maintenance(MaintenanceMode::ReadOnly),
                "sre".to_owned(),
                2,
            )
            .unwrap_or_else(|error| panic!("{error}"));
        assert_eq!(second.generation.get(), 2);
        assert_eq!(
            registry.compare_and_swap(
                ConfigGeneration::INITIAL,
                DynamicPolicy::Maintenance(MaintenanceMode::Drain),
                "other".to_owned(),
                3,
            ),
            Err(AdminError::ConfigGenerationConflict)
        );
        let rolled_back = registry
            .rollback(
                second.generation,
                ConfigGeneration::INITIAL,
                "sre".to_owned(),
                4,
            )
            .unwrap_or_else(|error| panic!("{error}"));
        assert_eq!(
            registry.current().map(|record| record.policy),
            Ok(rolled_back.policy)
        );
    }

    #[test]
    fn listener_defaults_disabled_and_validation_fails_closed() {
        let policy = AdminListenerPolicy::default();
        assert_eq!(policy.state, AdminListenerState::Disabled);
        assert!(policy.validate().is_ok());
        let public = AdminListenerPolicy {
            state: AdminListenerState::Enabled,
            bind_scope: AdminBindScope::Public,
            auth: AdminAuthConfiguration::StrongAuthAndPermissionRegistry,
            ..policy
        };
        assert_eq!(
            public.validate(),
            Err(AdminError::PublicAdminListenerForbidden)
        );
    }

    #[test]
    fn maintenance_state_is_durable_generation_cas() {
        let initial = MaintenanceState {
            maintenance_id: MaintenanceId::new(),
            mode: MaintenanceMode::Normal,
            generation: ConfigGeneration::INITIAL,
            reason: AdminReasonCode::PlannedMaintenance,
            effective_at_unix_ms: 1,
        };
        let store = InMemoryMaintenanceStateStore::new(initial);
        let next = MaintenanceState {
            maintenance_id: MaintenanceId::new(),
            mode: MaintenanceMode::Drain,
            generation: ConfigGeneration::INITIAL,
            reason: AdminReasonCode::PlannedMaintenance,
            effective_at_unix_ms: 2,
        };
        let applied = store
            .compare_and_swap(ConfigGeneration::INITIAL, next)
            .unwrap_or_else(|error| panic!("{error}"));
        assert_eq!(applied.generation.get(), 2);
        assert_eq!(
            store.compare_and_swap(ConfigGeneration::INITIAL, next),
            Err(AdminError::ConfigGenerationConflict)
        );
    }

    #[test]
    fn error_codes_are_stable_and_map_policy_failures_without_generic_500() {
        assert_eq!(AdminError::Unauthorized.http_status(), 401);
        assert_eq!(AdminError::Forbidden.http_status(), 403);
        assert_eq!(AdminError::PlanStale.http_status(), 412);
        assert_eq!(AdminError::JobNotRetryable.http_status(), 409);
        assert_eq!(AdminError::UnsafeOperation.http_status(), 422);
        assert_eq!(AdminError::StoreUnavailable.http_status(), 503);
    }
}
