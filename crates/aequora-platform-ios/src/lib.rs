//! iOS-specific normalization and packaging policy.
//!
//! Swift/Objective-C owns `BGTaskScheduler`, `NWPathMonitor`, Keychain, file-protection, and share
//! sheet calls. This crate keeps those framework types outside synchronization semantics.

use aequora_mobile_runtime::{
    AppLifecycle, BatteryClass, MobileSyncBudget, NetworkAvailability, NetworkConstraint,
    NetworkContext, NetworkCost, PowerContext, RoamingState, StoragePressure, ThermalState,
};
use serde::{Deserialize, Serialize};
use std::path::{Component, Path};
use thiserror::Error;

pub const REQUIRED_TARGETS: [&str; 2] = ["aarch64-apple-ios", "aarch64-apple-ios-sim"];

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum IosLifecycle {
    Active,
    Inactive,
    Background,
    Suspended,
    Terminating,
}

impl From<IosLifecycle> for AppLifecycle {
    fn from(value: IosLifecycle) -> Self {
        match value {
            IosLifecycle::Active => Self::Foreground,
            IosLifecycle::Inactive | IosLifecycle::Background => Self::Background,
            IosLifecycle::Suspended => Self::Suspended,
            IosLifecycle::Terminating => Self::Terminating,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub enum IosPathStatus {
    #[default]
    Unsatisfied,
    Satisfied,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub enum IosNetworkCost {
    #[default]
    Unmetered,
    Metered,
    Expensive,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub enum IosPathConstraint {
    #[default]
    Unconstrained,
    Constrained,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub enum IosTransport {
    #[default]
    Wifi,
    Cellular,
    Other,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct IosNetworkPath {
    pub status: IosPathStatus,
    pub cost: IosNetworkCost,
    pub constraint: IosPathConstraint,
    pub transport: IosTransport,
    pub roaming: RoamingState,
}

impl From<IosNetworkPath> for NetworkContext {
    fn from(value: IosNetworkPath) -> Self {
        Self {
            availability: match value.status {
                IosPathStatus::Unsatisfied => NetworkAvailability::Unavailable,
                IosPathStatus::Satisfied => NetworkAvailability::Available,
            },
            cost: match value.cost {
                IosNetworkCost::Unmetered => NetworkCost::Unmetered,
                IosNetworkCost::Metered => NetworkCost::Metered,
                IosNetworkCost::Expensive => NetworkCost::Expensive,
            },
            constraint: match value.constraint {
                IosPathConstraint::Unconstrained => NetworkConstraint::Unconstrained,
                IosPathConstraint::Constrained => NetworkConstraint::Constrained,
            },
            roaming: value.roaming,
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum IosThermalState {
    Nominal,
    Fair,
    Serious,
    Critical,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct IosPowerState {
    pub charging: bool,
    pub low_power_mode: bool,
    pub battery_percent: u8,
    pub thermal: IosThermalState,
}

impl From<IosPowerState> for PowerContext {
    fn from(value: IosPowerState) -> Self {
        let battery = match value.battery_percent.min(100) {
            0..=5 => BatteryClass::Critical,
            6..=20 => BatteryClass::Low,
            21..=79 => BatteryClass::Normal,
            _ => BatteryClass::High,
        };
        let thermal = match value.thermal {
            IosThermalState::Nominal => ThermalState::Nominal,
            IosThermalState::Fair => ThermalState::Fair,
            IosThermalState::Serious => ThermalState::Serious,
            IosThermalState::Critical => ThermalState::Critical,
        };
        Self {
            charging: value.charging,
            low_power_mode: value.low_power_mode,
            battery,
            thermal,
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct BgTaskGrant {
    pub remaining_ms: u64,
    pub upload_bytes: u64,
    pub download_bytes: u64,
    pub operations: usize,
}

impl BgTaskGrant {
    /// Converts a `BGTask` execution window into a bounded Rust budget.
    ///
    /// # Errors
    ///
    /// Rejects invalid or excessive grants.
    pub fn budget(self) -> Result<MobileSyncBudget, IosAdapterError> {
        let budget = MobileSyncBudget {
            max_duration: std::time::Duration::from_millis(self.remaining_ms),
            max_upload_bytes: self.upload_bytes,
            max_download_bytes: self.download_bytes,
            max_operations: self.operations,
        };
        budget
            .validate()
            .map_err(|_| IosAdapterError::InvalidGrant)?;
        Ok(budget.constrained_by(MobileSyncBudget::BACKGROUND))
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum FileProtection {
    Complete,
    CompleteUnlessOpen,
    CompleteUntilFirstAuthentication,
    None,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct IosStorePolicy {
    pub application_support_path: String,
    pub storage: StoragePressure,
    pub file_protection: FileProtection,
    pub excluded_from_icloud_backup: bool,
    pub keychain_access_group: Option<String>,
}

impl IosStorePolicy {
    /// Validates protected private placement and anti-cloning behavior.
    ///
    /// # Errors
    ///
    /// Rejects traversal, unprotected files, and backup-enabled device state.
    pub fn validate(&self) -> Result<(), IosAdapterError> {
        let path = Path::new(&self.application_support_path);
        if self.application_support_path.is_empty()
            || path.is_absolute()
            || path.components().any(|component| {
                matches!(
                    component,
                    Component::ParentDir | Component::RootDir | Component::Prefix(_)
                )
            })
            || self.file_protection == FileProtection::None
            || !self.excluded_from_icloud_backup
            || self.keychain_access_group.as_deref().is_some_and(|group| {
                group.is_empty() || group.len() > 256 || group.chars().any(char::is_control)
            })
        {
            return Err(IosAdapterError::UnsafeStorePolicy);
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Error, Eq, PartialEq)]
pub enum IosAdapterError {
    #[error("iOS background execution grant is invalid")]
    InvalidGrant,
    #[error("iOS local-store, protection, or backup policy is unsafe")]
    UnsafeStorePolicy,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn constrained_ios_path_is_preserved() {
        let context = NetworkContext::from(IosNetworkPath {
            status: IosPathStatus::Satisfied,
            cost: IosNetworkCost::Expensive,
            constraint: IosPathConstraint::Constrained,
            transport: IosTransport::Cellular,
            roaming: RoamingState::Home,
        });
        assert!(context.available() && context.metered() && context.constrained());
    }

    #[test]
    fn bgtask_cannot_expand_background_budget() {
        let budget = BgTaskGrant {
            remaining_ms: 60_000,
            upload_bytes: 10_000_000,
            download_bytes: 10_000_000,
            operations: 1_000,
        }
        .budget();
        assert_eq!(budget, Ok(MobileSyncBudget::BACKGROUND));
    }

    #[test]
    fn unprotected_store_fails_closed() {
        let policy = IosStorePolicy {
            application_support_path: "Application Support/aequora".to_owned(),
            storage: StoragePressure::Healthy,
            file_protection: FileProtection::None,
            excluded_from_icloud_backup: true,
            keychain_access_group: None,
        };
        assert_eq!(policy.validate(), Err(IosAdapterError::UnsafeStorePolicy));
    }
}
