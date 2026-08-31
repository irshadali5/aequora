//! Android-specific normalization and packaging policy.
//!
//! JNI, `Context`, `WorkManager`, `ConnectivityManager`, and Keystore objects remain in the Android
//! host. This crate validates their inputs and translates them to platform-neutral Rust values.

use aequora_mobile_runtime::{
    AppLifecycle, BatteryClass, MobileSyncBudget, NetworkAvailability, NetworkConstraint,
    NetworkContext, NetworkCost, PowerContext, RoamingState, StoragePressure, ThermalState,
};
use serde::{Deserialize, Serialize};
use std::path::{Component, Path};
use thiserror::Error;

pub const REQUIRED_ABIS: [&str; 2] = ["arm64-v8a", "x86_64"];
pub const REQUIRED_PERMISSIONS: [&str; 2] = [
    "android.permission.INTERNET",
    "android.permission.ACCESS_NETWORK_STATE",
];

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum AndroidLifecycle {
    Resumed,
    Started,
    Stopped,
    Destroyed,
}

impl From<AndroidLifecycle> for AppLifecycle {
    fn from(value: AndroidLifecycle) -> Self {
        match value {
            AndroidLifecycle::Resumed => Self::Foreground,
            AndroidLifecycle::Started | AndroidLifecycle::Stopped => Self::Background,
            AndroidLifecycle::Destroyed => Self::Terminating,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub enum AndroidConnectivity {
    #[default]
    Unavailable,
    Validated,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub enum AndroidNetworkCost {
    #[default]
    Unmetered,
    Metered,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub enum AndroidCongestion {
    #[default]
    NotCongested,
    Congested,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub enum AndroidTransport {
    #[default]
    Direct,
    Vpn,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct AndroidNetworkState {
    pub connectivity: AndroidConnectivity,
    pub cost: AndroidNetworkCost,
    pub congestion: AndroidCongestion,
    pub roaming: RoamingState,
    pub transport: AndroidTransport,
}

impl From<AndroidNetworkState> for NetworkContext {
    fn from(value: AndroidNetworkState) -> Self {
        Self {
            availability: match value.connectivity {
                AndroidConnectivity::Unavailable => NetworkAvailability::Unavailable,
                AndroidConnectivity::Validated => NetworkAvailability::Available,
            },
            cost: if value.cost == AndroidNetworkCost::Metered
                || value.roaming == RoamingState::Roaming
            {
                NetworkCost::Expensive
            } else {
                NetworkCost::Unmetered
            },
            constraint: match value.congestion {
                AndroidCongestion::NotCongested => NetworkConstraint::Unconstrained,
                AndroidCongestion::Congested => NetworkConstraint::Constrained,
            },
            roaming: value.roaming,
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct AndroidPowerState {
    pub charging: bool,
    pub low_power_mode: bool,
    pub battery_percent: u8,
    pub thermal_status: u8,
}

impl From<AndroidPowerState> for PowerContext {
    fn from(value: AndroidPowerState) -> Self {
        let battery = match value.battery_percent.min(100) {
            0..=5 => BatteryClass::Critical,
            6..=20 => BatteryClass::Low,
            21..=79 => BatteryClass::Normal,
            _ => BatteryClass::High,
        };
        let thermal = match value.thermal_status {
            0..=1 => ThermalState::Nominal,
            2 => ThermalState::Fair,
            3..=4 => ThermalState::Serious,
            _ => ThermalState::Critical,
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
pub struct WorkManagerGrant {
    pub timeout_ms: u64,
    pub upload_bytes: u64,
    pub download_bytes: u64,
    pub operations: usize,
}

impl WorkManagerGrant {
    /// Converts an OS execution opportunity into a bounded Rust budget.
    ///
    /// # Errors
    ///
    /// Rejects invalid or excessive grants.
    pub fn budget(self) -> Result<MobileSyncBudget, AndroidAdapterError> {
        let budget = MobileSyncBudget {
            max_duration: std::time::Duration::from_millis(self.timeout_ms),
            max_upload_bytes: self.upload_bytes,
            max_download_bytes: self.download_bytes,
            max_operations: self.operations,
        };
        budget
            .validate()
            .map_err(|_| AndroidAdapterError::InvalidGrant)?;
        Ok(budget.constrained_by(MobileSyncBudget::BACKGROUND))
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct AndroidStorePolicy {
    pub private_data_directory: String,
    pub storage: StoragePressure,
    pub exclude_database_from_auto_backup: bool,
    pub hardware_backed_keys_required: bool,
}

impl AndroidStorePolicy {
    /// Validates private relative placement and anti-cloning policy.
    ///
    /// # Errors
    ///
    /// Rejects public/absolute/traversing paths and backup-enabled device state.
    pub fn validate(&self) -> Result<(), AndroidAdapterError> {
        let path = Path::new(&self.private_data_directory);
        if self.private_data_directory.is_empty()
            || path.is_absolute()
            || path.components().any(|component| {
                matches!(
                    component,
                    Component::ParentDir | Component::RootDir | Component::Prefix(_)
                )
            })
            || !self.exclude_database_from_auto_backup
        {
            return Err(AndroidAdapterError::UnsafeStorePolicy);
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Error, Eq, PartialEq)]
pub enum AndroidAdapterError {
    #[error("Android background execution grant is invalid")]
    InvalidGrant,
    #[error("Android local-store or backup policy is unsafe")]
    UnsafeStorePolicy,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn metered_or_roaming_android_network_is_expensive() {
        let context = NetworkContext::from(AndroidNetworkState {
            connectivity: AndroidConnectivity::Validated,
            cost: AndroidNetworkCost::Unmetered,
            congestion: AndroidCongestion::NotCongested,
            roaming: RoamingState::Roaming,
            transport: AndroidTransport::Vpn,
        });
        assert!(context.available() && context.expensive() && context.roaming());
    }

    #[test]
    fn work_manager_cannot_expand_background_budget() {
        let budget = WorkManagerGrant {
            timeout_ms: 60_000,
            upload_bytes: 10_000_000,
            download_bytes: 10_000_000,
            operations: 1_000,
        }
        .budget();
        assert_eq!(budget, Ok(MobileSyncBudget::BACKGROUND));
    }

    #[test]
    fn backup_cloning_policy_fails_closed() {
        let policy = AndroidStorePolicy {
            private_data_directory: "databases/aequora".to_owned(),
            storage: StoragePressure::Healthy,
            exclude_database_from_auto_backup: false,
            hardware_backed_keys_required: true,
        };
        assert_eq!(
            policy.validate(),
            Err(AndroidAdapterError::UnsafeStorePolicy)
        );
    }
}
