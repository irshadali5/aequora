//! Explicit tenant/scope query binding.

use aequora_types::{SyncScopeId, TenantId};

/// Tenant and synchronization scope that every scoped SQL query must bind.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TenantScope {
    /// Authenticated tenant boundary.
    pub tenant_id: TenantId,
    /// Authorized synchronization scope.
    pub scope_id: SyncScopeId,
}
