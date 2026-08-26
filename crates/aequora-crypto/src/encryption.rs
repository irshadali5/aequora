use crate::{
    CiphertextDigest, CryptoPolicy, EncryptionAlgorithm, EncryptionError, KeyPurpose, KeyStatus,
    PlaintextDigest, TenantKeyId, canonical_bytes,
};
use aequora_types::TenantId;
use argon2::Argon2;
use async_trait::async_trait;
use chacha20poly1305::{
    KeyInit, XChaCha20Poly1305, XNonce,
    aead::{Aead, Payload},
};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    fmt,
    sync::{Arc, RwLock},
};
use uuid::Uuid;
use zeroize::Zeroize;

pub const AAD_VERSION_V1: u16 = 1;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct AssociatedData {
    pub tenant_id: TenantId,
    pub purpose: KeyPurpose,
    pub resource_kind: u16,
    pub resource_id: Uuid,
    pub field_id: Option<u16>,
    pub schema_version: u16,
}

/// Non-serializable 256-bit content/envelope key with redacted debug and drop zeroization.
pub struct DataEncryptionKey([u8; 32]);

impl DataEncryptionKey {
    /// Generates a 256-bit key from operating-system cryptographic randomness.
    ///
    /// # Errors
    ///
    /// Returns [`EncryptionError`] if the random source is unavailable.
    pub fn generate() -> Result<Self, EncryptionError> {
        let mut bytes = [0_u8; 32];
        getrandom::fill(&mut bytes).map_err(|_| EncryptionError::RandomnessUnavailable)?;
        Ok(Self(bytes))
    }

    #[must_use]
    pub const fn from_bytes(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }
}

impl Drop for DataEncryptionKey {
    fn drop(&mut self) {
        self.0.zeroize();
    }
}

impl fmt::Debug for DataEncryptionKey {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("DataEncryptionKey([REDACTED])")
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct EncryptedPayload {
    pub algorithm: EncryptionAlgorithm,
    pub key_id: TenantKeyId,
    pub nonce: Vec<u8>,
    pub aad_version: u16,
    pub ciphertext: Vec<u8>,
    pub ciphertext_digest: CiphertextDigest,
    pub plaintext_digest: Option<PlaintextDigest>,
}

#[async_trait]
pub trait EncryptionKeyProvider: Send + Sync {
    async fn encrypt(
        &self,
        purpose: KeyPurpose,
        key_id: TenantKeyId,
        nonce: [u8; 24],
        aad: &[u8],
        plaintext: &[u8],
    ) -> Result<Vec<u8>, EncryptionError>;

    async fn decrypt(
        &self,
        purpose: KeyPurpose,
        key_id: TenantKeyId,
        nonce: [u8; 24],
        aad: &[u8],
        ciphertext: &[u8],
    ) -> Result<Vec<u8>, EncryptionError>;
}

struct InMemoryEncryptionKey {
    purpose: KeyPurpose,
    status: KeyStatus,
    key: DataEncryptionKey,
}

/// Process-local encryption provider for tests and development only.
#[derive(Clone, Default)]
pub struct InMemoryEncryptionKeyProvider {
    keys: Arc<RwLock<HashMap<TenantKeyId, InMemoryEncryptionKey>>>,
}

impl InMemoryEncryptionKeyProvider {
    pub fn insert(
        &self,
        key_id: TenantKeyId,
        purpose: KeyPurpose,
        status: KeyStatus,
        key: DataEncryptionKey,
    ) {
        if let Ok(mut keys) = self.keys.write() {
            keys.insert(
                key_id,
                InMemoryEncryptionKey {
                    purpose,
                    status,
                    key,
                },
            );
        }
    }

    /// Applies an allowed lifecycle transition to a test key.
    ///
    /// # Errors
    ///
    /// Returns [`EncryptionError`] when the key is unknown or transition is invalid.
    pub fn set_status(&self, key_id: TenantKeyId, next: KeyStatus) -> Result<(), EncryptionError> {
        let mut keys = self.keys.write().map_err(|_| EncryptionError::KeyUnknown)?;
        let key = keys.get_mut(&key_id).ok_or(EncryptionError::KeyUnknown)?;
        key.status = key
            .status
            .transition(next)
            .map_err(|_| EncryptionError::KeyNotActive)?;
        Ok(())
    }
}

fn cipher(key: &DataEncryptionKey) -> XChaCha20Poly1305 {
    XChaCha20Poly1305::new((&key.0).into())
}

#[async_trait]
impl EncryptionKeyProvider for InMemoryEncryptionKeyProvider {
    async fn encrypt(
        &self,
        purpose: KeyPurpose,
        key_id: TenantKeyId,
        nonce: [u8; 24],
        aad: &[u8],
        plaintext: &[u8],
    ) -> Result<Vec<u8>, EncryptionError> {
        let keys = self.keys.read().map_err(|_| EncryptionError::KeyUnknown)?;
        let key = keys.get(&key_id).ok_or(EncryptionError::KeyUnknown)?;
        if key.purpose != purpose || !purpose.is_encryption() {
            return Err(EncryptionError::PurposeMismatch);
        }
        if key.status != KeyStatus::Active {
            return Err(EncryptionError::KeyNotActive);
        }
        cipher(&key.key)
            .encrypt(
                XNonce::from_slice(&nonce),
                Payload {
                    msg: plaintext,
                    aad,
                },
            )
            .map_err(|_| EncryptionError::DecryptFailed)
    }

