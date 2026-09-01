//! Structured local replica health and recovery state.

/// Migration state visible to diagnostics and readiness.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MigrationHealth {
    Current,
    RecoveryPending,
    UnsupportedNewerSchema,
}

/// Integrity state visible without exposing payloads.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum IntegrityHealth {
    Verified,
    VerificationPending,
    RecoveryRequired,
}

/// Safe, payload-free health snapshot.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HealthCheckState {
    Ready,
    Unavailable,
}

/// Coordinator/fence readiness.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LeaderHealth {
    CurrentOrFollowerSafe,
    LeadershipLost,
}

/// Safe, payload-free health snapshot.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct StoolapHealth {
    pub store: HealthCheckState,
    pub schema: HealthCheckState,
    pub writes: HealthCheckState,
    pub storage: HealthCheckState,
    pub migration: MigrationHealth,
    pub integrity: IntegrityHealth,
    pub leader: LeaderHealth,
}

impl StoolapHealth {
    /// Whether ordinary local mutation and synchronization may proceed.
    #[must_use]
    pub const fn ready(self) -> bool {
        matches!(self.store, HealthCheckState::Ready)
            && matches!(self.schema, HealthCheckState::Ready)
            && matches!(self.writes, HealthCheckState::Ready)
            && matches!(self.storage, HealthCheckState::Ready)
            && matches!(self.migration, MigrationHealth::Current)
            && !matches!(self.integrity, IntegrityHealth::RecoveryRequired)
    }
}
