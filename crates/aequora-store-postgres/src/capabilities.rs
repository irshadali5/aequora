//! Named Part 37 conformance profiles.

/// Full authoritative `PostgreSQL` semantics required by Part 37.
pub const POSTGRES_AUTHORITY_FULL_PROFILE: &str = "PostgresAuthorityFull";

/// Operational evidence layered over the `PostgreSQL` authority profile for Neon.
pub const NEON_OPERATIONAL_PROFILE: &str = "NeonOperationalProfile";

/// Stable adapter implementation name used in evidence and diagnostics.
pub const POSTGRES_ADAPTER_IDENTITY: &str = "aequora-postgres";
