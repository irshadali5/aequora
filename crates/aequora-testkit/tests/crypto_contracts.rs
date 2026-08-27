use aequora_crypto::{
    AssociatedData, CryptoPolicy, DataEncryptionKey, EncryptionError,
    InMemoryEncryptionKeyProvider, KeyPurpose, KeyStatus, TenantKeyId, decrypt_payload,
    encrypt_payload,
};
use aequora_types::TenantId;
use uuid::Uuid;

#[tokio::test]
async fn encrypted_artifact_provider_contract_rejects_cross_tenant_swap_and_key_loss() {
    let tenant_a = TenantId::from_uuid(Uuid::from_u128(1));
    let key_id = TenantKeyId::from_uuid(Uuid::from_u128(2));
    let provider = InMemoryEncryptionKeyProvider::default();
    provider.insert(
        key_id,
        KeyPurpose::ReplayBundleEncryption,
        KeyStatus::Active,
        DataEncryptionKey::from_bytes([23; 32]),
    );
    let aad = AssociatedData {
        tenant_id: tenant_a,
        purpose: KeyPurpose::ReplayBundleEncryption,
        resource_kind: 3,
        resource_id: Uuid::from_u128(3),
        field_id: None,
        schema_version: 1,
    };
    let policy = CryptoPolicy::enterprise();
    let encrypted = encrypt_payload(&provider, &policy, key_id, &aad, b"sensitive replay", false)
        .await
        .unwrap_or_else(|error| panic!("{error}"));

    let mut wrong_tenant = aad.clone();
    wrong_tenant.tenant_id = TenantId::from_uuid(Uuid::from_u128(4));
    assert_eq!(
        decrypt_payload(&provider, &policy, &wrong_tenant, &encrypted).await,
        Err(EncryptionError::DecryptFailed)
    );

    provider
        .set_status(key_id, KeyStatus::Revoked)
        .unwrap_or_else(|error| panic!("{error}"));
    assert_eq!(
        decrypt_payload(&provider, &policy, &aad, &encrypted).await,
        Err(EncryptionError::KeyNotActive)
    );
}
