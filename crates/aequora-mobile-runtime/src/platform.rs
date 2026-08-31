use crate::{AppLifecycle, MobileSyncBudget, NetworkContext, PowerContext};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::fmt;
use thiserror::Error;
use zeroize::Zeroize;

const MAX_HINT_BYTES: usize = 512;

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct SecureKeyHandle(String);

impl SecureKeyHandle {
    /// Creates an opaque, non-secret platform key identifier.
    ///
    /// # Errors
    ///
    /// Rejects empty, oversized, or control-character identifiers.
    pub fn new(value: impl Into<String>) -> Result<Self, PlatformError> {
        let value = value.into();
        if value.is_empty() || value.len() > 256 || value.chars().any(char::is_control) {
            return Err(PlatformError::InvalidInput);
        }
        Ok(Self(value))
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Secret bytes are zeroized on drop and intentionally cannot be serialized or debug-printed.
#[derive(Clone, Eq, PartialEq, Zeroize)]
#[zeroize(drop)]
pub struct SecretBytes(Vec<u8>);

impl SecretBytes {
    /// Takes ownership of secret bytes.
    ///
    /// # Errors
    ///
    /// Rejects empty or unreasonably large secret material.
    pub fn new(value: Vec<u8>) -> Result<Self, PlatformError> {
        if value.is_empty() || value.len() > 64 * 1_024 {
            return Err(PlatformError::InvalidInput);
        }
        Ok(Self(value))
    }

    #[must_use]
    pub fn expose(&self) -> &[u8] {
        &self.0
    }
}

impl fmt::Debug for SecretBytes {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("SecretBytes([REDACTED])")
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum PushHintReason {
    AuthoritativeChange,
    ScopeChanged,
    Revocation,
    UpgradeNotice,
    SyncRequired,
}

/// Untrusted payload-free scheduling input. It cannot represent domain state or authorization.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct PushHint {
    pub store_hint: String,
    pub scope_hint: Option<String>,
    pub reason: PushHintReason,
}

impl PushHint {
    /// Validates privacy-safe bounded routing hints.
    ///
    /// # Errors
    ///
    /// Rejects empty, oversized, or control-character input.
    pub fn validate(&self) -> Result<(), PlatformError> {
        let valid = |value: &str| {
            !value.is_empty()
                && value.len() <= MAX_HINT_BYTES
                && !value.chars().any(char::is_control)
        };
        if !valid(&self.store_hint) || self.scope_hint.as_deref().is_some_and(|v| !valid(v)) {
            return Err(PlatformError::InvalidInput);
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum BackgroundRequest {
    Sync,
    Maintenance,
    BlobTransfer,
}

#[derive(Clone, Debug, Error, Eq, PartialEq)]
pub enum PlatformError {
    #[error("platform input is invalid")]
    InvalidInput,
    #[error("secure material is locked")]
    KeyLocked,
    #[error("platform service is unavailable")]
    Unavailable,
    #[error("platform operation was denied")]
    Denied,
    #[error("platform operation failed")]
    Failed,
}

#[async_trait]
pub trait SecureStore: Send + Sync {
    async fn create_signing_key(&self, alias: &str) -> Result<SecureKeyHandle, PlatformError>;
    async fn store_secret(
        &self,
        key: &SecureKeyHandle,
        value: SecretBytes,
    ) -> Result<(), PlatformError>;
    async fn load_secret(&self, key: &SecureKeyHandle) -> Result<SecretBytes, PlatformError>;
    async fn delete(&self, key: &SecureKeyHandle) -> Result<(), PlatformError>;
}

#[async_trait]
pub trait CredentialProvider: Send + Sync {
    async fn access_token(&self) -> Result<SecretBytes, PlatformError>;
}

pub trait NetworkMonitor: Send + Sync {
    fn current_network(&self) -> NetworkContext;
}

pub trait PowerMonitor: Send + Sync {
    fn current_power(&self) -> PowerContext;
}

pub trait AppLifecycleSource: Send + Sync {
    fn current_lifecycle(&self) -> AppLifecycle;
}

#[async_trait]
pub trait BackgroundExecutionHost: Send + Sync {
    async fn schedule(&self, request: BackgroundRequest) -> Result<(), PlatformError>;
    async fn complete(&self, retry: bool) -> Result<(), PlatformError>;
    fn granted_budget(&self) -> Option<MobileSyncBudget>;
}

pub trait PushHintSource: Send + Sync {
    fn take_hint(&self) -> Option<PushHint>;
}
