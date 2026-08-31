//! Platform-neutral local-storage classification, layout, durability, and recovery contracts.

use serde::{Deserialize, Serialize};
use thiserror::Error;

pub const STORAGE_INVARIANT_IDS: [&str; 10] = [
    "AEQ-INV-STORAGE001",
    "AEQ-INV-STORAGE002",
    "AEQ-INV-STORAGE003",
    "AEQ-INV-STORAGE004",
    "AEQ-INV-STORAGE005",
    "AEQ-INV-STORAGE006",
    "AEQ-INV-STORAGE007",
    "AEQ-INV-STORAGE008",
    "AEQ-INV-STORAGE009",
    "AEQ-INV-STORAGE010",
];

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
pub enum StorageClass {
    CriticalIntent,
    ReplicatedState,
    DerivedState,
    EphemeralCache,
    SecretMaterial,
}

impl StorageClass {
    #[must_use]
    pub const fn automatically_evictable(self) -> bool {
        matches!(self, Self::DerivedState | Self::EphemeralCache)
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum DurabilityClass {
    Critical,
    Standard,
    Reconstructable,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum StoragePressure {
    Healthy,
    Pressure,
    Critical,
    ReadMostly,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum StorageAdmission {
    Allowed,
    AllowedReduced,
    Deferred,
    InsufficientSpace,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum Platform {
    Android,
    Ios,
    Linux,
    Windows,
    MacOs,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum PathClass {
    DurableData,
    DurableBlobs,
    Cache,
    Diagnostics,
    SecureStore,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct PlatformPathPolicy {
    pub platform: Platform,
    pub database: PathClass,
    pub durable_blobs: PathClass,
    pub temporary_files: PathClass,
    pub secrets: PathClass,
    pub live_store_may_roam: bool,
    pub broad_filesystem_permission_required: bool,
}

impl PlatformPathPolicy {
    #[must_use]
    pub const fn private_default(platform: Platform) -> Self {
        Self {
            platform,
            database: PathClass::DurableData,
            durable_blobs: PathClass::DurableBlobs,
            temporary_files: PathClass::Cache,
            secrets: PathClass::SecureStore,
            live_store_may_roam: false,
            broad_filesystem_permission_required: false,
        }
    }

    /// Validates that durable state and secrets cannot be placed in evictable or roaming storage.
    ///
    /// # Errors
    /// Returns an error when the layout weakens the Part 33 storage boundary.
    pub const fn validate(self) -> Result<(), LocalStorageError> {
        if !matches!(self.database, PathClass::DurableData)
            || !matches!(self.secrets, PathClass::SecureStore)
            || self.live_store_may_roam
        {
            return Err(LocalStorageError::UnsafeFilesystem);
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
pub struct LocalStoreFormatVersion(pub u32);

impl LocalStoreFormatVersion {
    /// Rejects an older binary opening a newer store.
    ///
    /// # Errors
    /// Returns [`LocalStorageError::UnsupportedStoreVersion`] when unsupported.
    pub const fn require_supported_by(self, maximum: Self) -> Result<(), LocalStorageError> {
        if self.0 > maximum.0 {
            Err(LocalStorageError::UnsupportedStoreVersion)
        } else {
            Ok(())
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct DeviceBinding {
    pub store_id: String,
    pub device_id: String,
    pub binding_generation: u64,
    pub secure_key_fingerprint: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BindingDisposition {
    Current,
    RebindRequired,
}

impl DeviceBinding {
    #[must_use]
    pub fn classify(&self, secure_fingerprint: Option<&str>) -> BindingDisposition {
        if secure_fingerprint == Some(self.secure_key_fingerprint.as_str()) {
            BindingDisposition::Current
        } else {
            BindingDisposition::RebindRequired
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct LocalStorageCapabilities(u16);

impl LocalStorageCapabilities {
    pub const ATOMIC_MULTI_RECORD: Self = Self(1 << 0);
    pub const DURABLE_COMMIT: Self = Self(1 << 1);
    pub const UNIQUE_OPERATION_ID: Self = Self(1 << 2);
    pub const ORDERED_LOCAL_SEQUENCE: Self = Self(1 << 3);
    pub const ATOMIC_CURSOR_UPDATE: Self = Self(1 << 4);
    pub const SCHEMA_MIGRATION: Self = Self(1 << 5);
    pub const CRASH_RECOVERY: Self = Self(1 << 6);
    pub const BOUNDED_INDEXED_LOOKUP: Self = Self(1 << 7);
    pub const REQUIRED: Self = Self(0xff);
    #[must_use]
    pub const fn union(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }
    #[must_use]
    pub const fn contains(self, other: Self) -> bool {
        self.0 & other.0 == other.0
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum PublicationState {
    Staging,
    Verified,
    Published,
}

impl PublicationState {
    /// Advances a snapshot/blob publication state without exposing unverified data.
    ///
    /// # Errors
    /// Returns a transaction error for an invalid transition.
    pub const fn transition(self, next: Self) -> Result<Self, LocalStorageError> {
        match (self, next) {
            (Self::Staging, Self::Verified) | (Self::Verified, Self::Published) => Ok(next),
            _ => Err(LocalStorageError::TransactionFailed),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
pub enum LocalStorageError {
    #[error("local storage is full")]
    DiskFull,
    #[error("local storage is read-only")]
    ReadOnlyFilesystem,
    #[error("local storage is corrupt")]
    Corruption,
    #[error("storage key is unavailable")]
    KeyUnavailable,
    #[error("migration is required")]
    MigrationRequired,
    #[error("store format is newer than this binary supports")]
    UnsupportedStoreVersion,
    #[error("secure device binding does not match the store")]
    BindingMismatch,
    #[error("local transaction failed")]
    TransactionFailed,
    #[error("unsafe filesystem placement")]
    UnsafeFilesystem,
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn critical_intent_is_never_evictable() {
        assert!(!StorageClass::CriticalIntent.automatically_evictable());
    }
    #[test]
    fn restored_binding_requires_matching_secure_key() {
        let binding = DeviceBinding {
            store_id: "s".into(),
            device_id: "d".into(),
            binding_generation: 1,
            secure_key_fingerprint: "key".into(),
        };
        assert_eq!(
            binding.classify(Some("other")),
            BindingDisposition::RebindRequired
        );
    }
    #[test]
    fn publication_requires_verification() {
        assert!(
            PublicationState::Staging
                .transition(PublicationState::Published)
                .is_err()
        );
    }
}
