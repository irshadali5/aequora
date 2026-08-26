use aequora_bootstrap::{
    ActivationEvidence, AuthorityEpoch, BootstrapJob, BootstrapMutationPolicy, BootstrapState,
    ChunkState, ChunkingConfig, PendingIntentPlan, ProjectionSchemaVersion, ReplicaGeneration,
    SnapshotBoundary, SnapshotLease, SnapshotManifest, SnapshotSink, verify_activation,
};
use aequora_protocol::SnapshotEntity;
use aequora_scope::{ScopeGeneration, ScopeVersion};
use aequora_testkit::large_bootstrap::{BootstrapFailpoint, FaultInjectingSnapshotSink};
use aequora_types::{
    EntityId, EntityRef, EntityType, EntityVersion, OperationId, Sequence, SnapshotId, SyncScopeId,
};
use std::collections::BTreeSet;
use uuid::Uuid;

fn entity(id: u128) -> SnapshotEntity {
    SnapshotEntity {
        entity: EntityRef {
            entity_type: EntityType::new(1).unwrap_or_else(|error| panic!("{error}")),
            entity_id: EntityId::from_uuid(Uuid::from_u128(id)),
        },
        version: EntityVersion::INITIAL,
        payload: vec![u8::try_from(id).unwrap_or(0); 8],
        tombstone: false,
    }
}

#[tokio::test]
#[allow(clippy::too_many_lines)]
async fn staging_and_activation_are_atomic_replayable_and_preserve_pending_intent() {
    let boundary = SnapshotBoundary {
        scope_id: SyncScopeId::from_uuid(Uuid::from_u128(9)),
        scope_version: ScopeVersion::INITIAL,
        scope_generation: ScopeGeneration::INITIAL,
        sequence: Sequence(88),
        authority_epoch: AuthorityEpoch::new(1).unwrap_or_else(|error| panic!("{error}")),
    };
    let built = SnapshotManifest::build(
        SnapshotId::from_uuid(Uuid::from_u128(10)),
        boundary,
        ProjectionSchemaVersion::new(1).unwrap_or_else(|error| panic!("{error}")),
        &[entity(1), entity(2), entity(3)],
        ChunkingConfig {
            target_records: 2,
            target_uncompressed_bytes: 1_024,
            max_chunk_uncompressed_bytes: 4_096,
            max_chunks: 8,
            max_records: 8,
            max_total_uncompressed_bytes: 16_384,
        },
    )
    .unwrap_or_else(|error| panic!("{error}"));
    let pending_id = OperationId::from_uuid(Uuid::from_u128(99));
    let pending = PendingIntentPlan::build(vec![pending_id], BTreeSet::from([pending_id]))
        .unwrap_or_else(|error| panic!("{error}"));
    let mut job = BootstrapJob::new(
        &built.manifest,
        ReplicaGeneration::new(2).unwrap_or_else(|error| panic!("{error}")),
        BootstrapMutationPolicy::AllowQueue,
        &pending,
    )
    .unwrap_or_else(|error| panic!("{error}"));
    let sink = FaultInjectingSnapshotSink::default();
    sink.begin_staging(&job, &built.manifest, &pending)
        .await
        .unwrap_or_else(|error| panic!("{error}"));

    let first_records = built.chunks[0]
        .decode_verified(&built.chunks[0].descriptor, 4_096)
        .unwrap_or_else(|error| panic!("{error}"));
    sink.fail_once(BootstrapFailpoint::AfterRecordsBeforeProgress);
    assert!(
        sink.install_chunk(&job, &built.chunks[0].descriptor, &first_records)
            .await
            .is_err()
    );
    assert_eq!(sink.staged_record_count(job.staging_generation), 0);
    assert_eq!(sink.installed_chunk_count(job.staging_generation), 0);

    sink.fail_once(BootstrapFailpoint::AfterChunkCommitBeforeResponse);
    assert!(
        sink.install_chunk(&job, &built.chunks[0].descriptor, &first_records)
            .await
            .is_err()
    );
    sink.install_chunk(&job, &built.chunks[0].descriptor, &first_records)
        .await
        .unwrap_or_else(|error| panic!("{error}"));
    job.chunks
        .get_mut(&built.chunks[0].descriptor.ordinal)
        .unwrap_or_else(|| panic!("missing progress"))
        .state = ChunkState::Installed;

    for chunk in built.chunks.iter().skip(1) {
        let records = chunk
            .decode_verified(&chunk.descriptor, 4_096)
            .unwrap_or_else(|error| panic!("{error}"));
        sink.install_chunk(&job, &chunk.descriptor, &records)
            .await
            .unwrap_or_else(|error| panic!("{error}"));
        job.chunks
            .get_mut(&chunk.descriptor.ordinal)
            .unwrap_or_else(|| panic!("missing progress"))
            .state = ChunkState::Installed;
    }
    sink.verify_staging(&job, &built.manifest)
        .await
        .unwrap_or_else(|error| panic!("{error}"));
    job.state = BootstrapState::ReadyToActivate;
    let evidence = ActivationEvidence {
        manifest_root: built.manifest.root_digest,
        installed_chunks: built
            .manifest
            .chunks
            .iter()
            .map(|chunk| chunk.chunk_id)
            .collect(),
        current_scope_version: boundary.scope_version,
        current_scope_generation: boundary.scope_generation,
        current_authority_epoch: boundary.authority_epoch,
        authorization_current: true,
        staging_verified: true,
        pending_intent_digest: pending.digest,
        lease: SnapshotLease {
            snapshot_id: built.manifest.snapshot_id,
            boundary_sequence: boundary.sequence,
            retained_from: Sequence(0),
            expires_at_unix_ms: 20_000,
        },
    };
    let activation = verify_activation(&job, &built.manifest, &evidence, 10_000)
        .unwrap_or_else(|error| panic!("{error}"));

    sink.fail_once(BootstrapFailpoint::BeforeActivationCommit);
    assert!(sink.activate(activation).await.is_err());
    assert_eq!(
        sink.active_generation(),
        ReplicaGeneration::new(1).unwrap_or_else(|error| panic!("{error}"))
    );
    assert_eq!(sink.cursor(), Sequence(0));

    sink.fail_once(BootstrapFailpoint::AfterActivationCommitBeforeResponse);
    assert!(sink.activate(activation).await.is_err());
    let replay = sink
        .activate(activation)
        .await
        .unwrap_or_else(|error| panic!("{error}"));
    assert_eq!(replay.active_generation, job.staging_generation);
    assert_eq!(replay.cursor_sequence, boundary.sequence);
    assert_eq!(replay.preserved_pending_operations, 1);
}
