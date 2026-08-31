//! Linux XDG, Secret Service, local socket, and user-autostart policy contracts.

use aequora_desktop_runtime::{NetworkAvailability, NetworkContext, NetworkCost, NetworkPath};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use thiserror::Error;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct LinuxPaths {
    pub data: PathBuf,
    pub cache: PathBuf,
    pub config: PathBuf,
    pub runtime: PathBuf,
}

impl LinuxPaths {
    /// Derives private application paths from absolute XDG roots.
    ///
    /// # Errors
    ///
    /// Rejects unsafe application identifiers or non-absolute roots.
    pub fn from_xdg(
        app: &str,
        data: &Path,
        cache: &Path,
        config: &Path,
        runtime: &Path,
    ) -> Result<Self, LinuxError> {
        if !valid_app(app)
            || [data, cache, config, runtime]
                .iter()
                .any(|path| !path.is_absolute())
        {
            return Err(LinuxError::UnsafePath);
        }
        Ok(Self {
            data: data.join(app),
            cache: cache.join(app),
            config: config.join(app),
            runtime: runtime.join(app),
        })
    }
    #[must_use]
    pub fn socket_path(&self) -> PathBuf {
        self.runtime.join("agent.sock")
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum LinuxSecureStore {
    SecretService,
    PassphraseEncrypted,
    Unavailable,
}

impl LinuxSecureStore {
    /// Ensures credentials never silently fall back to plaintext.
    ///
    /// # Errors
    ///
    /// Fails when no approved secure provider is configured.
    pub const fn require_secure(self) -> Result<(), LinuxError> {
        match self {
            Self::SecretService | Self::PassphraseEncrypted => Ok(()),
            Self::Unavailable => Err(LinuxError::SecureStoreUnavailable),
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum LinuxAutostart {
    Disabled,
    XdgAutostart,
    UserSystemd,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum LinuxPackage {
    AppImage,
    Flatpak,
    NativePackage,
    PortableTarball,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct LinuxReleasePolicy {
    pub package: LinuxPackage,
    pub signed_checksum: bool,
}

impl LinuxReleasePolicy {
    /// Requires release-integrity evidence for every Linux package format.
    ///
    /// # Errors
    ///
    /// Rejects unsigned release artifacts.
    pub const fn validate(self) -> Result<(), LinuxError> {
        if self.signed_checksum {
            Ok(())
        } else {
            Err(LinuxError::UnsignedPackage)
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum LinuxConnectivity {
    Disconnected,
    Connected,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum LinuxCost {
    Unmetered,
    Metered,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum LinuxRoute {
    Direct,
    Vpn,
    CaptivePortal,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct LinuxNetwork {
    pub connectivity: LinuxConnectivity,
    pub cost: LinuxCost,
    pub route: LinuxRoute,
    pub generation: u64,
}

impl From<LinuxNetwork> for NetworkContext {
    fn from(value: LinuxNetwork) -> Self {
        Self {
            availability: if value.connectivity == LinuxConnectivity::Connected {
                NetworkAvailability::Available
            } else {
                NetworkAvailability::Unavailable
            },
            cost: if value.cost == LinuxCost::Metered {
                NetworkCost::Metered
            } else {
                NetworkCost::Unmetered
            },
            path: match value.route {
                LinuxRoute::Direct => NetworkPath::Direct,
                LinuxRoute::Vpn => NetworkPath::Vpn,
                LinuxRoute::CaptivePortal => NetworkPath::CaptivePortal,
            },
            generation: value.generation,
        }
    }
}

#[derive(Clone, Copy, Debug, Error, Eq, PartialEq)]
pub enum LinuxError {
    #[error("Linux desktop path policy is unsafe")]
    UnsafePath,
    #[error("Secret Service or explicit encrypted fallback is required")]
    SecureStoreUnavailable,
    #[error("Linux release package lacks signed checksum evidence")]
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
    fn plaintext_fallback_is_rejected() {
        assert_eq!(
            LinuxSecureStore::Unavailable.require_secure(),
            Err(LinuxError::SecureStoreUnavailable)
        );
    }
}
