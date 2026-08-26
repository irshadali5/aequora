use super::*;
use aequora_protocol::{OperationEnvelope, OperationKind, OperationMetadata};
use aequora_types::{
    ActorId, DeviceId, EntityId, EntityRef, EntityType, HybridTimestamp, LineageContext, NodeId,
    OperationId, ProtocolVersion, SchemaVersion, TenantId,
};
use proptest::prelude::*;
use std::{
    future::Future,
    sync::Arc,
    task::{Context, Poll, Wake, Waker},
    thread,
};
use uuid::Uuid;

struct TestWake(thread::Thread);

impl Wake for TestWake {
    fn wake(self: Arc<Self>) {
        self.0.unpark();
    }
}

fn block_on<F: Future>(future: F) -> F::Output {
    let waker = Waker::from(Arc::new(TestWake(thread::current())));
    let mut context = Context::from_waker(&waker);
    let mut future = Box::pin(future);
    loop {
        match future.as_mut().poll(&mut context) {
            Poll::Ready(output) => return output,
            Poll::Pending => thread::park(),
        }
    }
}

fn tenant(value: u128) -> TenantId {
    TenantId::from_uuid(Uuid::from_u128(value))
}

fn signing_provider(
    key_id: SigningKeyId,
    purpose: KeyPurpose,
    byte: u8,
) -> (InMemorySigningKeyProvider, PublicKeyBytes) {
    let secret = SigningSecret::from_bytes([byte; 32]);
    let public = secret.public_key();
    let provider = InMemorySigningKeyProvider::default();
    provider.insert(key_id, purpose, KeyStatus::Active, secret);
    (provider, public)
}

async fn trusted_artifact_registry(
    artifact_tenant: TenantId,
) -> (InMemorySigningKeyProvider, SigningKeyId, TrustedKeyRegistry) {
    let root_id = SigningKeyId::from_uuid(Uuid::from_u128(10));
    let artifact_id = SigningKeyId::from_uuid(Uuid::from_u128(11));
    let (root_provider, root_public) = signing_provider(root_id, KeyPurpose::RegistrySigning, 7);
    let (artifact_provider, artifact_public) =
        signing_provider(artifact_id, KeyPurpose::ServerArtifactSigning, 9);
    let record = KeyRecord {
        key_id: artifact_id,
        tenant_id: Some(artifact_tenant),
        purpose: KeyPurpose::ServerArtifactSigning,
        public_key: artifact_public,
        status: KeyStatus::Active,
        not_before: CryptoTimestamp(100),
        not_after: Some(CryptoTimestamp(1_000)),
        created_at: CryptoTimestamp(90),
        revoked_at: None,
        compromise_time: None,
        revocation_reason: None,
    };
    let manifest = sign_key_registry(
        &root_provider,
        KeyRegistryGeneration(1),
        CryptoTimestamp(100),
        vec![record],
        root_id,
    )
    .await
    .unwrap_or_else(|error| panic!("{error}"));
    let mut registry = TrustedKeyRegistry::new(root_id, root_public);
    registry
        .accept(&manifest)
        .unwrap_or_else(|error| panic!("{error}"));
    (artifact_provider, artifact_id, registry)
}

#[test]
fn digest_domains_are_not_interchangeable() {
    let bytes = b"same canonical bytes";
    assert_ne!(SnapshotDigest::of(bytes).0, OperationDigest::of(bytes).0);
    assert_ne!(ArtifactDigest::of(bytes).0, BlobDigest::of(bytes).0);
    assert_eq!(SnapshotDigest::of(bytes), SnapshotDigest::of(bytes));
}