    async fn decrypt(
        &self,
        purpose: KeyPurpose,
        key_id: TenantKeyId,
        nonce: [u8; 24],
        aad: &[u8],
        ciphertext: &[u8],
    ) -> Result<Vec<u8>, EncryptionError> {
        let keys = self.keys.read().map_err(|_| EncryptionError::KeyUnknown)?;
        let key = keys.get(&key_id).ok_or(EncryptionError::KeyUnknown)?;
        if key.purpose != purpose || !purpose.is_encryption() {
            return Err(EncryptionError::PurposeMismatch);
        }
        if !matches!(
            key.status,
            KeyStatus::Active | KeyStatus::Retiring | KeyStatus::VerificationOnly
        ) {
            return Err(EncryptionError::KeyNotActive);
        }
        cipher(&key.key)
            .decrypt(
                XNonce::from_slice(&nonce),
                Payload {
                    msg: ciphertext,
                    aad,
                },
            )
            .map_err(|_| EncryptionError::DecryptFailed)
    }
}

/// Encrypts plaintext with random-nonce XChaCha20-Poly1305 and canonical tenant-bound AAD.
///
/// # Errors
///
/// Returns [`EncryptionError`] for policy, randomness, key-provider, or encoding failures.
pub async fn encrypt_payload<P: EncryptionKeyProvider + ?Sized>(
    provider: &P,
    policy: &CryptoPolicy,
    key_id: TenantKeyId,
    aad: &AssociatedData,
    plaintext: &[u8],
    retain_plaintext_digest: bool,
) -> Result<EncryptedPayload, EncryptionError> {
    let algorithm = EncryptionAlgorithm::XChaCha20Poly1305V1;
    policy
        .require_encryption(algorithm)
        .map_err(|_| EncryptionError::AlgorithmDisallowed)?;
    if !aad.purpose.is_encryption() {
        return Err(EncryptionError::PurposeMismatch);
    }
    let aad_bytes =
        canonical_bytes(&(AAD_VERSION_V1, aad)).map_err(|_| EncryptionError::CanonicalEncoding)?;
    let mut nonce = [0_u8; 24];
    getrandom::fill(&mut nonce).map_err(|_| EncryptionError::RandomnessUnavailable)?;
    let ciphertext = provider
        .encrypt(aad.purpose, key_id, nonce, &aad_bytes, plaintext)
        .await?;
    Ok(EncryptedPayload {
        algorithm,
        key_id,
        nonce: nonce.to_vec(),
        aad_version: AAD_VERSION_V1,
        ciphertext_digest: CiphertextDigest::of(&ciphertext),
        plaintext_digest: retain_plaintext_digest.then(|| PlaintextDigest::of(plaintext)),
        ciphertext,
    })
}

/// Authenticates and decrypts a payload under caller-supplied expected context.
///
/// # Errors
///
/// Returns [`EncryptionError`] for policy, digest, AAD, provider, or authentication failure.
pub async fn decrypt_payload<P: EncryptionKeyProvider + ?Sized>(
    provider: &P,
    policy: &CryptoPolicy,
    aad: &AssociatedData,
    encrypted: &EncryptedPayload,
) -> Result<Vec<u8>, EncryptionError> {
    policy
        .require_encryption(encrypted.algorithm)
        .map_err(|_| EncryptionError::AlgorithmDisallowed)?;
    if encrypted.aad_version != AAD_VERSION_V1 || encrypted.nonce.len() != 24 {
        return Err(EncryptionError::AssociatedDataVersion);
    }
    if CiphertextDigest::of(&encrypted.ciphertext) != encrypted.ciphertext_digest {
        return Err(EncryptionError::DecryptFailed);
    }
    let nonce: [u8; 24] = encrypted
        .nonce
        .as_slice()
        .try_into()
        .map_err(|_| EncryptionError::AssociatedDataVersion)?;
    let aad_bytes = canonical_bytes(&(encrypted.aad_version, aad))
        .map_err(|_| EncryptionError::CanonicalEncoding)?;
    let plaintext = provider
        .decrypt(
            aad.purpose,
            encrypted.key_id,
            nonce,
            &aad_bytes,
            &encrypted.ciphertext,
        )
        .await?;
    if encrypted
        .plaintext_digest
        .is_some_and(|digest| digest != PlaintextDigest::of(&plaintext))
    {
        return Err(EncryptionError::DecryptFailed);
    }
    Ok(plaintext)
}

/// Derives a one-time export key with Argon2id. Callers persist salt and parameters, not password.
///
/// # Errors
///
/// Returns [`EncryptionError`] for an empty passphrase, short salt, or KDF failure.
pub fn derive_export_key(
    passphrase: &[u8],
    salt: &[u8],
) -> Result<DataEncryptionKey, EncryptionError> {
    if passphrase.is_empty() || salt.len() < 16 {
        return Err(EncryptionError::KeyDerivationFailed);
    }
    let mut output = [0_u8; 32];
    Argon2::default()
        .hash_password_into(passphrase, salt, &mut output)
        .map_err(|_| EncryptionError::KeyDerivationFailed)?;
    Ok(DataEncryptionKey::from_bytes(output))
}
