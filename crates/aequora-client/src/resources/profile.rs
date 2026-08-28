use aequora_protocol::{ClientLimits, SnapshotLimits};
use serde::{Deserialize, Serialize};
use thiserror::Error;

use super::{
    ClientMemoryLimits, ClientNetworkPolicy, ClientPowerPolicy, ClientThermalPolicy, DataBudget,
    SnapshotCachePolicy, StoragePolicy,
};

/// Safe baseline selected by an application/platform integration.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub enum ClientResourceProfile {
    MobileMinimal,
    MobileStandard,
    DesktopConservative,
    #[default]
    DesktopStandard,
}

/// Coarse capability class disclosed to a server; exact device resources remain local.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum ClientCapabilityClass {
    LowMemory,
    Standard,
}

/// Transport limits and coarse features safe to expose during capability negotiation.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ClientCapabilityProfile {
    pub class: ClientCapabilityClass,
    pub max_request_bytes: u32,
    pub max_snapshot_chunk_bytes: u32,
    pub supports_zstd: bool,
}

impl ClientCapabilityProfile {
    #[must_use]
    pub const fn client_limits(self, max_changes: u32, max_response_bytes: u32) -> ClientLimits {
        ClientLimits {
            max_changes,
            max_response_bytes,
        }
    }

    #[must_use]
    pub const fn snapshot_limits(self, max_entities: u32) -> SnapshotLimits {
        SnapshotLimits {
            max_entities,
            max_payload_bytes: self.max_snapshot_chunk_bytes,
        }
    }
}

/// Complete validated Part 20 resource baseline.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ClientResourcePolicy {
    pub version: u32,
    pub profile: ClientResourceProfile,
    pub memory: ClientMemoryLimits,
    pub storage: StoragePolicy,
    pub network: ClientNetworkPolicy,
    pub power: ClientPowerPolicy,
    pub thermal: ClientThermalPolicy,
    pub data_budget: DataBudget,
    pub supports_zstd: bool,
}