#[test]
fn signed_artifact_binds_content_tenant_and_trusted_registry() {
    block_on(async {
        let tenant_a = tenant(1);
        let tenant_b = tenant(2);
        let (provider, key_id, registry) = trusted_artifact_registry(tenant_a).await;
        let artifact = sign_artifact(
            &provider,
            ArtifactSigningContext {
                artifact_type: ArtifactType::Snapshot,
                format_version: 1,
                policy_version: CryptoPolicyVersion(1),
                tenant_id: tenant_a,
            },
            vec![1_u8, 2, 3],
            key_id,
            CryptoTimestamp(200),
        )
        .await
        .unwrap_or_else(|error| panic!("{error}"));
        let policy = CryptoPolicy::standard();
        let trust = TrustContext {
            tenant_id: tenant_a,
            artifact_type: ArtifactType::Snapshot,
            expected_purpose: KeyPurpose::ServerArtifactSigning,
            registry: &registry,
            policy: &policy,
        };
        verify_artifact(&artifact, &trust).unwrap_or_else(|error| panic!("{error}"));

        let wrong_tenant = TrustContext {
            tenant_id: tenant_b,
            ..trust
        };
        assert_eq!(
            verify_artifact(&artifact, &wrong_tenant),
            Err(VerificationError::TenantMismatch)
        );
        let mut tampered = artifact;
        tampered.manifest.content.push(4);
        assert_eq!(
            verify_artifact(&tampered, &trust),
            Err(VerificationError::DigestMismatch)
        );
    });
}

#[test]
fn registry_generation_cannot_roll_back() {
    block_on(async {
        let root_id = SigningKeyId::from_uuid(Uuid::from_u128(20));
        let (provider, public) = signing_provider(root_id, KeyPurpose::RegistrySigning, 3);
        let manifest = sign_key_registry(
            &provider,
            KeyRegistryGeneration(4),
            CryptoTimestamp(10),
            Vec::new(),
            root_id,
        )
        .await
        .unwrap_or_else(|error| panic!("{error}"));
        let mut registry = TrustedKeyRegistry::new(root_id, public);
        registry
            .accept(&manifest)
            .unwrap_or_else(|error| panic!("{error}"));
        assert_eq!(registry.accept(&manifest), Err(RegistryError::Rollback));
    });
}

#[test]
fn rotation_retains_history_while_revocation_blocks_new_signatures() {
    block_on(async {
        let artifact_tenant = tenant(1);
        let root_id = SigningKeyId::from_uuid(Uuid::from_u128(21));
        let artifact_id = SigningKeyId::from_uuid(Uuid::from_u128(22));
        let (root_provider, root_public) =
            signing_provider(root_id, KeyPurpose::RegistrySigning, 4);
        let (artifact_provider, artifact_public) =
            signing_provider(artifact_id, KeyPurpose::ServerArtifactSigning, 6);
        let artifact = sign_artifact(
            &artifact_provider,
            ArtifactSigningContext {
                artifact_type: ArtifactType::Export,
                format_version: 1,
                policy_version: CryptoPolicyVersion(1),
                tenant_id: artifact_tenant,
            },
            vec![1_u8],
            artifact_id,
            CryptoTimestamp(200),
        )
        .await
        .unwrap_or_else(|error| panic!("{error}"));
        artifact_provider
            .set_status(artifact_id, KeyStatus::Revoked)
            .unwrap_or_else(|error| panic!("{error}"));
        assert_eq!(
            sign_artifact(
                &artifact_provider,
                ArtifactSigningContext {
                    artifact_type: ArtifactType::Export,
                    format_version: 1,
                    policy_version: CryptoPolicyVersion(1),
                    tenant_id: artifact_tenant,
                },
                vec![2_u8],
                artifact_id,
                CryptoTimestamp(400),
            )
            .await,
            Err(SigningError::KeyNotActive)
        );

        let revoked_record = KeyRecord {
            key_id: artifact_id,
            tenant_id: Some(artifact_tenant),
            purpose: KeyPurpose::ServerArtifactSigning,
            public_key: artifact_public,
            status: KeyStatus::Revoked,
            not_before: CryptoTimestamp(100),
            not_after: Some(CryptoTimestamp(1_000)),
            created_at: CryptoTimestamp(90),
            revoked_at: Some(CryptoTimestamp(300)),
            compromise_time: None,
            revocation_reason: Some("operator-revocation".to_owned()),
        };
        let manifest = sign_key_registry(
            &root_provider,
            KeyRegistryGeneration(2),
            CryptoTimestamp(400),
            vec![revoked_record],
            root_id,
        )
        .await
        .unwrap_or_else(|error| panic!("{error}"));
        let mut registry = TrustedKeyRegistry::new(root_id, root_public);
        registry
            .accept(&manifest)
            .unwrap_or_else(|error| panic!("{error}"));
        let policy = CryptoPolicy::standard();
        let trust = TrustContext {
            tenant_id: artifact_tenant,
            artifact_type: ArtifactType::Export,
            expected_purpose: KeyPurpose::ServerArtifactSigning,
            registry: &registry,
            policy: &policy,
        };
        verify_artifact(&artifact, &trust).unwrap_or_else(|error| panic!("{error}"));
    });
}

