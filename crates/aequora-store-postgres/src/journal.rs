//! Journal cursor and timeline declarations.

use aequora_types::{AuthorityEpoch, AuthorityId, Sequence, SyncScopeId, TenantId};

/// Complete durable cursor binding for `PostgreSQL` scans.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PostgresJournalCursor {
    /// Authenticated tenant.
    pub tenant_id: TenantId,
    /// Authorized scope partition.
    pub scope_id: SyncScopeId,
    /// Authority identity.
    pub authority_id: AuthorityId,
    /// Non-decreasing authority epoch.
    pub authority_epoch: AuthorityEpoch,
    /// Last accepted committed timeline position.
    pub sequence: Sequence,
}
