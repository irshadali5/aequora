//! Authoritative device metadata.

use aequora_types::{ActorId, DeviceId, Sequence, TenantId};

/// Device eligibility state persisted by the authority.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PostgresDeviceStatus {
    /// May authenticate and synchronize.
    Active,
    /// Explicitly denied and retained as revocation evidence.
    Revoked,
    /// Permanently retired and requires a new binding to return.
    Retired,
}

impl PostgresDeviceStatus {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::Revoked => "revoked",
            Self::Retired => "retired",
        }
    }
}

/// Tenant-bound device record without secret key material.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PostgresDeviceRecord {
    /// Tenant that owns this binding.
    pub tenant_id: TenantId,
    /// Stable device identity.
    pub device_id: DeviceId,
    /// Authenticated actor bound to the device.
    pub actor_id: ActorId,
    /// Public verification key bytes.
    pub public_key: Vec<u8>,
    /// Monotonic secure-binding generation.
    pub binding_generation: u64,
    /// Current device eligibility.
    pub status: PostgresDeviceStatus,
    /// Last observed server time in Unix milliseconds.
    pub last_seen_unix_ms: u64,
    /// Highest durably acknowledged sequence.
    pub last_ack_sequence: Sequence,
}
