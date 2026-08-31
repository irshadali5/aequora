//! Certification claims are bound to an adapter, target platform, and complete test matrix.
use aequora_storage_core::{LocalStorageCapabilities, Platform};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use thiserror::Error;

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
pub enum StorageCertificationTest {
    AtomicLocalIntent,
    CursorAtomicity,
    CrashReopen,
    UniqueOperationId,
    Migration,
    Durability,
    CorruptionHandling,
    ProcessKill,
    LowDisk,
    CachePurge,
    BackupRestore,
    KeyUnavailable,
    SmallMemory,
    MultiProcess,
    StaleWriterFencing,
    CloneRestore,
    LargeDataset,
    AgentOwnership,
    SleepResume,
}

pub const SHARED_TESTS: [StorageCertificationTest; 7] = [
    StorageCertificationTest::AtomicLocalIntent,
    StorageCertificationTest::CursorAtomicity,
    StorageCertificationTest::CrashReopen,
    StorageCertificationTest::UniqueOperationId,
    StorageCertificationTest::Migration,
    StorageCertificationTest::Durability,
    StorageCertificationTest::CorruptionHandling,
];
pub const MOBILE_TESTS: [StorageCertificationTest; 6] = [
    StorageCertificationTest::ProcessKill,
    StorageCertificationTest::LowDisk,
    StorageCertificationTest::CachePurge,
    StorageCertificationTest::BackupRestore,
    StorageCertificationTest::KeyUnavailable,
    StorageCertificationTest::SmallMemory,
];
pub const DESKTOP_TESTS: [StorageCertificationTest; 6] = [
    StorageCertificationTest::MultiProcess,
    StorageCertificationTest::StaleWriterFencing,
    StorageCertificationTest::CloneRestore,
    StorageCertificationTest::LargeDataset,
    StorageCertificationTest::AgentOwnership,
    StorageCertificationTest::SleepResume,
];

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum CertificationProfile {
    MobileLocalStoreFull,
    DesktopLocalStoreFull,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum Observation {
    Passed,
    Failed,
    Unsupported,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct CertificationRecord {
    pub adapter: String,
    pub adapter_version: String,
    pub platform: Platform,
    pub target: String,
    pub profile: CertificationProfile,
    pub capabilities: LocalStorageCapabilities,
    pub observations: BTreeMap<StorageCertificationTest, Observation>,
}

#[derive(Clone, Debug, Eq, Error, PartialEq)]
pub enum CertificationError {
    #[error("adapter lacks mandatory local storage capabilities")]
    MissingCapability,
    #[error("profile does not match target platform")]
    PlatformProfileMismatch,
    #[error("required platform test did not pass: {0:?}")]
    RequiredTestFailed(StorageCertificationTest),
}

impl CertificationRecord {
    /// Validates a full target-specific storage certification claim.
    ///
    /// # Errors
    /// Returns a typed failure for missing capabilities, mismatched profiles, or incomplete tests.
    pub fn validate(&self) -> Result<(), CertificationError> {
        if !self
            .capabilities
            .contains(LocalStorageCapabilities::REQUIRED)
        {
            return Err(CertificationError::MissingCapability);
        }
        let platform_tests = match (self.profile, self.platform) {
            (CertificationProfile::MobileLocalStoreFull, Platform::Android | Platform::Ios) => {
                &MOBILE_TESTS
            }
            (
                CertificationProfile::DesktopLocalStoreFull,
                Platform::Linux | Platform::Windows | Platform::MacOs,
            ) => &DESKTOP_TESTS,
            _ => return Err(CertificationError::PlatformProfileMismatch),
        };
        for test in SHARED_TESTS.iter().chain(platform_tests) {
            if self.observations.get(test) != Some(&Observation::Passed) {
                return Err(CertificationError::RequiredTestFailed(*test));
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn abstract_only_claim_fails() {
        let r = CertificationRecord {
            adapter: "db".into(),
            adapter_version: "1".into(),
            platform: Platform::Android,
            target: "aarch64-linux-android".into(),
            profile: CertificationProfile::MobileLocalStoreFull,
            capabilities: LocalStorageCapabilities::REQUIRED,
            observations: BTreeMap::new(),
        };
        assert!(matches!(
            r.validate(),
            Err(CertificationError::RequiredTestFailed(_))
        ));
    }
}
