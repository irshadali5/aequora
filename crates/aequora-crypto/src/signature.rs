use crate::{
    SigningError, VerificationError,
    key::{CryptoTimestamp, KeyPurpose, KeyStatus, PublicKeyBytes, SignatureBytes, SigningKeyId},
    policy::SignatureAlgorithm,
};
use async_trait::async_trait;
use ed25519_dalek::{Signer as _, Verifier as _};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    fmt,
    sync::{Arc, RwLock},
};
use zeroize::Zeroize;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct SignatureEnvelope {
    pub algorithm: SignatureAlgorithm,
    pub key_id: SigningKeyId,
    pub purpose: KeyPurpose,
    pub signed_at: CryptoTimestamp,
    pub signature: SignatureBytes,
}

/// Non-serializable Ed25519 secret material. Debug output is always redacted.
pub struct SigningSecret([u8; 32]);

impl SigningSecret {
    /// Generates an Ed25519 secret using operating-system cryptographic randomness.
    ///
    /// # Errors
    ///
    /// Returns [`SigningError`] if the random source is unavailable.
    pub fn generate() -> Result<Self, SigningError> {
        let mut bytes = [0_u8; 32];
        getrandom::fill(&mut bytes).map_err(|_| SigningError::RandomnessUnavailable)?;
        Ok(Self(bytes))
    }

    #[must_use]
    pub const fn from_bytes(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    #[must_use]
    pub fn public_key(&self) -> PublicKeyBytes {
        PublicKeyBytes(
            ed25519_dalek::SigningKey::from_bytes(&self.0)
                .verifying_key()
                .to_bytes(),
        )
    }

    fn sign(&self, digest: &[u8; 32]) -> SignatureBytes {
        SignatureBytes(
            ed25519_dalek::SigningKey::from_bytes(&self.0)
                .sign(digest)
                .to_bytes()
                .to_vec(),
        )
    }
}

impl Drop for SigningSecret {
    fn drop(&mut self) {
        self.0.zeroize();
    }
}

impl fmt::Debug for SigningSecret {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("SigningSecret([REDACTED])")
    }
}

#[async_trait]
pub trait KeyProvider: Send + Sync {
    async fn sign(
        &self,
        purpose: KeyPurpose,
        key_id: SigningKeyId,
        digest: [u8; 32],
    ) -> Result<SignatureBytes, SigningError>;

    async fn public_key(&self, key_id: SigningKeyId) -> Result<PublicKeyBytes, SigningError>;
}

struct InMemorySigningKey {
    purpose: KeyPurpose,
    status: KeyStatus,
    secret: SigningSecret,
}

/// Process-local provider intended for tests and development, not production custody.
#[derive(Clone, Default)]
pub struct InMemorySigningKeyProvider {
    keys: Arc<RwLock<HashMap<SigningKeyId, InMemorySigningKey>>>,
}

impl InMemorySigningKeyProvider {
    pub fn insert(
        &self,
        key_id: SigningKeyId,
        purpose: KeyPurpose,
        status: KeyStatus,
        secret: SigningSecret,
    ) {
        if let Ok(mut keys) = self.keys.write() {
            keys.insert(
                key_id,
                InMemorySigningKey {
                    purpose,
                    status,
                    secret,
                },
            );
        }
    }

    /// Applies an allowed lifecycle transition to a test key.
    ///
    /// # Errors
    ///
    /// Returns [`SigningError`] when the key is unknown or the transition is invalid.
    pub fn set_status(&self, key_id: SigningKeyId, next: KeyStatus) -> Result<(), SigningError> {
        let mut keys = self.keys.write().map_err(|_| SigningError::KeyUnknown)?;
        let key = keys.get_mut(&key_id).ok_or(SigningError::KeyUnknown)?;
        key.status = key
            .status
            .transition(next)
            .map_err(|_| SigningError::KeyNotActive)?;
        Ok(())
    }
}

#[async_trait]
impl KeyProvider for InMemorySigningKeyProvider {
    async fn sign(
        &self,
        purpose: KeyPurpose,
        key_id: SigningKeyId,
        digest: [u8; 32],
    ) -> Result<SignatureBytes, SigningError> {
        let keys = self.keys.read().map_err(|_| SigningError::KeyUnknown)?;
        let key = keys.get(&key_id).ok_or(SigningError::KeyUnknown)?;
        if key.purpose != purpose {
            return Err(SigningError::PurposeMismatch);
        }
        if !key.status.permits_signing() {
            return Err(SigningError::KeyNotActive);
        }
        Ok(key.secret.sign(&digest))
    }

    async fn public_key(&self, key_id: SigningKeyId) -> Result<PublicKeyBytes, SigningError> {
        let keys = self.keys.read().map_err(|_| SigningError::KeyUnknown)?;
        keys.get(&key_id)
            .map(|key| key.secret.public_key())
            .ok_or(SigningError::KeyUnknown)
    }
}

/// Signs a domain-separated digest with purpose and time metadata.
///
/// # Errors
///
/// Returns [`SigningError`] when the provider rejects lookup, purpose, status, or signing.
pub async fn sign_digest<P: KeyProvider + ?Sized>(
    provider: &P,
    purpose: KeyPurpose,
    key_id: SigningKeyId,
    signed_at: CryptoTimestamp,
    digest: [u8; 32],
) -> Result<SignatureEnvelope, SigningError> {
    Ok(SignatureEnvelope {
        algorithm: SignatureAlgorithm::Ed25519V1,
        key_id,
        purpose,
        signed_at,
        signature: provider.sign(purpose, key_id, digest).await?,
    })
}

/// Verifies an Ed25519 signature over an already domain-separated digest.
///
/// # Errors
///
/// Returns [`VerificationError`] for malformed material or signature mismatch.
pub fn verify_digest(
    public_key: PublicKeyBytes,
    digest: [u8; 32],
    envelope: &SignatureEnvelope,
) -> Result<(), VerificationError> {
    if envelope.algorithm != SignatureAlgorithm::Ed25519V1
        || envelope.signature.0.len() != SignatureBytes::ED25519_LENGTH
    {
        return Err(VerificationError::SignatureInvalid);
    }
    let key = ed25519_dalek::VerifyingKey::from_bytes(&public_key.0)
        .map_err(|_| VerificationError::SignatureInvalid)?;
    let signature = ed25519_dalek::Signature::try_from(envelope.signature.0.as_slice())
        .map_err(|_| VerificationError::SignatureInvalid)?;
    key.verify(&digest, &signature)
        .map_err(|_| VerificationError::SignatureInvalid)
}
