use aequora_conformance::{CertificationTier, ConformanceProfile, definitions_for};
use aequora_crypto::{
    CryptoTimestamp, InMemorySigningKeyProvider, KeyPurpose, KeyStatus, SigningKeyId, SigningSecret,
};
use aequora_invariants::InvariantId;
use aequora_release::{
    ArtifactDescriptor, ArtifactHashes, ArtifactKind, BuildProvenance, CompatibilityMatrix,
    ImmutableReleaseIndex, MigrationBundleDescriptor, ReleaseChannel, ReleaseError,
    ReleaseManifest, ReleaseTrustStore, RollbackClass, TrustedReleaseKey, Version, sign_artifact,
    sign_release_manifest,
};
use aequora_update::{AtomicUpdateState, UpdateError, UpdateStage, UpgradeCheckpoint};
use std::collections::{BTreeMap, BTreeSet};
use uuid::Uuid;

fn checkpoint() -> UpgradeCheckpoint {
    UpgradeCheckpoint {
        pending_operations: 7,
        cursor: 42,
        conflict_count: 3,
        store_identity: [1; 16],
        durable_intent_digest: [2; 32],
        store_version: 4,
    }
}

#[test]
fn channel_promotion_cannot_repoint_a_semantic_version() {
    let mut index = ImmutableReleaseIndex::default();
    let version: Version = "1.4.0"
        .parse()
        .unwrap_or_else(|error| panic!("invalid fixture version: {error}"));
    assert_eq!(index.record(version.clone(), b"candidate"), Ok(()));
    assert_eq!(index.record(version.clone(), b"candidate"), Ok(()));
    assert_eq!(
        index.record(version.clone(), b"rebuilt-stable"),
        Err(ReleaseError::ImmutableVersionConflict(version))
    );
}

#[test]
fn desktop_upgrade_preserves_all_durable_intent_dimensions() {
    let state = AtomicUpdateState {
        stage: UpdateStage::Installed,
        checkpoint: checkpoint(),
    };
    let changed = UpgradeCheckpoint {
        durable_intent_digest: [9; 32],
        ..checkpoint()
    };
    assert_eq!(
        state.advance(UpdateStage::Migrated, changed),
        Err(UpdateError::DurableStateChanged)
    );
}

#[test]
fn rollback_class_is_enforced_before_old_binary_start() {
    let state = AtomicUpdateState {
        stage: UpdateStage::Resumed,
        checkpoint: checkpoint(),
    };
    assert_eq!(
        state.authorize_rollback(RollbackClass::ForwardOnly, 4, true),
        Err(UpdateError::RollbackRejected)
    );
    assert_eq!(
        state.authorize_rollback(RollbackClass::RollbackRequiresMigration, 4, false),
        Err(UpdateError::RollbackMigrationRequired)
    );
}

#[test]
fn release_invariants_and_conformance_profile_are_complete() {
    let invariants = InvariantId::ALL
        .iter()
        .map(|id| id.as_str())
        .filter(|id| id.starts_with("AEQ-INV-RELEASE"))
        .collect::<Vec<_>>();
    assert_eq!(invariants.len(), 10);
    let definitions = definitions_for(
        ConformanceProfile::ReleaseEngineeringFull,
        CertificationTier::FullSync,
    );
    assert_eq!(definitions.len(), 10);
    for invariant in invariants {
        assert!(
            definitions
                .iter()
                .any(|definition| definition.invariant_ids.contains(&invariant))
        );
    }
}

#[tokio::test]
async fn final_artifact_bytes_are_purpose_signed_and_tamper_evident() {
    let bytes = b"final packaged bytes";
    let key_id = SigningKeyId::from_uuid(Uuid::from_u128(44));
    let secret = SigningSecret::from_bytes([44; 32]);
    let public_key = secret.public_key();
    let provider = InMemorySigningKeyProvider::default();
    provider.insert(
        key_id,
        KeyPurpose::ReleaseArtifactSigning,
        KeyStatus::Active,
        secret,
    );
    let signature = sign_artifact(bytes, &provider, key_id, CryptoTimestamp(10))
        .await
        .unwrap_or_else(|error| panic!("artifact signing failed: {error}"));
    let descriptor = ArtifactDescriptor {
        name: "aequora-cli-1.4.0-x86_64-unknown-linux-gnu.tar.zst".into(),
        kind: ArtifactKind::Cli,
        target: "x86_64-unknown-linux-gnu".into(),
        size: u64::try_from(bytes.len()).unwrap_or(u64::MAX),
        hashes: ArtifactHashes::digest(bytes),
        signature,
        sbom: None,
        provenance: None,
    };
    let mut trust = ReleaseTrustStore::default();
    trust.insert(
        key_id,
        TrustedReleaseKey {
            public_key,
            purpose: KeyPurpose::ReleaseArtifactSigning,
            status: KeyStatus::Active,
            not_before: CryptoTimestamp(1),
            not_after: Some(CryptoTimestamp(100)),
        },
    );
    assert_eq!(
        descriptor.verify(bytes, &trust, CryptoTimestamp(20)),
        Ok(())
    );
    assert!(matches!(
        descriptor.verify(b"changed bytes", &trust, CryptoTimestamp(20)),
        Err(ReleaseError::ArtifactDigestMismatch(_))
    ));
}

