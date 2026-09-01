//! Structured liveness, readiness, and deep-diagnostic state.

/// Outcome of one fail-closed readiness requirement.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ReadinessCheck {
    /// Requirement is satisfied.
    Passed,
    /// Requirement is not satisfied.
    Failed,
}

impl From<bool> for ReadinessCheck {
    fn from(value: bool) -> Self {
        if value { Self::Passed } else { Self::Failed }
    }
}

/// Readiness result for correctness-critical `PostgreSQL` state.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PostgresReadiness {
    /// Runtime pool can reach the database.
    pub reachable: ReadinessCheck,
    /// The endpoint accepts writer transactions.
    pub writer_available: ReadinessCheck,
    /// Migration ledger exactly matches this build.
    pub schema_current: ReadinessCheck,
    /// Statement, lock, and idle transaction timeouts match policy.
    pub settings_safe: ReadinessCheck,
    /// Durable authority identity and epoch are initialized.
    pub authority_initialized: ReadinessCheck,
}

impl PostgresReadiness {
    /// Whether the adapter may admit authoritative writes.
    #[must_use]
    pub const fn is_ready(self) -> bool {
        matches!(self.reachable, ReadinessCheck::Passed)
            && matches!(self.writer_available, ReadinessCheck::Passed)
            && matches!(self.schema_current, ReadinessCheck::Passed)
            && matches!(self.settings_safe, ReadinessCheck::Passed)
            && matches!(self.authority_initialized, ReadinessCheck::Passed)
    }
}
