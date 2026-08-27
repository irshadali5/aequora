//! Fault-injecting reference sink for large-bootstrap adapter certification.

use aequora_bootstrap::{
    ActivationOutcome, BootstrapJob, BootstrapStoreError, BootstrapStoreErrorKind, ChunkDescriptor,
    PendingIntentPlan, ReplicaGeneration, SnapshotManifest, SnapshotSink, VerifiedActivation,
};
use aequora_protocol::SnapshotEntity;
use aequora_types::{EntityRef, Sequence};
use async_trait::async_trait;
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::{Mutex, MutexGuard},
};

/// One-shot crash/response-loss point in the reference adapter.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BootstrapFailpoint {
    AfterRecordsBeforeProgress,
    AfterChunkCommitBeforeResponse,
    BeforeActivationCommit,
    AfterActivationCommitBeforeResponse,
}

#[derive(Clone, Debug, Default)]
struct StagingState {
    records: BTreeMap<EntityRef, SnapshotEntity>,
    installed_chunks: BTreeSet<u32>,
    verified: bool,
    pending_count: usize,
    pending_digest: [u8; 32],
}

#[derive(Debug)]
struct SinkState {
    active_generation: ReplicaGeneration,
    cursor: Sequence,
    staging: BTreeMap<ReplicaGeneration, StagingState>,
    failpoint: Option<BootstrapFailpoint>,
}

impl Default for SinkState {
    fn default() -> Self {
        Self {
            active_generation: ReplicaGeneration::new(1).unwrap_or_else(|error| panic!("{error}")),
            cursor: Sequence(0),
            staging: BTreeMap::new(),
            failpoint: None,
        }
    }
}

/// Database-free generation sink with transaction rollback and lost-response simulation.
#[derive(Debug, Default)]
pub struct FaultInjectingSnapshotSink(Mutex<SinkState>);

impl FaultInjectingSnapshotSink {
    pub fn fail_once(&self, failpoint: BootstrapFailpoint) {
        lock(&self.0).failpoint = Some(failpoint);
    }

    #[must_use]
    pub fn active_generation(&self) -> ReplicaGeneration {
        lock(&self.0).active_generation
    }

    #[must_use]
    pub fn cursor(&self) -> Sequence {
        lock(&self.0).cursor
    }

    #[must_use]
    pub fn staged_record_count(&self, generation: ReplicaGeneration) -> usize {
        lock(&self.0)
            .staging
            .get(&generation)
            .map_or(0, |staging| staging.records.len())
    }

    #[must_use]
    pub fn installed_chunk_count(&self, generation: ReplicaGeneration) -> usize {
        lock(&self.0)
            .staging
            .get(&generation)
            .map_or(0, |staging| staging.installed_chunks.len())
    }
}

#[async_trait]
impl SnapshotSink for FaultInjectingSnapshotSink {
    async fn begin_staging(
        &self,
        job: &BootstrapJob,
        manifest: &SnapshotManifest,
        pending: &PendingIntentPlan,
    ) -> Result<(), BootstrapStoreError> {
        job.validate_resume(manifest).map_err(validation)?;
        if pending.digest != job.pending_intent_digest {
            return Err(permanent("pending intent changed before staging"));
        }
        let mut state = lock(&self.0);
        let candidate = StagingState {
            pending_count: pending.operation_ids.len(),
            pending_digest: pending.digest,
            ..StagingState::default()
        };
        if let Some(existing) = state.staging.get(&job.staging_generation) {
            if existing.pending_digest != candidate.pending_digest
                || existing.pending_count != candidate.pending_count
            {
                return Err(permanent("staging generation identity changed"));
            }
            return Ok(());
        }
        state.staging.insert(job.staging_generation, candidate);
        Ok(())
    }

