//! Windows `LocalAppData`, Credential Manager/DPAPI, named-pipe, and startup policy contracts.

use aequora_desktop_runtime::{NetworkAvailability, NetworkContext, NetworkCost, NetworkPath};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use thiserror::Error;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct WindowsPaths {
    pub data: PathBuf,
    pub cache: PathBuf,
    pub config: PathBuf,
}

impl WindowsPaths {
    /// Derives current-user application paths.
    ///
    /// # Errors
    ///
    /// Rejects unsafe application identifiers or non-absolute roots.
    pub fn from_user_roots(
        app: &str,
        local_app_data: &Path,
        roaming_app_data: &Path,
    ) -> Result<Self, WindowsError> {
        if !valid_app(app) || !local_app_data.is_absolute() || !roaming_app_data.is_absolute() {
            return Err(WindowsError::UnsafePath);
        }
        Ok(Self {
            data: local_app_data.join(app).join("Data"),
            cache: local_app_data.join(app).join("Cache"),
            config: roaming_app_data.join(app),
        })
    }
}

/// Returns a per-user named pipe; its host must apply an owner-only ACL.
///
/// # Errors
///
/// Rejects unsafe user or application identifiers.
pub fn named_pipe(app: &str, user_sid: &str) -> Result<String, WindowsError> {
    if !valid_app(app)
        || user_sid.is_empty()
        || user_sid.len() > 256
        || !user_sid
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-')
    {
        return Err(WindowsError::UnsafeIdentity);
    }
    Ok(format!(r"\\.\pipe\{app}-{user_sid}-aequora"))
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum WindowsSecureStore {
    CredentialManager,
    DpapiCurrentUser,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum WindowsAutostart {
    Disabled,
    StartupTask,
    UserService,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum WindowsPackage {
    Msix,
    Msi,
    PortableZip,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct WindowsReleasePolicy {
    pub package: WindowsPackage,
    pub code_signed: bool,
}

impl WindowsReleasePolicy {
    /// Requires signed Windows executables and installers.
    ///
    /// # Errors
    ///
    /// Rejects unsigned release packages.
    pub const fn validate(self) -> Result<(), WindowsError> {
        if self.code_signed {
            Ok(())
        } else {
            Err(WindowsError::UnsignedPackage)
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum WindowsConnectivity {
    Disconnected,
    Connected,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum WindowsCost {
    Unmetered,
    Metered,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum WindowsRoute {
    Direct,
    Vpn,
    CaptivePortal,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct WindowsNetwork {
    pub connectivity: WindowsConnectivity,
    pub cost: WindowsCost,
    pub route: WindowsRoute,
    pub generation: u64,
}

impl From<WindowsNetwork> for NetworkContext {
    fn from(value: WindowsNetwork) -> Self {
        Self {
            availability: if value.connectivity == WindowsConnectivity::Connected {
                NetworkAvailability::Available
            } else {
                NetworkAvailability::Unavailable
            },
            cost: if value.cost == WindowsCost::Metered {
                NetworkCost::Metered
            } else {
                NetworkCost::Unmetered
            },
            path: match value.route {
                WindowsRoute::Direct => NetworkPath::Direct,
                WindowsRoute::Vpn => NetworkPath::Vpn,
                WindowsRoute::CaptivePortal => NetworkPath::CaptivePortal,
            },
            generation: value.generation,
        }
    }
}

#[derive(Clone, Copy, Debug, Error, Eq, PartialEq)]
pub enum WindowsError {
    #[error("Windows desktop path policy is unsafe")]
    UnsafePath,
    #[error("Windows desktop user identity is unsafe")]
    UnsafeIdentity,
    #[error("Windows executable or installer is not signed")]
    UnsignedPackage,
}

fn valid_app(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn pipe_is_scoped_to_user() {
        assert!(named_pipe("app", "S-1-5-21").is_ok());
        assert!(named_pipe("app", "bad/user").is_err());
    }
}
