//! Deterministic, durable workflow planning contracts.
//!
//! Workflow transition logic is pure. External observations enter as persisted events, and every
//! resulting job or compensation is persisted before a worker may act on it.

#![allow(clippy::missing_errors_doc)]

use aequora_jobs::{JobKind, WorkflowId};
use aequora_types::{CorrelationId, JobId, TenantId};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use thiserror::Error;

pub const MAX_WORKFLOW_STATE_BYTES: usize = 256 * 1024;
pub const MAX_TRANSITION_ACTIONS: usize = 1_024;
pub const MAX_WORKFLOW_CHILDREN: usize = 10_000;

#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct WorkflowKind(u32);
impl WorkflowKind {
    pub const fn new(value: u32) -> Result<Self, WorkflowError> {
        if value == 0 {
            Err(WorkflowError::ZeroIdentity)
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
pub struct WorkflowVersion(u32);
impl WorkflowVersion {
    pub const fn new(value: u32) -> Result<Self, WorkflowError> {
        if value == 0 {
            Err(WorkflowError::ZeroIdentity)
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
pub enum WorkflowState {
    Running,
    Waiting,
    Compensating,
    Completed,
    Failed,
    Canceled,
    ManualIntervention,
}
impl WorkflowState {
    #[must_use]
    pub const fn is_terminal(self) -> bool {
        matches!(
            self,
            Self::Completed | Self::Failed | Self::Canceled | Self::ManualIntervention
        )
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct WorkflowCheckpoint {
    pub phase: u32,
    pub state_bytes: Vec<u8>,
    pub completed_steps: BTreeSet<u32>,
    pub scheduled_compensations: BTreeSet<u32>,
}
impl WorkflowCheckpoint {
    pub fn verify(&self) -> Result<(), WorkflowError> {
        if self.state_bytes.len() > MAX_WORKFLOW_STATE_BYTES {
            Err(WorkflowError::StateTooLarge)
        } else {
            Ok(())
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct WorkflowRecord {
    pub workflow_id: WorkflowId,
    pub tenant_id: TenantId,
    pub kind: WorkflowKind,
    pub version: WorkflowVersion,
    pub state: WorkflowState,
    pub checkpoint: WorkflowCheckpoint,
    pub correlation_id: CorrelationId,
    pub row_version: u64,
    pub child_count: u32,
    pub created_at_unix_ms: u64,
    pub updated_at_unix_ms: u64,
}
impl WorkflowRecord {
    pub fn verify(&self) -> Result<(), WorkflowError> {
        self.checkpoint.verify()?;
        if self.row_version == 0 || self.updated_at_unix_ms < self.created_at_unix_ms {
            return Err(WorkflowError::InvalidRecord);
        }
        if usize::try_from(self.child_count).map_or(true, |count| count > MAX_WORKFLOW_CHILDREN) {
            return Err(WorkflowError::ChildLimit);
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum WorkflowEvent {
    Started,
    JobCompleted {
        job_id: JobId,
        result_digest: Option<[u8; 32]>,
    },
    JobFailed {
        job_id: JobId,
        error_code: u32,
    },
    TimerFired {
        timer_id: u32,
    },
    ExternalResult {
        kind: u32,
        payload: Vec<u8>,
    },
    Approval {
        principal_ref: String,
        approved: bool,
    },
    CancelRequested {
        reason_code: u32,
    },
}
impl WorkflowEvent {
    pub fn verify(&self) -> Result<(), WorkflowError> {
        match self {
            Self::ExternalResult { payload, .. } if payload.len() > MAX_WORKFLOW_STATE_BYTES => {
                Err(WorkflowError::EventTooLarge)
            }
            Self::Approval { principal_ref, .. }
                if principal_ref.is_empty() || principal_ref.len() > 512 =>
            {
                Err(WorkflowError::InvalidEvent)
            }
            _ => Ok(()),
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum WorkflowAction {
    ScheduleJob {
        step: u32,
        kind: JobKind,
        payload: Vec<u8>,
    },
    ScheduleCompensation {
        for_step: u32,
        kind: JobKind,
        payload: Vec<u8>,
    },
    WaitUntil {
        unix_ms: u64,
    },
    WaitForApproval,
    EmitAuthoritativeOperation {
        operation_kind: u32,
        payload: Vec<u8>,
    },
    Complete,
    Fail {
        error_code: u32,
    },
    RequireManualIntervention {
        reason_code: u32,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WorkflowTransition {
    pub expected_row_version: u64,
    pub next_state: WorkflowState,
    pub checkpoint: WorkflowCheckpoint,
    pub actions: Vec<WorkflowAction>,
}
impl WorkflowTransition {
    pub fn verify(&self, current: &WorkflowRecord) -> Result<(), WorkflowError> {
        self.checkpoint.verify()?;
        if self.expected_row_version != current.row_version {
            return Err(WorkflowError::StaleVersion);
        }
        if current.state.is_terminal() {
            return Err(WorkflowError::TerminalWorkflow);
        }
        if self.actions.len() > MAX_TRANSITION_ACTIONS {
            return Err(WorkflowError::ActionLimit);
        }
        let new_children = self
            .actions
            .iter()
            .filter(|action| {
                matches!(
                    action,
                    WorkflowAction::ScheduleJob { .. }
                        | WorkflowAction::ScheduleCompensation { .. }
                )
            })
            .count();
        let total = usize::try_from(current.child_count)
            .unwrap_or(usize::MAX)
            .saturating_add(new_children);
        if total > MAX_WORKFLOW_CHILDREN {
            return Err(WorkflowError::ChildLimit);
        }
        for action in &self.actions {
            match action {
                WorkflowAction::ScheduleJob { payload, .. }
                | WorkflowAction::ScheduleCompensation { payload, .. }
                | WorkflowAction::EmitAuthoritativeOperation { payload, .. }
                    if payload.len() > MAX_WORKFLOW_STATE_BYTES =>
                {
                    return Err(WorkflowError::ActionPayloadTooLarge);
                }
                _ => {}
            }
        }
        Ok(())
    }
}

pub trait WorkflowDefinition: Send + Sync {
    fn kind(&self) -> WorkflowKind;
    fn version(&self) -> WorkflowVersion;
    fn next(
        &self,
        current: &WorkflowRecord,
        event: &WorkflowEvent,
    ) -> Result<WorkflowTransition, WorkflowError>;
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum WorkflowVersionDisposition {
    Continue,
    Migrate {
        target: WorkflowVersion,
        state_bytes: Vec<u8>,
    },
    ManualIntervention {
        reason_code: u32,
    },
    Terminate {
        reason_code: u32,
    },
}

#[async_trait]
pub trait WorkflowStore: Send + Sync {
    async fn create(&self, workflow: WorkflowRecord) -> Result<(), WorkflowStoreError>;
    async fn load(
        &self,
        workflow_id: WorkflowId,
        tenant_id: TenantId,
    ) -> Result<WorkflowRecord, WorkflowStoreError>;
    async fn apply(
        &self,
        workflow_id: WorkflowId,
        tenant_id: TenantId,
        event: WorkflowEvent,
        transition: WorkflowTransition,
    ) -> Result<WorkflowRecord, WorkflowStoreError>;
}

/// Atomic persistence boundary used when a transition schedules jobs or compensations.
#[async_trait]
pub trait WorkflowTransaction: Send {
    async fn update_workflow(
        &mut self,
        workflow_id: WorkflowId,
        tenant_id: TenantId,
        event: WorkflowEvent,
        transition: WorkflowTransition,
    ) -> Result<(), WorkflowStoreError>;
    async fn insert_child_job(
        &mut self,
        job: aequora_jobs::NewJob,
    ) -> Result<(), WorkflowStoreError>;
    async fn commit(self) -> Result<(), WorkflowStoreError>
    where
        Self: Sized;
}

#[derive(Clone, Debug, Error, Eq, PartialEq)]
pub enum WorkflowStoreError {
    #[error("workflow already exists")]
    Duplicate,
    #[error("workflow was not found in this tenant")]
    NotFound,
    #[error("workflow row version is stale")]
    StaleVersion,
    #[error("workflow storage is temporarily unavailable")]
    Unavailable,
    #[error("workflow storage rejected invalid data: {0}")]
    Invalid(WorkflowError),
    #[error("workflow storage adapter failure: {0}")]
    Adapter(String),
}

#[derive(Clone, Copy, Debug, Error, Eq, PartialEq)]
pub enum WorkflowError {
    #[error("persistent numeric identity must be non-zero")]
    ZeroIdentity,
    #[error("workflow checkpoint state exceeds its hard bound")]
    StateTooLarge,
    #[error("workflow record is invalid")]
    InvalidRecord,
    #[error("workflow event exceeds its hard bound")]
    EventTooLarge,
    #[error("workflow event is invalid")]
    InvalidEvent,
    #[error("workflow transition has a stale row version")]
    StaleVersion,
    #[error("terminal workflow cannot transition implicitly")]
    TerminalWorkflow,
    #[error("workflow transition action count exceeds its hard bound")]
    ActionLimit,
    #[error("workflow child count exceeds its hard bound")]
    ChildLimit,
    #[error("workflow action payload exceeds its hard bound")]
    ActionPayloadTooLarge,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn current() -> WorkflowRecord {
        WorkflowRecord {
            workflow_id: WorkflowId::new(),
            tenant_id: TenantId::new(),
            kind: WorkflowKind::new(1).unwrap_or_else(|error| panic!("{error}")),
            version: WorkflowVersion::new(1).unwrap_or_else(|error| panic!("{error}")),
            state: WorkflowState::Running,
            checkpoint: WorkflowCheckpoint {
                phase: 1,
                state_bytes: Vec::new(),
                completed_steps: BTreeSet::new(),
                scheduled_compensations: BTreeSet::new(),
            },
            correlation_id: CorrelationId::new(),
            row_version: 1,
            child_count: 0,
            created_at_unix_ms: 1,
            updated_at_unix_ms: 1,
        }
    }

    #[test]
    fn stale_transition_is_rejected() {
        let workflow = current();
        let transition = WorkflowTransition {
            expected_row_version: 2,
            next_state: WorkflowState::Waiting,
            checkpoint: workflow.checkpoint.clone(),
            actions: vec![WorkflowAction::WaitForApproval],
        };
        assert_eq!(
            transition.verify(&workflow),
            Err(WorkflowError::StaleVersion)
        );
    }

    #[test]
    fn compensation_is_an_explicit_durable_action() {
        let workflow = current();
        let transition = WorkflowTransition {
            expected_row_version: 1,
            next_state: WorkflowState::Compensating,
            checkpoint: workflow.checkpoint.clone(),
            actions: vec![WorkflowAction::ScheduleCompensation {
                for_step: 1,
                kind: JobKind::new(2).unwrap_or_else(|error| panic!("{error}")),
                payload: vec![1],
            }],
        };
        assert!(transition.verify(&workflow).is_ok());
    }
}