#[tokio::test]
#[allow(clippy::too_many_lines)]
async fn complete_release_manifest_verifies_every_required_artifact() {
    let artifact_key_id = SigningKeyId::from_uuid(Uuid::from_u128(45));
    let manifest_key_id = SigningKeyId::from_uuid(Uuid::from_u128(46));
    let artifact_secret = SigningSecret::from_bytes([45; 32]);
    let manifest_secret = SigningSecret::from_bytes([46; 32]);
    let artifact_public = artifact_secret.public_key();
    let manifest_public = manifest_secret.public_key();
    let provider = InMemorySigningKeyProvider::default();
    provider.insert(
        artifact_key_id,
        KeyPurpose::ReleaseArtifactSigning,
        KeyStatus::Active,
        artifact_secret,
    );
    provider.insert(
        manifest_key_id,
        KeyPurpose::ReleaseManifestSigning,
        KeyStatus::Active,
        manifest_secret,
    );
    let specifications = [
        ("sbom.cdx.json", ArtifactKind::Sbom),
        ("provenance.json", ArtifactKind::Provenance),
        ("migrations.tar.zst", ArtifactKind::MigrationBundle),
        ("registry.ron", ArtifactKind::RegistrySnapshot),
        ("config-schema.ron", ArtifactKind::ConfigurationSchema),
        ("conformance.ron", ArtifactKind::ConformanceReport),
        ("release-notes.md", ArtifactKind::Documentation),
        (
            "aequora-cli-1.4.0-rc.1-x86_64-unknown-linux-gnu.tar.zst",
            ArtifactKind::Cli,
        ),
    ];
    let mut bytes_by_name = BTreeMap::new();
    let mut artifact_set = Vec::new();
    for (name, kind) in specifications {
        let bytes = format!("final:{name}").into_bytes();
        let signature = sign_artifact(&bytes, &provider, artifact_key_id, CryptoTimestamp(10))
            .await
            .unwrap_or_else(|error| panic!("artifact signing failed: {error}"));
        artifact_set.push(ArtifactDescriptor {
            name: name.into(),
            kind,
            target: "x86_64-unknown-linux-gnu".into(),
            size: u64::try_from(bytes.len()).unwrap_or(u64::MAX),
            hashes: ArtifactHashes::digest(&bytes),
            signature,
            sbom: matches!(kind, ArtifactKind::Cli | ArtifactKind::MigrationBundle)
                .then(|| "sbom.cdx.json".into()),
            provenance: matches!(kind, ArtifactKind::Cli | ArtifactKind::MigrationBundle)
                .then(|| "provenance.json".into()),
        });
        bytes_by_name.insert(name.to_owned(), bytes);
    }
    let compatibility = CompatibilityMatrix {
        protocol_min: 1,
        protocol_max: 2,
        operation_schema: 1,
        config_schema: 1,
        snapshot_schema: 1,
        registry_generation: 2,
        registry_digest: [7; 32],
        store_formats: BTreeMap::from([("sqlite".into(), 2)]),
        minimum_client: Version::new(1, 0, 0),
        minimum_server: Version::new(1, 0, 0),
        migration_set_digest: [9; 32],
    };
    let manifest = ReleaseManifest {
        format_version: 1,
        release_id: "release-1-4-0".into(),
        release_version: "1.4.0-rc.1"
            .parse()
            .unwrap_or_else(|error| panic!("invalid fixture version: {error}")),
        channel: ReleaseChannel::ReleaseCandidate,
        provenance: BuildProvenance {
            source_repository: "github.com/irshadali5/aequora".into(),
            git_commit: "0123456789abcdef0123456789abcdef01234567".into(),
            build_id: "build-linux-1".into(),
            rust_toolchain: "1.87.0".into(),
            target: "x86_64-unknown-linux-gnu".into(),
            cargo_features: BTreeSet::from(["default".into()]),
            source_digest: [6; 32],
            workflow_identity: "release-candidate".into(),
        },
        compatibility,
        rollback_class: RollbackClass::RollbackRequiresMigration,
        artifact_set,
        migrations: vec![MigrationBundleDescriptor {
            adapter: "sqlite".into(),
            artifact: "migrations.tar.zst".into(),
            from_format: 1,
            to_format: 2,
            migration_ids: vec!["migration-0001".into()],
            digest: [9; 32],
            reversible: true,
        }],
        release_notes: "release-notes.md".into(),
    };
    let signed = sign_release_manifest(manifest, &provider, manifest_key_id, CryptoTimestamp(11))
        .await
        .unwrap_or_else(|error| panic!("manifest signing failed: {error}"));
    let mut trust = ReleaseTrustStore::default();
    for (id, public_key, purpose) in [
        (
            artifact_key_id,
            artifact_public,
            KeyPurpose::ReleaseArtifactSigning,
        ),
        (
            manifest_key_id,
            manifest_public,
            KeyPurpose::ReleaseManifestSigning,
        ),
    ] {
        trust.insert(
            id,
            TrustedReleaseKey {
                public_key,
                purpose,
                status: KeyStatus::Active,
                not_before: CryptoTimestamp(1),
                not_after: Some(CryptoTimestamp(100)),
            },
        );
    }
    assert_eq!(
        signed.verify(&trust, CryptoTimestamp(20), |name| {
            bytes_by_name
                .get(name)
                .cloned()
                .ok_or_else(|| ReleaseError::MissingArtifactReference(name.into()))
        }),
        Ok(())
    );
}