impl ClientResourcePolicy {
    /// Returns conservative provider-neutral defaults for one client class.
    #[must_use]
    #[allow(clippy::too_many_lines)]
    pub const fn for_profile(profile: ClientResourceProfile) -> Self {
        match profile {
            ClientResourceProfile::MobileMinimal => Self {
                version: 1,
                profile,
                memory: ClientMemoryLimits {
                    sync_decode_bytes: 512 * 1_024,
                    sync_response_bytes: 1_024 * 1_024,
                    snapshot_pipeline_bytes: 2 * 1_024 * 1_024,
                    snapshot_chunk_bytes: 1_024 * 1_024,
                    max_batch_operations: 32,
                    max_query_page_rows: 64,
                    max_parallel_transfers: 1,
                    max_cpu_workers: 1,
                    diagnostic_events: 100,
                },
                storage: StoragePolicy {
                    low_watermark_bytes: 512 * 1_024 * 1_024,
                    critical_watermark_bytes: 128 * 1_024 * 1_024,
                    transaction_reserve_bytes: 32 * 1_024 * 1_024,
                    blob_spool_quota_bytes: 128 * 1_024 * 1_024,
                    snapshot_cache: SnapshotCachePolicy::None,
                },
                network: ClientNetworkPolicy {
                    allow_interactive_metered: true,
                    allow_bulk_metered: false,
                    allow_roaming_bulk: false,
                },
                power: ClientPowerPolicy {
                    maintenance_requires_charging: true,
                    defer_bulk_when_constrained: true,
                },
                thermal: ClientThermalPolicy {
                    defer_maintenance_when_hot: true,
                    force_single_worker_when_warm: true,
                },
                data_budget: DataBudget {
                    daily_metered_bytes: None,
                    monthly_metered_bytes: None,
                },
                supports_zstd: true,
            },
            ClientResourceProfile::MobileStandard => Self {
                version: 1,
                profile,
                memory: ClientMemoryLimits {
                    sync_decode_bytes: 2 * 1_024 * 1_024,
                    sync_response_bytes: 4 * 1_024 * 1_024,
                    snapshot_pipeline_bytes: 8 * 1_024 * 1_024,
                    snapshot_chunk_bytes: 4 * 1_024 * 1_024,
                    max_batch_operations: 128,
                    max_query_page_rows: 128,
                    max_parallel_transfers: 2,
                    max_cpu_workers: 2,
                    diagnostic_events: 100,
                },
                storage: StoragePolicy {
                    low_watermark_bytes: 512 * 1_024 * 1_024,
                    critical_watermark_bytes: 128 * 1_024 * 1_024,
                    transaction_reserve_bytes: 64 * 1_024 * 1_024,
                    blob_spool_quota_bytes: 512 * 1_024 * 1_024,
                    snapshot_cache: SnapshotCachePolicy::InstalledOnly,
                },
                network: ClientNetworkPolicy {
                    allow_interactive_metered: true,
                    allow_bulk_metered: false,
                    allow_roaming_bulk: false,
                },
                power: ClientPowerPolicy {
                    maintenance_requires_charging: true,
                    defer_bulk_when_constrained: true,
                },
                thermal: ClientThermalPolicy {
                    defer_maintenance_when_hot: true,
                    force_single_worker_when_warm: true,
                },
                data_budget: DataBudget {
                    daily_metered_bytes: None,
                    monthly_metered_bytes: None,
                },
                supports_zstd: true,
            },
            ClientResourceProfile::DesktopConservative => Self {
                version: 1,
                profile,
                memory: ClientMemoryLimits {
                    sync_decode_bytes: 4 * 1_024 * 1_024,
                    sync_response_bytes: 8 * 1_024 * 1_024,
                    snapshot_pipeline_bytes: 16 * 1_024 * 1_024,
                    snapshot_chunk_bytes: 8 * 1_024 * 1_024,
                    max_batch_operations: 256,
                    max_query_page_rows: 256,
                    max_parallel_transfers: 2,
                    max_cpu_workers: 2,
                    diagnostic_events: 100,
                },
                storage: StoragePolicy {
                    low_watermark_bytes: 1024 * 1_024 * 1_024,
                    critical_watermark_bytes: 256 * 1_024 * 1_024,
                    transaction_reserve_bytes: 128 * 1_024 * 1_024,
                    blob_spool_quota_bytes: 1024 * 1_024 * 1_024,
                    snapshot_cache: SnapshotCachePolicy::InstalledOnly,
                },
                network: ClientNetworkPolicy {
                    allow_interactive_metered: true,
                    allow_bulk_metered: false,
                    allow_roaming_bulk: false,
                },
                power: ClientPowerPolicy {
                    maintenance_requires_charging: false,
                    defer_bulk_when_constrained: true,
                },
                thermal: ClientThermalPolicy {
                    defer_maintenance_when_hot: true,
                    force_single_worker_when_warm: false,
                },
                data_budget: DataBudget {
                    daily_metered_bytes: None,
                    monthly_metered_bytes: None,
                },
                supports_zstd: true,
            },
            ClientResourceProfile::DesktopStandard => Self {
                version: 1,
                profile,
                memory: ClientMemoryLimits {
                    sync_decode_bytes: 8 * 1_024 * 1_024,
                    sync_response_bytes: 16 * 1_024 * 1_024,
                    snapshot_pipeline_bytes: 32 * 1_024 * 1_024,
                    snapshot_chunk_bytes: 16 * 1_024 * 1_024,
                    max_batch_operations: 512,
                    max_query_page_rows: 512,
                    max_parallel_transfers: 4,
                    max_cpu_workers: 4,
                    diagnostic_events: 100,
                },
                storage: StoragePolicy {
                    low_watermark_bytes: 2 * 1024 * 1_024 * 1_024,
                    critical_watermark_bytes: 512 * 1_024 * 1_024,
                    transaction_reserve_bytes: 256 * 1_024 * 1_024,
                    blob_spool_quota_bytes: 2 * 1024 * 1_024 * 1_024,
                    snapshot_cache: SnapshotCachePolicy::KeepRecent,
                },
                network: ClientNetworkPolicy {
                    allow_interactive_metered: true,
                    allow_bulk_metered: true,
                    allow_roaming_bulk: false,
                },
                power: ClientPowerPolicy {
                    maintenance_requires_charging: false,
                    defer_bulk_when_constrained: false,
                },
                thermal: ClientThermalPolicy {
                    defer_maintenance_when_hot: true,
                    force_single_worker_when_warm: false,
                },
                data_budget: DataBudget {
                    daily_metered_bytes: None,
                    monthly_metered_bytes: None,
                },
                supports_zstd: true,
            },
        }
    }

    /// Validates every non-zero/monotonic hard limit.
    ///
    /// # Errors
    ///
    /// Returns a typed policy error for an invalid format, memory, or storage bound.
    pub fn validate(self) -> Result<(), PolicyError> {
        if self.version == 0 {
            return Err(PolicyError::InvalidVersion);
        }
        self.memory.validate()?;
        if !self.storage.valid() {
            return Err(PolicyError::InvalidStorageLimits);
        }
        Ok(())
    }

    /// Builds privacy-coarse negotiation data from local hard bounds.
    #[must_use]
    pub const fn capability_profile(self) -> ClientCapabilityProfile {
        ClientCapabilityProfile {
            class: match self.profile {
                ClientResourceProfile::MobileMinimal | ClientResourceProfile::MobileStandard => {
                    ClientCapabilityClass::LowMemory
                }
                ClientResourceProfile::DesktopConservative
                | ClientResourceProfile::DesktopStandard => ClientCapabilityClass::Standard,
            },
            max_request_bytes: self.memory.sync_decode_bytes,
            max_snapshot_chunk_bytes: self.memory.snapshot_chunk_bytes,
            supports_zstd: self.supports_zstd,
        }
    }
}

impl Default for ClientResourcePolicy {
    fn default() -> Self {
        Self::for_profile(ClientResourceProfile::DesktopStandard)
    }
}

#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
pub enum PolicyError {
    #[error("client resource policy version must be non-zero")]
    InvalidVersion,
    #[error("client memory limits must be non-zero and monotonic")]
    InvalidMemoryLimits,
    #[error("client storage watermarks and reserves are invalid")]
    InvalidStorageLimits,
}
