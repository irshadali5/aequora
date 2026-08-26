use std::collections::BTreeMap;

use aequora_migration::{
    CheckpointedImportSink, ImportBatch, ImportCheckpoint, ImportJob, ImportJobState,
    MigrationMode, PreparedImportRecord, QuarantineEntry, QuarantineStatus, SourceFingerprint,
    SourceRecordKey, SourceSystemId,
};
use aequora_schema::{CanonicalEntityId, CanonicalRecord, CanonicalValue};
use aequora_testkit::migration::{FaultInjectingImportSink, ImportFailpoint};
use aequora_types::EntityId;

fn key(value: &str) -> SourceRecordKey {
    SourceRecordKey::new(value).unwrap_or_else(|error| panic!("{error}"))
}

fn fixture() -> (ImportJob, ImportBatch) {
    let source = SourceSystemId::new("legacy-fixture").unwrap_or_else(|error| panic!("{error}"));
    let fingerprint = SourceFingerprint::calculate(&source, 1, None, b"immutable")
        .unwrap_or_else(|error| panic!("{error}"));
    let mut job = ImportJob::new(MigrationMode::SeedAuthority, source, fingerprint, 1, 1, 1)
        .unwrap_or_else(|error| panic!("{error}"));
    job.transition(ImportJobState::Scanning, 2)
        .unwrap_or_else(|error| panic!("{error}"));
    job.transition(ImportJobState::Transforming, 3)
        .unwrap_or_else(|error| panic!("{error}"));
    job.transition(ImportJobState::Importing, 4)
        .unwrap_or_else(|error| panic!("{error}"));
    let source_key = key("student/1");
    let checksum = *blake3::hash(b"student/1:ada").as_bytes();
    let record = PreparedImportRecord {
        source_key: source_key.clone(),
        target_id: EntityId::new(),
        checksum,
        record: CanonicalRecord {
            entity: CanonicalEntityId::new(1).unwrap_or_else(|error| panic!("{error}")),
            key: CanonicalValue::Text(source_key.as_str().to_owned()),
            fields: BTreeMap::new(),
        },
        dependencies: Vec::new(),
        scopes: Vec::new(),
    };
    let batch = ImportBatch {
        records: vec![record],
        next_checkpoint: ImportCheckpoint {
            committed_batches: 1,
            committed_records: 1,
            source_offset: 1,
            last_source_key: Some(source_key),
            watermark: None,
        },
    };
    (job, batch)
}

#[tokio::test]
async fn checkpointed_sink_rolls_back_replays_and_quarantines_durably() {
    let sink = FaultInjectingImportSink::default();
    let (job, batch) = fixture();
    let initial = ImportCheckpoint::default();

    sink.fail_once(ImportFailpoint::AfterDataBeforeCheckpoint);
    assert!(
        sink.commit_batch_and_checkpoint(&job, &initial, &batch)
            .await
            .is_err()
    );
    assert_eq!(sink.record_count(), 0);
    assert_eq!(sink.checkpoint(), initial);

    sink.fail_once(ImportFailpoint::AfterCommitBeforeResponse);
    assert!(
        sink.commit_batch_and_checkpoint(&job, &initial, &batch)
            .await
            .is_err()
    );
    assert_eq!(sink.record_count(), 1);
    assert_eq!(sink.checkpoint(), batch.next_checkpoint);
    assert_eq!(
        sink.commit_batch_and_checkpoint(&job, &initial, &batch)
            .await,
        Ok(aequora_migration::BatchCommitOutcome::Duplicate)
    );
    assert_eq!(sink.record_count(), 1);

    let quarantine = QuarantineEntry {
        job_id: job.job_id,
        source_key: key("broken/2"),
        entity: CanonicalEntityId::new(1).unwrap_or_else(|error| panic!("{error}")),
        error_code: "AEQ-IMP-REF-001".to_owned(),
        source_checksum: *blake3::hash(b"broken/2").as_bytes(),
        status: QuarantineStatus::Open,
        resolution: None,
    };
    assert!(sink.quarantine(&quarantine).await.is_ok());
    assert!(sink.quarantine(&quarantine).await.is_ok());
    assert_eq!(sink.quarantine_count(), 1);
}
