//! Retention leases and safe journal-floor decisions.

use aequora_types::{DeviceId, Sequence, SyncScopeId, TenantId};

/// Consumer lease that may constrain journal compaction.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PostgresRetentionLease {
    /// Tenant whose journal is protected.
    pub tenant_id: TenantId,
    /// Device or consumer holding the lease.
    pub device_id: DeviceId,
    /// Scope whose cursor is acknowledged.
    pub scope_id: SyncScopeId,
    /// Highest sequence durably consumed.
    pub ack_sequence: Sequence,
    /// Last activity time in Unix milliseconds.
    pub last_active_unix_ms: u64,
    /// Time after which this lease no longer pins history.
    pub lease_expiry_unix_ms: u64,
}

/// Evidence explaining a bounded compaction decision.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PostgresRetentionDecision {
    /// Requested inclusive deletion boundary.
    pub requested: Sequence,
    /// Maximum boundary allowed by active leases and policy.
    pub safe_through: Sequence,
    /// Whether an available bootstrap snapshot covers the selected floor.
    pub bootstrap_available: bool,
}
