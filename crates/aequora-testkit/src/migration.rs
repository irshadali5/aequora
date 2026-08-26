//! Deterministic checkpointed-import adapter and failpoint compliance fixture.

use std::{
    collections::BTreeMap,
    sync::{Mutex, MutexGuard},
};

use aequora_migration::{
    BatchCommitOutcome, CheckpointedImportSink, ImportBatch, ImportCheckpoint, ImportJob,
    ImportWorkflowError, ImportWorkflowStoreError, QuarantineEntry, SourceRecordKey,
};
use aequora_types::EntityId;
use async_trait::async_trait;

/// Transaction failpoint used to prove checkpoint/data atomicity and lost-response replay.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ImportFailpoint {
    BeforeCommit,
    AfterDataBeforeCheckpoint,
    AfterCommitBeforeResponse,
}

#[derive(Debug, Default)]
struct ImportState {
    checkpoint: ImportCheckpoint,
    records: BTreeMap<SourceRecordKey, (EntityId, [u8; 32])>,
    quarantine: BTreeMap<SourceRecordKey, QuarantineEntry>,
    failpoint: Option<ImportFailpoint>,
}

/// Database-free reference sink with transaction-like rollback and response-loss injection.
#[derive(Debug, Default)]
pub struct FaultInjectingImportSink(Mutex<ImportState>);

impl FaultInjectingImportSink {
    pub fn fail_once(&self, failpoint: ImportFailpoint) {
        lock(&self.0).failpoint = Some(failpoint);
    }

    #[must_use]
    pub fn checkpoint(&self) -> ImportCheckpoint {
        lock(&self.0).checkpoint.clone()
    }

    #[must_use]
    pub fn record_count(&self) -> usize {
        lock(&self.0).records.len()
    }

    #[must_use]
    pub fn quarantine_count(&self) -> usize {
        lock(&self.0).quarantine.len()
    }
}

#[async_trait]
impl CheckpointedImportSink for FaultInjectingImportSink {
    async fn commit_batch_and_checkpoint(
        &self,
        _job: &ImportJob,
        expected: &ImportCheckpoint,
        batch: &ImportBatch,
    ) -> Result<BatchCommitOutcome, ImportWorkflowStoreError> {
        let mut state = lock(&self.0);
        if state.checkpoint == batch.next_checkpoint {
            let exact = batch.records.iter().all(|record| {
                state.records.get(&record.source_key) == Some(&(record.target_id, record.checksum))
            });
            return if exact {
                Ok(BatchCommitOutcome::Duplicate)
            } else {
                Err(permanent("committed batch identity changed"))
            };
        }
        if &state.checkpoint != expected {
            return Err(permanent("checkpoint compare-and-set failed"));
        }
        expected
            .validate_successor(&batch.next_checkpoint)
            .map_err(workflow_error)?;
        let failpoint = state.failpoint.take();
        if matches!(failpoint, Some(ImportFailpoint::BeforeCommit)) {
            return Err(transient("failed before transaction commit"));
        }
        let mut candidate_records = state.records.clone();
        for record in &batch.records {
            if let Some(existing) = candidate_records.get(&record.source_key) {
                if existing != &(record.target_id, record.checksum) {
                    return Err(permanent("source record checksum or identity changed"));
                }
            } else {
                candidate_records.insert(
                    record.source_key.clone(),
                    (record.target_id, record.checksum),
                );
            }
        }
        if matches!(failpoint, Some(ImportFailpoint::AfterDataBeforeCheckpoint)) {
            return Err(transient("transaction rolled back before checkpoint"));
        }
        state.records = candidate_records;
        state.checkpoint = batch.next_checkpoint.clone();
        if matches!(failpoint, Some(ImportFailpoint::AfterCommitBeforeResponse)) {
            return Err(transient("response lost after commit"));
        }
        Ok(BatchCommitOutcome::Applied)
    }

    async fn quarantine(&self, entry: &QuarantineEntry) -> Result<(), ImportWorkflowStoreError> {
        let mut state = lock(&self.0);
        if let Some(existing) = state.quarantine.get(&entry.source_key) {
            if existing != entry {
                return Err(permanent("quarantine identity changed"));
            }
            return Ok(());
        }
        state
            .quarantine
            .insert(entry.source_key.clone(), entry.clone());
        Ok(())
    }
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

fn workflow_error(error: ImportWorkflowError) -> ImportWorkflowStoreError {
    permanent(&error.to_string())
}

fn transient(message: &str) -> ImportWorkflowStoreError {
    ImportWorkflowStoreError {
        transient: true,
        message: message.to_owned(),
    }
}

fn permanent(message: &str) -> ImportWorkflowStoreError {
    ImportWorkflowStoreError {
        transient: false,
        message: message.to_owned(),
    }
}
