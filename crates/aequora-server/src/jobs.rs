//! Server-facing polling, bounded dispatch, status, and admin contracts for durable work.

#![allow(clippy::missing_errors_doc)]

use aequora_jobs::{
    AdminAction, ClaimRequest, ClaimedJob, ConcurrencyClass, JobError, JobKind, JobRegistry,
    JobStatus, JobStore, JobStoreError, MAX_CLAIM_BATCH, WorkerCapability, WorkerId,
};
use aequora_types::{JobId, RegionId, TenantId};
use async_trait::async_trait;
use std::collections::{BTreeMap, BTreeSet};
use thiserror::Error;

#[derive(Clone, Debug)]
pub struct JobWorkerConfig {
    pub worker_id: WorkerId,
    pub capabilities: BTreeSet<WorkerCapability>,
    pub allowed_regions: BTreeSet<RegionId>,
    pub lease_duration_ms: u64,
    pub maximum_claim_batch: usize,
    pub class_concurrency: BTreeMap<ConcurrencyClass, usize>,
    pub idle_backoff_ms: u64,
    pub failure_backoff_ms: u64,
}
impl JobWorkerConfig {
    pub fn verify(&self) -> Result<(), JobServerError> {
        if self.lease_duration_ms == 0
            || self.maximum_claim_batch == 0
            || self.maximum_claim_batch > MAX_CLAIM_BATCH
            || self.idle_backoff_ms == 0
            || self.failure_backoff_ms == 0
            || self.class_concurrency.values().any(|limit| *limit == 0)
        {
            return Err(JobServerError::InvalidWorkerConfig);
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum WorkerShutdown {
    #[default]
    Running,
    StopClaiming,
    Forced,
}

/// Pure planner that never treats a wake notification as durable state.
pub struct JobWorkerPlanner<'a> {
    registry: &'a JobRegistry,
    config: &'a JobWorkerConfig,
}
impl<'a> JobWorkerPlanner<'a> {
    pub fn new(
        registry: &'a JobRegistry,
        config: &'a JobWorkerConfig,
    ) -> Result<Self, JobServerError> {
        config.verify()?;
        Ok(Self { registry, config })
    }

    #[must_use]
    pub fn claim_request(
        &self,
        now_unix_ms: u64,
        available_permits: usize,
    ) -> Option<ClaimRequest> {
        let limit = available_permits.min(self.config.maximum_claim_batch);
        (limit > 0).then(|| ClaimRequest {
            worker_id: self.config.worker_id,
            supported_kinds: self.registry.supported_kinds(),
            capabilities: self.config.capabilities.clone(),
            allowed_regions: self.config.allowed_regions.clone(),
            now_unix_ms,
            lease_duration_ms: self.config.lease_duration_ms,
            limit,
        })
    }

    pub fn verify_claims(&self, claims: &[ClaimedJob]) -> Result<(), JobServerError> {
        if claims.len() > self.config.maximum_claim_batch {
            return Err(JobServerError::AdapterExceededClaimLimit);
        }
        for claim in claims {
            let descriptor = self
                .registry
                .descriptor(claim.job.kind)
                .ok_or(JobServerError::UnknownClaimedKind(claim.job.kind))?;
            if !descriptor
                .required_capabilities
                .is_subset(&self.config.capabilities)
            {
                return Err(JobServerError::MissingCapability);
            }
            if claim.lease.worker_id != self.config.worker_id {
                return Err(JobServerError::ForeignLease);
            }
        }
        Ok(())
    }
}

#[async_trait]
pub trait JobAccessPolicy: Send + Sync {
    async fn can_view(&self, tenant_id: TenantId, job_id: JobId) -> bool;
    async fn can_admin(&self, tenant_id: TenantId, job_id: JobId, action: AdminAction) -> bool;
}

pub struct JobAdminService<S, P> {
    store: S,
    policy: P,
}
impl<S, P> JobAdminService<S, P> {
    #[must_use]
    pub const fn new(store: S, policy: P) -> Self {
        Self { store, policy }
    }
}
impl<S, P> JobAdminService<S, P>
where
    S: JobStore,
    P: JobAccessPolicy,
{
    pub async fn status(
        &self,
        tenant_id: TenantId,
        job_id: JobId,
    ) -> Result<JobStatus, JobServerError> {
        if !self.policy.can_view(tenant_id, job_id).await {
            return Err(JobServerError::Forbidden);
        }
        Ok(self.store.status(job_id, tenant_id).await?)
    }

    pub async fn apply(
        &self,
        tenant_id: TenantId,
        job_id: JobId,
        action: AdminAction,
        reason_code: u32,
    ) -> Result<(), JobServerError> {
        if !self.policy.can_admin(tenant_id, job_id, action).await {
            return Err(JobServerError::Forbidden);
        }
        self.store
            .admin_transition(job_id, tenant_id, action, reason_code)
            .await?;
        Ok(())
    }
}

#[derive(Debug, Error)]
pub enum JobServerError {
    #[error("job worker configuration is invalid")]
    InvalidWorkerConfig,
    #[error("job store returned more work than the bounded claim request")]
    AdapterExceededClaimLimit,
    #[error("job store returned unknown kind {0:?}")]
    UnknownClaimedKind(JobKind),
    #[error("worker lacks a capability required by the claimed job")]
    MissingCapability,
    #[error("job store returned a lease owned by another worker")]
    ForeignLease,
    #[error("job action is not authorized")]
    Forbidden,
    #[error("job store failed: {0}")]
    Store(#[from] JobStoreError),
    #[error("job contract failed validation: {0}")]
    Contract(#[from] JobError),
}
