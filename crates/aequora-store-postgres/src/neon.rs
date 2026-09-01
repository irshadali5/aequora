//! Neon-specific operational declarations that never redefine authority semantics.

/// Operational tests required before claiming the Neon deployment profile.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NeonOperationalCheck {
    /// First-query behavior after compute suspension.
    ColdStartLatency,
    /// Connection establishment and recycling behavior.
    ConnectionChurn,
    /// Expand-and-contract migration rehearsal on a branch.
    BranchMigrationRehearsal,
    /// Restore procedure proves an authority epoch transition.
    RestoreEpochProcedure,
}

/// Complete Neon operational evidence set.
pub const REQUIRED_NEON_CHECKS: [NeonOperationalCheck; 4] = [
    NeonOperationalCheck::ColdStartLatency,
    NeonOperationalCheck::ConnectionChurn,
    NeonOperationalCheck::BranchMigrationRehearsal,
    NeonOperationalCheck::RestoreEpochProcedure,
];