    async fn install_chunk(
        &self,
        job: &BootstrapJob,
        descriptor: &ChunkDescriptor,
        records: &[SnapshotEntity],
    ) -> Result<(), BootstrapStoreError> {
        let mut state = lock(&self.0);
        let failpoint = state.failpoint.take();
        let staging = state
            .staging
            .get(&job.staging_generation)
            .ok_or_else(|| permanent("staging generation is absent"))?;
        if staging.installed_chunks.contains(&descriptor.ordinal) {
            let exact = records
                .iter()
                .all(|record| staging.records.get(&record.entity) == Some(record));
            return if exact {
                Ok(())
            } else {
                Err(permanent("installed chunk content changed"))
            };
        }

        let mut candidate = staging.clone();
        for record in records {
            if let Some(existing) = candidate.records.insert(record.entity, record.clone()) {
                if existing != *record {
                    return Err(permanent("entity content changed across chunks"));
                }
            }
        }
        if matches!(
            failpoint,
            Some(BootstrapFailpoint::AfterRecordsBeforeProgress)
        ) {
            return Err(transient("transaction rolled back before chunk progress"));
        }
        candidate.installed_chunks.insert(descriptor.ordinal);
        state.staging.insert(job.staging_generation, candidate);
        if matches!(
            failpoint,
            Some(BootstrapFailpoint::AfterChunkCommitBeforeResponse)
        ) {
            return Err(transient("chunk response lost after commit"));
        }
        Ok(())
    }

    async fn verify_staging(
        &self,
        job: &BootstrapJob,
        manifest: &SnapshotManifest,
    ) -> Result<(), BootstrapStoreError> {
        let mut state = lock(&self.0);
        let staging = state
            .staging
            .get_mut(&job.staging_generation)
            .ok_or_else(|| permanent("staging generation is absent"))?;
        if staging.installed_chunks.len() != manifest.chunks.len()
            || staging.records.len()
                != usize::try_from(manifest.record_count)
                    .map_err(|_| permanent("record count overflow"))?
        {
            return Err(permanent("staging generation is incomplete"));
        }
        staging.verified = true;
        Ok(())
    }

    async fn activate(
        &self,
        activation: VerifiedActivation,
    ) -> Result<ActivationOutcome, BootstrapStoreError> {
        let mut state = lock(&self.0);
        if state.active_generation == activation.staging_generation
            && state.cursor == activation.cursor_sequence
        {
            let staging = state
                .staging
                .get(&activation.staging_generation)
                .ok_or_else(|| permanent("active generation metadata is absent"))?;
            return Ok(outcome(activation, staging.pending_count));
        }
        let failpoint = state.failpoint.take();
        if matches!(failpoint, Some(BootstrapFailpoint::BeforeActivationCommit)) {
            return Err(transient("activation transaction failed before commit"));
        }
        let staging = state
            .staging
            .get(&activation.staging_generation)
            .ok_or_else(|| permanent("staging generation is absent"))?;
        if !staging.verified || staging.pending_digest != activation.pending_intent_digest {
            return Err(permanent("staging or pending intent is unverified"));
        }
        let pending_count = staging.pending_count;
        state.active_generation = activation.staging_generation;
        state.cursor = activation.cursor_sequence;
        if matches!(
            failpoint,
            Some(BootstrapFailpoint::AfterActivationCommitBeforeResponse)
        ) {
            return Err(transient("activation response lost after commit"));
        }
        Ok(outcome(activation, pending_count))
    }

    async fn quarantine_revoked(&self, job: &BootstrapJob) -> Result<(), BootstrapStoreError> {
        lock(&self.0).staging.remove(&job.staging_generation);
        Ok(())
    }
}

fn outcome(activation: VerifiedActivation, pending_count: usize) -> ActivationOutcome {
    ActivationOutcome {
        active_generation: activation.staging_generation,
        cursor_sequence: activation.cursor_sequence,
        preserved_pending_operations: pending_count,
    }
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

#[allow(clippy::needless_pass_by_value)]
fn validation(error: aequora_bootstrap::BootstrapError) -> BootstrapStoreError {
    permanent(&error.to_string())
}

fn transient(message: &str) -> BootstrapStoreError {
    BootstrapStoreError {
        kind: BootstrapStoreErrorKind::Transient,
        message: message.to_owned(),
    }
}

fn permanent(message: &str) -> BootstrapStoreError {
    BootstrapStoreError {
        kind: BootstrapStoreErrorKind::Permanent,
        message: message.to_owned(),
    }
}
