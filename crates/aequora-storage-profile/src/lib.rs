//! Policy presets and low-storage admission decisions; policy never changes sync semantics.
use aequora_storage_core::{StorageAdmission, StorageClass, StoragePressure};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum ProfileKind {
    MobileMinimal,
    MobileStandard,
    DesktopConservative,
    DesktopStandard,
    DesktopLarge,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum MaintenancePolicy {
    MobileOpportunistic,
    DesktopIdle,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct StorageProfile {
    pub kind: ProfileKind,
    pub max_blob_cache_bytes: u64,
    pub max_optional_scope_bytes: u64,
    pub min_free_bytes: u64,
    pub min_free_percent: u8,
    pub maintenance_policy: MaintenancePolicy,
}

impl StorageProfile {
    pub const MOBILE_MINIMAL: Self = Self {
        kind: ProfileKind::MobileMinimal,
        max_blob_cache_bytes: 128 << 20,
        max_optional_scope_bytes: 256 << 20,
        min_free_bytes: 256 << 20,
        min_free_percent: 10,
        maintenance_policy: MaintenancePolicy::MobileOpportunistic,
    };
    pub const MOBILE_STANDARD: Self = Self {
        kind: ProfileKind::MobileStandard,
        max_blob_cache_bytes: 512 << 20,
        max_optional_scope_bytes: 1 << 30,
        min_free_bytes: 256 << 20,
        min_free_percent: 10,
        maintenance_policy: MaintenancePolicy::MobileOpportunistic,
    };
    pub const DESKTOP_STANDARD: Self = Self {
        kind: ProfileKind::DesktopStandard,
        max_blob_cache_bytes: 4 << 30,
        max_optional_scope_bytes: 20 << 30,
        min_free_bytes: 1 << 30,
        min_free_percent: 5,
        maintenance_policy: MaintenancePolicy::DesktopIdle,
    };

    #[must_use]
    pub const fn admit(
        self,
        free_bytes: u64,
        capacity_bytes: u64,
        required_bytes: u64,
        temporary_bytes: u64,
    ) -> StorageAdmission {
        let reserve_percent = capacity_bytes.saturating_mul(self.min_free_percent as u64) / 100;
        let reserve = if self.min_free_bytes > reserve_percent {
            self.min_free_bytes
        } else {
            reserve_percent
        };
        let needed = required_bytes
            .saturating_add(temporary_bytes)
            .saturating_add(reserve);
        if free_bytes >= needed {
            StorageAdmission::Allowed
        } else if free_bytes >= required_bytes.saturating_add(reserve) {
            StorageAdmission::AllowedReduced
        } else if free_bytes >= reserve {
            StorageAdmission::Deferred
        } else {
            StorageAdmission::InsufficientSpace
        }
    }
}

#[must_use]
pub const fn pressure_for(
    free_bytes: u64,
    reserve_bytes: u64,
    durable_write_possible: bool,
) -> StoragePressure {
    if !durable_write_possible {
        StoragePressure::ReadMostly
    } else if free_bytes < reserve_bytes / 2 {
        StoragePressure::Critical
    } else if free_bytes < reserve_bytes {
        StoragePressure::Pressure
    } else {
        StoragePressure::Healthy
    }
}

#[must_use]
pub const fn may_evict(class: StorageClass, pinned: bool, has_pending_dependency: bool) -> bool {
    !pinned && !has_pending_dependency && class.automatically_evictable()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn low_disk_never_admits_false_success() {
        assert_eq!(
            StorageProfile::MOBILE_STANDARD.admit(10, 1_000, 20, 20),
            StorageAdmission::InsufficientSpace
        );
    }
    #[test]
    fn pending_intent_is_not_evictable() {
        assert!(!may_evict(StorageClass::CriticalIntent, false, false));
    }
}