#[test]
fn tenant_aad_and_ciphertext_tampering_fail_closed() {
    block_on(async {
        let key_id = TenantKeyId::from_uuid(Uuid::from_u128(30));
        let provider = InMemoryEncryptionKeyProvider::default();
        provider.insert(
            key_id,
            KeyPurpose::SnapshotEncryption,
            KeyStatus::Active,
            DataEncryptionKey::from_bytes([5; 32]),
        );
        let aad = AssociatedData {
            tenant_id: tenant(1),
            purpose: KeyPurpose::SnapshotEncryption,
            resource_kind: 1,
            resource_id: Uuid::from_u128(31),
            field_id: None,
            schema_version: 1,
        };
        let policy = CryptoPolicy::standard();
        let encrypted = encrypt_payload(&provider, &policy, key_id, &aad, b"snapshot", true)
            .await
            .unwrap_or_else(|error| panic!("{error}"));
        assert_eq!(
            decrypt_payload(&provider, &policy, &aad, &encrypted)
                .await
                .unwrap_or_else(|error| panic!("{error}")),
            b"snapshot"
        );

        let mut swapped = aad.clone();
        swapped.tenant_id = tenant(2);
        assert_eq!(
            decrypt_payload(&provider, &policy, &swapped, &encrypted).await,
            Err(EncryptionError::DecryptFailed)
        );
        let mut tampered = encrypted;
        tampered.ciphertext[0] ^= 1;
        assert_eq!(
            decrypt_payload(&provider, &policy, &aad, &tampered).await,
            Err(EncryptionError::DecryptFailed)
        );
    });
}

fn operation(device_id: DeviceId) -> OperationEnvelope {
    OperationEnvelope {
        protocol_version: ProtocolVersion::V1,
        operation_id: OperationId::from_uuid(Uuid::from_u128(40)),
        tenant_id: tenant(1),
        actor_id: ActorId::from_uuid(Uuid::from_u128(41)),
        device_id,
        entity: EntityRef {
            entity_type: EntityType::new(1).unwrap_or_else(|error| panic!("{error}")),
            entity_id: EntityId::from_uuid(Uuid::from_u128(42)),
        },
        base_version: None,
        created_at: HybridTimestamp {
            physical_ms: 500,
            logical: 0,
            node: NodeId::from_uuid(Uuid::from_u128(43)),
        },
        schema_version: SchemaVersion(1),
        operation_kind: OperationKind(7),
        payload: vec![8, 9],
        metadata: OperationMetadata {
            trace_id: Some("volatile-a".to_owned()),
            dependencies: smallvec::SmallVec::default(),
            lineage: LineageContext::legacy_missing(),
        },
    }
}

