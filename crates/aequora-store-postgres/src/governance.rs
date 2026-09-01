//! Restore and governance safety declarations.

use aequora_types::{Sequence, SyncScopeId, TenantId};

/// Tenant/scope retention policy that constrains physical journal compaction.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PostgresRetentionPolicy {
    /// Tenant whose policy is authoritative.
    pub tenant_id: TenantId,
    /// Scope whose history is protected.
    pub scope_id: SyncScopeId,
    /// Oldest sequence that policy still requires to remain available.
    pub retain_from_sequence: Sequence,
    /// Active legal hold prevents all ordinary journal deletion.
    pub legal_hold: bool,
    /// Monotonic policy update time in Unix milliseconds.
    pub updated_at_unix_ms: u64,
}

/// Requirements that must pass before restored authority serves synchronization.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RestoreReadiness {
    /// A new authority epoch was durably recorded.
    pub epoch_advanced: bool,
    /// Completed erasures were reconciled against restored data.
    pub erasures_reconciled: bool,
    /// Revocations were reconciled against restored data.
    pub revocations_reconciled: bool,
}

impl RestoreReadiness {
    /// Whether restored authority can safely become ready.
    #[must_use]
    pub const fn is_ready(self) -> bool {
        self.epoch_advanced && self.erasures_reconciled && self.revocations_reconciled
    }
}
