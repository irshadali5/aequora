//! Part 37 migration policy declarations.

/// Migration coordination strategy used by the official adapter.
pub const MIGRATION_LOCK_STRATEGY: &str = "transaction-scoped PostgreSQL advisory lock";

/// Rolling changes must remain compatible with old and new application versions.
pub const ROLLING_MIGRATION_STRATEGY: &str = "expand-backfill-switch-contract";