#[test]
fn device_signature_covers_semantics_but_not_volatile_trace() {
    block_on(async {
        let device_id = DeviceId::from_uuid(Uuid::from_u128(44));
        let key_id = SigningKeyId::from_uuid(Uuid::from_u128(45));
        let (provider, public) = signing_provider(key_id, KeyPurpose::DeviceOperationSigning, 11);
        let mut registry = DeviceKeyRegistry::default();
        registry
            .register(DeviceKeyRecord {
                device_id,
                key_id,
                algorithm: SignatureAlgorithm::Ed25519V1,
                public_key: public,
                status: KeyStatus::Active,
                not_before: CryptoTimestamp(1),
                not_after: None,
                revoked_at: None,
            })
            .unwrap_or_else(|error| panic!("{error}"));
        let mut signed =
            sign_operation(&provider, operation(device_id), key_id, CryptoTimestamp(2))
                .await
                .unwrap_or_else(|error| panic!("{error}"));
        let policy = CryptoPolicy::standard();
        let original = verify_operation_signature(&signed, device_id, &registry, &policy)
            .unwrap_or_else(|error| panic!("{error}"));
        signed.operation.metadata.trace_id = Some("volatile-b".to_owned());
        assert_eq!(
            verify_operation_signature(&signed, device_id, &registry, &policy),
            Ok(original)
        );
        signed.operation.payload.push(10);
        assert_eq!(
            verify_operation_signature(&signed, device_id, &registry, &policy),
            Err(VerificationError::SignatureInvalid)
        );
    });
}

#[test]
fn e2e_domains_cannot_hide_server_decision_fields() {
    let unsafe_policy = ProtectedDomainPolicy {
        mode: PayloadProtectionMode::ClientManagedE2E,
        server_responsibility: ServerPlaintextResponsibility {
            uses: [ServerPlaintextUse::Validation].into_iter().collect(),
        },
        append_only_or_whole_value: true,
    };
    assert_eq!(
        unsafe_policy.validate(),
        Err(ProtectedPayloadError::ServerRequiresPlaintext)
    );
}

#[test]
fn erasure_requires_every_usable_key_copy_and_plaintext_cache_removed() {
    let blocked = KeyDestructionEvidence {
        required_ciphertext_references: 3,
        usable_backup_copies: 1,
        usable_recovery_wraps: 0,
        plaintext_caches: 0,
        intentional_erasure_authorized: true,
    };
    assert_eq!(
        blocked.evaluate(),
        Err(KeyLifecycleError::UsableKeyCopyRemains)
    );
    let allowed = KeyDestructionEvidence {
        usable_backup_copies: 0,
        ..blocked
    };
    assert_eq!(
        allowed.evaluate(),
        Ok(KeyDestructionDecision::SafeIntentionalErasure)
    );
}

#[test]
fn secret_debug_is_redacted_and_passphrase_kdf_rejects_weak_inputs() {
    let secret = SigningSecret::from_bytes([42; 32]);
    assert_eq!(format!("{secret:?}"), "SigningSecret([REDACTED])");
    assert!(derive_export_key(b"passphrase", &[1; 16]).is_ok());
    assert!(derive_export_key(b"", &[1; 16]).is_err());
    assert!(derive_export_key(b"passphrase", &[1; 8]).is_err());
}

#[test]
fn required_crypto_policy_rejects_missing_providers_and_capabilities() {
    let policy = CryptoPolicy::enterprise();
    assert!(matches!(
        AequoraCrypto::builder().policy(policy.clone()).build(),
        Err(CryptoBuildError::SigningProviderRequired)
    ));
    assert_eq!(
        policy.validate_client_capabilities(&[]),
        Err(PolicyError::RequiredCapabilityMissing)
    );
    assert!(
        policy
            .validate_client_capabilities(&[
                aequora_protocol::Capability::SignedSnapshotV1,
                aequora_protocol::Capability::EncryptedSnapshotV1,
            ])
            .is_ok()
    );
}

proptest! {
    #[test]
    fn canonical_digest_is_deterministic_and_domain_separated(bytes in prop::collection::vec(any::<u8>(), 0..4096)) {
        prop_assert_eq!(SnapshotDigest::of(&bytes), SnapshotDigest::of(&bytes));
        prop_assert_ne!(SnapshotDigest::of(&bytes).0, OperationDigest::of(&bytes).0);
    }
}
