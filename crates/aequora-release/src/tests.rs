use super::*;

fn compatibility() -> CompatibilityMatrix {
    CompatibilityMatrix {
        protocol_min: 1,
        protocol_max: 2,
        operation_schema: 1,
        config_schema: 1,
        snapshot_schema: 1,
        registry_generation: 7,
        registry_digest: [7; 32],
        store_formats: BTreeMap::from([("sqlite".into(), 2)]),
        minimum_client: Version::new(0, 1, 0),
        minimum_server: Version::new(0, 1, 0),
        migration_set_digest: [9; 32],
    }
}

#[test]
fn immutable_version_cannot_be_repointed() {
    let mut index = ImmutableReleaseIndex::default();
    let version = Version::new(1, 2, 3);
    assert_eq!(index.record(version.clone(), b"manifest-a"), Ok(()));
    assert_eq!(index.record(version.clone(), b"manifest-a"), Ok(()));
    assert_eq!(
        index.record(version.clone(), b"manifest-b"),
        Err(ReleaseError::ImmutableVersionConflict(version))
    );
}

#[test]
fn update_decision_checks_all_dimensions_and_rollout() {
    let installation = InstallationState {
        version: Version::new(1, 0, 0),
        target: "x86_64-unknown-linux-gnu".into(),
        last_update_generation: 0,
        protocol_version: 1,
        config_schema: 1,
        registry_generation: 6,
        store_formats: BTreeMap::from([("sqlite".into(), 1)]),
        pending_operations: 4,
    };
    let metadata = UpdateMetadata {
        metadata_generation: 1,
        release_id: "release-2".into(),
        version: Version::new(1, 1, 0),
        channel: ReleaseChannel::Stable,
        target: "x86_64-unknown-linux-gnu".into(),
        artifact_name: "cli.tar.zst".into(),
        artifact_size: 3,
        artifact_hashes: ArtifactHashes::digest(b"new"),
        minimum_compatible_version: Version::new(1, 0, 0),
        criticality: UpdateCriticality::Routine,
        status: UpdateStatus::Available,
        rollout_basis_points: 10_000,
        issued_at: CryptoTimestamp(10),
        expires_at: CryptoTimestamp(100),
    };
    assert_eq!(
        decide_update(
            &installation,
            &metadata,
            &compatibility(),
            ReleaseChannel::Stable,
            AutomaticUpdatePolicy::InstallOnRestart,
            b"installation-a",
        ),
        Ok(UpdateDecision::InstallOnRestart)
    );
    let mut replayed = metadata.clone();
    replayed.metadata_generation = 1;
    let mut accepted = installation.clone();
    accepted.last_update_generation = 2;
    assert_eq!(
        decide_update(
            &accepted,
            &replayed,
            &compatibility(),
            ReleaseChannel::Stable,
            AutomaticUpdatePolicy::NotifyOnly,
            b"installation-a",
        ),
        Err(ReleaseError::StaleUpdateMetadata)
    );
    let mut halted = metadata;
    halted.status = UpdateStatus::Halted;
    assert_eq!(
        decide_update(
            &installation,
            &halted,
            &compatibility(),
            ReleaseChannel::Stable,
            AutomaticUpdatePolicy::NotifyOnly,
            b"installation-a",
        ),
        Err(ReleaseError::ReleaseUnavailable(UpdateStatus::Halted))
    );
}

#[test]
fn stable_promotion_requires_same_bytes_all_gates_and_isolated_credentials() {
    let evidence = PromotionEvidence {
        release_id: "release-3".into(),
        candidate_manifest_digest: [3; 32],
        promoted_manifest_digest: [3; 32],
        passed_gates: ReleaseGate::ALL.into_iter().collect(),
        source_commit: "0123456789abcdef0123456789abcdef01234567".into(),
        approvers: BTreeSet::from(["release-approver-a".into(), "release-approver-b".into()]),
        production_credentials_present_in_build_job: false,
    };
    assert_eq!(evidence.authorize_stable(2), Ok(()));
    assert_eq!(
        PromotionEvidence {
            promoted_manifest_digest: [4; 32],
            ..evidence
        }
        .authorize_stable(2),
        Err(ReleaseError::PromotionRejected)
    );
}

#[test]
fn checked_in_cross_platform_support_matrix_is_valid() {
    let matrix = SupportMatrix::from_ron(include_str!("../../../release/support-matrix.ron"))
        .unwrap_or_else(|error| panic!("support matrix failed: {error}"));
    assert!(
        matrix
            .targets
            .iter()
            .any(|target| target.platform == DeliveryPlatform::Oci)
    );
    assert!(matrix.targets.iter().any(|target| {
        target.platform == DeliveryPlatform::Windows && target.native_signing_required
    }));
}
