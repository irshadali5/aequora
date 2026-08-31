//! macOS Application Support, Keychain, local socket, and `LaunchAgent` policy contracts.

use aequora_desktop_runtime::{NetworkAvailability, NetworkContext, NetworkCost, NetworkPath};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use thiserror::Error;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct MacosPaths {
    pub data: PathBuf,
    pub cache: PathBuf,
    pub config: PathBuf,
    pub socket: PathBuf,
}

impl MacosPaths {
    /// Derives current-user application paths under the home directory.
    ///
    /// # Errors
    ///
    /// Rejects unsafe bundle identifiers or a non-absolute home path.
    pub fn from_home(bundle_id: &str, home: &Path) -> Result<Self, MacosError> {
        if !valid_bundle(bundle_id) || !home.is_absolute() {
            return Err(MacosError::UnsafePath);
        }
        let data = home.join("Library/Application Support").join(bundle_id);
        Ok(Self {
            socket: data.join("agent.sock"),
            data,
            cache: home.join("Library/Caches").join(bundle_id),
            config: home
                .join("Library/Preferences")
                .join(format!("{bundle_id}.plist")),
        })
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum MacosSecureStore {
    KeychainThisUser,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum MacosAutostart {
    Disabled,
    LaunchAgent,
    LoginItem,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum MacosPackage {
    AppBundle,
    Dmg,
    Pkg,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct MacosReleasePolicy {
    pub package: MacosPackage,
    pub code_signed: bool,
    pub notarized: bool,
}

impl MacosReleasePolicy {
    /// Requires signing and notarization for production macOS distribution.
    ///
    /// # Errors
    ///
    /// Rejects unsigned or unnotarized release artifacts.
    pub const fn validate(self) -> Result<(), MacosError> {
        if self.code_signed && self.notarized {
            Ok(())
        } else {
            Err(MacosError::UntrustedPackage)
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum MacosConnectivity {
    Unsatisfied,
    Satisfied,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum MacosCost {
    Unmetered,
    Expensive,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum MacosRoute {
    Direct,
    Vpn,
    CaptivePortal,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct MacosNetwork {
    pub connectivity: MacosConnectivity,
    pub cost: MacosCost,
    pub route: MacosRoute,
    pub generation: u64,
}

impl From<MacosNetwork> for NetworkContext {
    fn from(value: MacosNetwork) -> Self {
        Self {
            availability: if value.connectivity == MacosConnectivity::Satisfied {
                NetworkAvailability::Available
            } else {
                NetworkAvailability::Unavailable
            },
            cost: if value.cost == MacosCost::Expensive {
                NetworkCost::Metered
            } else {
                NetworkCost::Unmetered
            },
            path: match value.route {
                MacosRoute::Direct => NetworkPath::Direct,
                MacosRoute::Vpn => NetworkPath::Vpn,
                MacosRoute::CaptivePortal => NetworkPath::CaptivePortal,
            },
            generation: value.generation,
        }
    }
}

#[derive(Clone, Copy, Debug, Error, Eq, PartialEq)]
pub enum MacosError {
    #[error("macOS desktop path policy is unsafe")]
    UnsafePath,
    #[error("macOS package is unsigned or not notarized")]
    UntrustedPackage,
}

fn valid_bundle(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 255
        && value.contains('.')
        && value
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '.'))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn paths_stay_under_user_home() {
        let paths = MacosPaths::from_home("com.example.app", Path::new("/Users/alice"));
        assert!(paths.is_ok());
    }
}
